use super::{
    BACK, Coach, DONE, Duration, Form, HashMap, MARKER, Path, QUIT, Rect, Spec, TASK, Task, Terminal, TerminalOptions,
    Viewport, chrome, headless, io, json, layout, new_thread, run_dir, sandbox, sandbox_root, size, size_gate, store,
    style_path, sweep, wipe,
};

type TutorialTerminal = Terminal<ratatui::prelude::CrosstermBackend<crate::ui::frames::Frames>>;
type ThreadExit = crate::screens::thread_view::Exit;

struct TutorialContext<'context> {
    main: &'context str,
    seeded: &'context str,
    repo: &'context Path,
    spec: &'context Spec,
    root: &'context Path,
}
struct TerminalState {
    out: io::Stdout,
    tty: crate::ui::terminal::Tty,
    top: TutorialTerminal,
}
struct StageState {
    coach: Coach,
    tails: HashMap<String, store::Tail>,
    stage: Option<String>,
    prev: String,
}
struct TutorialSession<'context> {
    context: TutorialContext<'context>,
    terminal: TerminalState,
    progress: StageState,
}
#[derive(Clone, Copy)]
struct StageLayout {
    width: u16,
    height: u16,
    view: Rect,
    side: Option<Rect>,
}
enum StageChoice {
    Thread(String),
    Repeat,
    Quit,
}

/// The screen: chrome + quests, then the stages (a thread's view, the new-thread form) in the
/// frame. The exit code.
pub(super) fn show(main: &str, seeded: &str, repo: &Path, spec: &Spec, root: &Path) -> i32 {
    let mut out = io::stdout();
    let mut tty = crate::ui::terminal::Tty { enhanced: false };

    if tty.enter(&mut out).is_err() {
        return BACK;
    }

    let Ok(mut top) = Terminal::new(crate::ui::frames::backend()) else {
        tty.leave(&mut out);
        return BACK;
    };

    wipe(&mut top);

    if let Some(code) = size_gate(&mut top) {
        tty.release(&mut out);
        return code;
    }

    let coach = Coach::new(&spec.crew);

    crate::domain::drafts::save_prompt_draft_files(main, TASK, &[(MARKER.to_string(), style_path(root))]);

    run_session(TutorialSession {
        context: TutorialContext {
            main,
            seeded,
            repo,
            spec,
            root,
        },
        terminal: TerminalState { out, tty, top },
        progress: StageState {
            coach,
            tails: HashMap::new(),
            stage: Some(main.to_string()),
            prev: main.to_string(),
        },
    })
}

fn run_session(mut session: TutorialSession<'_>) -> i32 {
    let code = loop {
        // each stage (and each resize): the layout of the size now, the chrome drawn whole
        let (width, height) = size();
        let (view, side) = layout(width, height);
        let frame = StageLayout {
            width,
            height,
            view,
            side,
        };

        wipe(&mut session.terminal.top);
        let _ = session
            .terminal
            .top
            .draw(|canvas| chrome(canvas, &session.progress.coach, session.context.spec.dry, view, side));

        let id = match prepare_stage(&mut session) {
            StageChoice::Thread(id) => id,
            StageChoice::Repeat => continue,
            StageChoice::Quit => break QUIT,
        };

        let result = run_thread(&mut session, &id, frame);

        if let Some(code) = apply_exit(&mut session, id, result) {
            break code;
        }
    };

    session.terminal.tty.release(&mut session.terminal.out);
    code
}

fn prepare_stage(session: &mut TutorialSession<'_>) -> StageChoice {
    if let Some(id) = session.progress.stage.clone() {
        return StageChoice::Thread(id);
    }

    // the new-thread form
    match new_thread(
        &mut session.terminal.top,
        &mut session.progress.coach,
        session.context.spec,
        session.context.repo,
        session.context.root,
    ) {
        Some(Form::Started(new)) => session.progress.stage = Some(new),
        Some(Form::Quit) => return StageChoice::Quit,
        _ => session.progress.stage = Some(session.progress.prev.clone()),
    }

    StageChoice::Repeat
}

fn apply_exit(
    session: &mut TutorialSession<'_>,
    id: String,
    result: Result<Option<ThreadExit>, String>,
) -> Option<i32> {
    match result {
        Ok(Some(ThreadExit::Open { id: new, .. })) => {
            if new == session.context.seeded {
                session.progress.coach.reach(Task::Switch);
            }

            session.progress.prev = id;
            session.progress.stage = Some(new);
        }
        Ok(Some(ThreadExit::Home(crate::screens::quick_menu::Home::New))) => {
            session.progress.coach.reach(Task::New);
            session.progress.prev = id;
            session.progress.stage = None;
        }
        // other start-screen tabs: not in the tutorial, the thread again
        Ok(None | Some(ThreadExit::Home(_))) => {}
        Ok(Some(ThreadExit::Back)) => return Some(if session.progress.coach.finished() { DONE } else { BACK }),
        Ok(Some(ThreadExit::Quit)) => return Some(QUIT),
        Err(_) => return Some(BACK),
    }

    None
}

fn run_thread(session: &mut TutorialSession<'_>, id: &str, frame: StageLayout) -> Result<Option<ThreadExit>, String> {
    let Ok(mut term) = Terminal::with_options(
        crate::ui::frames::backend(),
        TerminalOptions {
            viewport: Viewport::Fixed(frame.view),
        },
    ) else {
        return Err(String::new());
    };

    let dir = store::load(id).map_or_else(|| session.context.repo.to_path_buf(), |thread| thread.work_dir());
    let title = store::load(id).map(|thread| thread.title).unwrap_or_default();
    let tail = session
        .progress
        .tails
        .entry(id.to_string())
        .or_insert_with(|| store::Tail::new(id));
    let coach = &mut session.progress.coach;
    let top = &mut session.terminal.top;
    let context = &session.context;

    let mut hook = |peek: &crate::screens::thread_view::Peek| {
        let events = tail.read();

        if coach.see(&events) | coach.see_view(peek) | coach.see_repo(context.repo) {
            let _ = top.draw(|canvas| chrome(canvas, &*coach, context.spec.dry, frame.view, frame.side));
        }

        // a resize: the view ends, the loop lays the screen out again
        size() != (frame.width, frame.height)
    };

    crate::screens::thread_view::run_with(&mut term, id, &dir, &title, None, context.main, Some(&mut hook))
}

/// `codehead tutorial [--check] <spec json>`.
pub(crate) fn run(args: &[String]) -> ! {
    let check = args.first().is_some_and(|agent| agent == "--check");
    let arg = args
        .iter()
        .find(|agent_value| agent_value.starts_with('{'))
        .map(String::as_str);
    let spec: Spec = arg
        .and_then(|agent_2| serde_json::from_str(agent_2).ok())
        .unwrap_or_else(|| crate::infra::terminal::fail("codehead tutorial: spec json required", 2));

    if spec.crew.is_empty() {
        crate::infra::terminal::fail("codehead tutorial: 0 agents", 2);
    }

    // the owner's look before the env moves to the sandbox
    let look = crate::infra::config::read_layers(&crate::infra::config::path())
        .map(|line| line.merged())
        .unwrap_or_default();

    sweep();
    let root = sandbox_root(std::process::id());
    let _ = std::fs::remove_dir_all(&root);
    let run = run_dir(std::process::id());
    let _ = std::fs::create_dir_all(root.join("home"));

    // 1 thread at this point: 0 other thread reads the env. HOME too: drafts, seen marks, paste
    // dirs, snapshots (~/.cache/codehead/…) stay in the sandbox, 0 trace in the owner's lists
    unsafe {
        std::env::set_var("HOME", root.join("home"));
        std::env::set_var("CODEHEAD_DRAFTS", root.join("home/drafts"));
        std::env::set_var("CODEHEAD_RUN", &run);
        std::env::set_var("CODEHEAD_HOME", root.join("data"));
        std::env::set_var("XDG_CONFIG_HOME", root.join("config"));
        std::env::set_var("CODEHEAD_CONFIG", root.join("config/codehead.toml"));
        std::env::set_var("CODEHEAD_HOME_REPO", root.join("home-repo"));

        // the mocks (IDE, browser, GitHub): on, for the sandbox repo
        std::env::set_var("CODEHEAD_TUTORIAL", root.join("hello"));
        std::env::set_var("CODEHEAD_AGENT", "");
        std::env::set_var("CODEHEAD_REPO", "");
        std::env::remove_var("CODEHEAD_NOTES");
    }

    let code = match sandbox(&root, &look, &spec.crew) {
        Ok((id, seeded, repo)) => {
            let code = if check {
                headless(&id, &repo, &spec, &root)
            } else {
                show(&id, &seeded, &repo, &spec, &root)
            };

            for thread_value in store::all() {
                let _ = crate::engine::client::send_within(
                    &thread_value.id,
                    &json!({"op": "stop"}),
                    Duration::from_secs(2),
                );
            }

            std::thread::sleep(Duration::from_millis(300));
            code
        }
        Err(error) => {
            eprintln!("tutorial: {error}");
            BACK
        }
    };

    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(&run);

    std::process::exit(code)
}
