use super::{
    Coach, Duration, FLOW_TASK, Form, Path, Rect, Spec, Task, Terminal, TerminalOptions, Viewport, chrome, json,
    layout, size, store, wipe,
};

/// The real new-thread form in the frame, the prompt ready: a flow picked, started → the new
/// thread (with its flow; 0 flow picked: chore). None: the terminal failed.
pub(super) fn new_thread(
    top: &mut Terminal<ratatui::backend::CrosstermBackend<crate::ui::frames::Frames>>,
    coach: &mut Coach,
    spec: &Spec,
    repo: &Path,
    root: &Path,
) -> Option<Form> {
    let mut app = tutorial_form(spec, repo, root);

    let fixed = |view: Rect| {
        Terminal::with_options(
            crate::ui::frames::backend(),
            TerminalOptions {
                viewport: Viewport::Fixed(view),
            },
        )
        .ok()
    };

    let mut wh = size();
    let (mut view, mut side) = layout(wh.0, wh.1);
    let mut term = fixed(view)?;
    let (mut last_esc, mut last_ctrl_c) = (None, None);

    loop {
        // a resize: the chrome whole again, a terminal for the new frame (the form keeps its state)
        if size() != wh {
            wh = size();
            (view, side) = layout(wh.0, wh.1);
            wipe(top);
            let _ = top.draw(|file| chrome(file, coach, spec.dry, view, side));

            term = fixed(view)?;
        }

        let _ = term.draw(|file_value| app.draw(file_value));
        let (_, flow) = app.flow_peek();

        if flow.is_some() && coach.tick(Task::Flow) {
            let _ = top.draw(|file_2| chrome(file_2, coach, spec.dry, view, side));
        }

        if let Some(output) = app.outcome.take() {
            return Some(match output {
                crate::screens::start_screen::Outcome::New {
                    message, agents, flows, ..
                } => start_thread(coach, repo, &message, agents, flows, &spec.crew[0].name)
                    .map_or(Form::Back, Form::Started),
                _ => Form::Back,
            });
        }

        // an editor on a note / the prompt: not in the tutorial
        let _ = app.suspend.take();

        // Err: a signal or the terminal gone, the tutorial ends
        let Ok(batch) = crate::infra::terminal::read_ready(Duration::from_millis(50)) else {
            return Some(Form::Quit);
        };

        if let Some(value) = batch
            .into_iter()
            .find_map(|(event, _)| new_thread_twice(&mut app, &mut last_esc, &mut last_ctrl_c, event))
        {
            return Some(value);
        }

        app.tick();
    }
}

fn new_thread_twice(
    app: &mut crate::screens::start_screen::App,
    last_esc: &mut Option<std::time::Instant>,
    last_ctrl_c: &mut Option<std::time::Instant>,
    ev: ratatui::crossterm::event::Event,
) -> Option<Form> {
    match ev {
        ratatui::crossterm::event::Event::Key(key_value)
            if key_value.kind != ratatui::crossterm::event::KeyEventKind::Release =>
        {
            // esc esc: back to the thread before (the real form goes to the start
            // screen); ctrl+c ctrl+c: the tutorial ends, as in a thread
            use ratatui::crossterm::event::{KeyCode, KeyModifiers};

            let twice = |last: &mut Option<std::time::Instant>| {
                let hit = last.is_some_and(|thread| thread.elapsed() < Duration::from_millis(500));

                *last = Some(std::time::Instant::now());
                hit
            };

            if key_value.code == KeyCode::Esc && twice(last_esc) {
                return Some(Form::Back);
            }

            if key_value.code == KeyCode::Char('c')
                && key_value.modifiers.contains(KeyModifiers::CONTROL)
                && twice(last_ctrl_c)
            {
                return Some(Form::Quit);
            }

            app.key(key_value);
        }
        ratatui::crossterm::event::Event::Mouse(message_value) => app.mouse(message_value),
        ratatui::crossterm::event::Event::Paste(text) => app.paste(&text),
        _ => {}
    }

    None
}

/// The new thread of the form: in the sandbox repo, each agent's flow (0 picked: chore for the
/// lead), the message to its agents. Its id.
pub(super) fn start_thread(
    coach: &mut Coach,
    repo: &Path,
    message: &str,
    agents: Vec<String>,
    flows: Vec<(String, String, usize)>,
    lead: &str,
) -> Option<String> {
    let agents = if agents.is_empty() {
        vec![lead.to_string()]
    } else {
        agents
    };

    let title: String = message.lines().next().unwrap_or("flows").chars().take(60).collect();
    let mut thread = store::Thread::new(&title, message, repo, agents.clone());

    let flows = if flows.is_empty() {
        thread
            .lead()
            .map(|line| vec![(line.to_string(), "chore".to_string(), 0)])
            .unwrap_or_default()
    } else {
        flows
    };

    let lib = crate::engine::flows::Library::load(Some(repo));

    for (agent, slug, at) in flows {
        if let Some(fl) = lib.flow(&slug) {
            thread.set_flow(
                &agent,
                Some(crate::engine::flows::run::FlowState::new(
                    fl,
                    &lib,
                    at,
                    crate::engine::now_ms(),
                )),
            );
        }
    }

    thread.save().ok()?;
    let _ = crate::engine::client::call(&thread.id, &json!({"op": "prompt", "to": agents, "text": message}));

    coach.reach(Task::Start);

    Some(thread.id)
}

fn tutorial_form(spec: &Spec, repo: &Path, root: &Path) -> crate::screens::start_screen::App {
    let notes = root.join("home-repo/notes");
    let cfg = crate::screens::start_screen::Config {
        notes,
        repo: Some(repo.to_path_buf()),
        cwd: repo.to_path_buf(),
        agent: spec.crew[0].name.clone(),
        picks: Vec::new(),
        branch: None,
        prefix: "task".into(),
        copy_mode: false,
        logs: crate::engine::logs_dir(),
        drafts: crate::screens::start_screen::drafts::dir(),
    };
    let mut app = crate::screens::start_screen::App::new(cfg, "");

    app.go_home(&crate::screens::quick_menu::Home::New);
    app.set_prompt(FLOW_TASK);

    app
}
