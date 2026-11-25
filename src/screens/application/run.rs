use super::{
    Application, Duration, Event, Instant, KeyEventKind, Outcome, Path, PathBuf, ReturnTarget, Suspend, Target, editor,
    engine, fail, open_view, quote, read_ready, wake,
};
use std::fmt::Write as _;

impl Application {
    pub(super) fn run(&mut self) {
        run_application(self);
    }
}

fn run_application(application: &mut Application) {
    loop {
        events(application);
        let outcome = application.app.outcome.take().unwrap_or(Outcome::Quit);

        let Some(home_tab) = next_home(outcome, application) else {
            return;
        };

        application.transition = Some(Instant::now());
        application.app.return_from_thread();

        if let ReturnTarget::Tab(tab) = home_tab {
            application.app.go_home(&tab);
        }

        application.dirty = true;
    }
}

fn next_home(outcome: Outcome, application: &mut Application) -> Option<ReturnTarget> {
    match outcome {
        Outcome::Quit => {
            application.session.tty.leave(&mut application.session.out);
            None
        }
        Outcome::Copy(paths) => {
            copy_paths(&paths, application);
            None
        }
        new @ Outcome::New { .. } => new_thread(new, application),
        Outcome::Resume(worktree) => resume(&worktree, application),
    }
}

fn print_plan(
    repo: &Path,
    plan: &crate::screens::start_screen::start::Plan,
    target: &Target,
    message: &str,
    picked: &[String],
) {
    if let Some(argv) = &plan.prep {
        let line = argv.iter().map(|arg| quote(arg)).collect::<Vec<_>>().join(" ");

        if plan.stash {
            println!(
                "{line} || {{ git -C {} stash push --include-untracked -m 'codehead: before switch' && {line}; }}",
                quote(&repo.display().to_string())
            );
        } else {
            println!("{line}");
        }
    }

    if let Some(branch) = &plan.worktree {
        println!(
            "git -C {} worktree add <codehead data>/worktrees/{} {}",
            quote(&repo.display().to_string()),
            quote(branch),
            quote(branch)
        );
    }

    if plan.worktree.is_none() && matches!(target, Target::Checkout { .. } | Target::Worktree { .. }) {
        println!(
            "git -C {} pull --ff-only    # on main / master with an upstream",
            quote(&plan.dir.display().to_string())
        );
    }

    println!(
        "thread in {} · agents {} · task:\n{message}",
        plan.dir.display(),
        picked.join(", ")
    );
}

fn copy_paths(paths: &[PathBuf], application: &mut Application) {
    application.session.tty.leave(&mut application.session.out);

    if paths.is_empty() {
        return;
    }

    let mut text = String::new();

    for path in paths {
        let _ = writeln!(text, "{}", path.display());
    }

    if crate::infra::clip::copy_wait(&text) {
        println!("copied {} path(s)", paths.len());
    } else {
        print!("{text}");
        eprintln!("0 clipboard tools found (wl-copy, xclip, xsel). paths printed");
    }
}

fn new_thread(outcome: Outcome, application: &mut Application) -> Option<ReturnTarget> {
    let Outcome::New {
        message,
        target,
        agents: picked,
        clone,
        flows,
        projects,
        title_set,
    } = outcome
    else {
        return None;
    };

    engine::trace("start: submit");
    engine::transition_begin("start: submit → thread view");

    // a home repo not on disk yet: clone it first
    if let Some((url, path)) = &clone {
        let _ = application.session.term.draw(|frame| {
            crate::screens::start_screen::draw_status(frame, &format!("cloning {url}…"), 0);
        });

        if let Err(error) = crate::infra::home::clone(url, path) {
            application.session.tty.leave(&mut application.session.out);
            fail(&error, 1)
        }
    }

    let repo = application.app.cfg.repo.clone().unwrap_or(application.cwd.clone());
    let plan = crate::screens::start_screen::start::plan(&repo, &application.cwd, &target);

    if application.args.dry {
        application.session.tty.leave(&mut application.session.out);
        print_plan(&repo, &plan, &target, &message, &picked);
        return None;
    }

    // a status frame only for a `git switch` (ms to s): else the start takes ~1 ms and a
    // blank frame between the form and the view only flashes
    show_start_status(&picked, &plan, application);

    let start = crate::screens::start_screen::start::Start {
        repo: &repo,
        target: &target,
        message: &message,
        agents: &picked,
        flows: &flows,
        projects: projects.as_deref(),
        title_set,
    };

    let (thread, note) = match crate::screens::start_screen::start::launch(&start, &plan, engine::client::start) {
        Ok(launched) => launched,
        Err(error) => {
            application.session.tty.leave(&mut application.session.out);
            fail(&error, 1)
        }
    };

    let title = thread.title.clone();

    // the thread has the message (or its draft): the form starts empty again
    application.app.clear_task();

    Some(open_view(
        &mut application.session,
        &thread.id,
        &thread.work_dir(),
        &title,
        &application.cwd,
        note,
    ))
}

fn show_start_status(
    picked: &[String],
    plan: &crate::screens::start_screen::start::Plan,
    application: &mut Application,
) {
    if plan.prep.is_some() {
        let _ = application.session.term.draw(|frame| {
            crate::screens::start_screen::draw_status(
                frame,
                &format!("starting {} · {}…", plan.label, picked.join(", ")),
                0,
            );
        });
    }
}

fn resume(worktree: &crate::domain::threads::Worktree, application: &mut Application) -> Option<ReturnTarget> {
    let Some(thread) = worktree.thread.clone() else {
        application.session.tty.leave(&mut application.session.out);
        return None;
    };

    let dir = PathBuf::from(&worktree.path);

    if application.args.dry {
        application.session.tty.leave(&mut application.session.out);
        println!("codehead --view {}", thread.id);
        return None;
    }

    wake(&thread.id);
    Some(open_view(
        &mut application.session,
        &thread.id,
        &dir,
        &thread.title,
        &application.cwd,
        None,
    ))
}

fn events(application: &mut Application) {
    while event_step(application) {}
}

fn event_step(application: &mut Application) -> bool {
    // new notifications / seen marks: the header redraws
    draw_if_due(application);

    if application.app.outcome.is_some() {
        return false;
    }

    if let Some(action) = application.app.suspend.take() {
        suspend(action, application);
        return true;
    }

    // every waiting event before the next draw: a burst (OS text substitution) stays 1 burst
    // sleep until the next frame is due (1 per second: relative times); keys, background
    // results and bus changes wake the loop sooner
    let wait = if crate::ui::dissolve::active() {
        Duration::from_millis(33)
    } else {
        Duration::from_secs(1)
            .saturating_sub(application.drawn.elapsed())
            .max(Duration::from_millis(1))
    };

    let failed = read_events(wait, application);

    if failed {
        return false;
    }

    let _w = engine::watch(|| "start: tick".to_string());

    application.dirty |= application.app.tick();

    true
}

fn read_events(mut wait: Duration, application: &mut Application) -> bool {
    let mut failed = false;

    loop {
        let batch = match read_ready(wait) {
            Ok(ready) if ready.is_empty() => break,
            Ok(ready) => ready,
            Err(_) => {
                failed = true;
                break;
            }
        };

        application.dirty = true;

        if batch
            .into_iter()
            .any(|(event, at)| handle_event(event, at, application))
        {
            break;
        }

        crate::ui::textarea::stamp(None);
        wait = Duration::ZERO;
    }

    failed
}

fn draw_if_due(application: &mut Application) {
    let version = crate::domain::notifications::version() + crate::infra::bus::version();

    if version != application.notes_v {
        application.notes_v = version;
        application.dirty = true;
    }

    // a dissolve under way: 30 frames per second
    let due = application.dirty
        || application.drawn.elapsed() >= Duration::from_secs(1)
        || (crate::ui::dissolve::active() && application.drawn.elapsed() >= Duration::from_millis(33));

    if !due {
        return;
    }

    let _w = engine::watch(|| "start: draw".to_string());
    let _ = application.session.term.draw(|frame| application.app.draw(frame));

    application.dirty = false;
    application.drawn = Instant::now();
    engine::first_frame();

    if let Some(at) = application.transition.take() {
        let took = at.elapsed();

        if took > engine::BUDGET {
            engine::trace(&format!("SLOW {} back to the start screen", took.as_millis()));
        }
    }
}

fn suspend(action: Suspend, application: &mut Application) {
    application.session.tty.leave(&mut application.session.out);

    match action {
        Suspend::Note(path) => {
            editor(&path);
            application.app.reload_notes();
        }
        Suspend::Prompt => edit_prompt(application),
    }

    let _ = application.session.tty.enter(&mut application.session.out);
    let _ = application.session.term.clear();

    application.dirty = true;
}

fn edit_prompt(application: &mut Application) {
    let path = std::env::temp_dir().join(format!("codehead-prompt-{}.md", std::process::id()));

    if std::fs::write(&path, application.app.prompt_text()).is_ok() {
        editor(&path);

        if let Ok(text) = std::fs::read_to_string(&path) {
            application.app.set_prompt(text.trim_end_matches('\n'));
        }

        let _ = std::fs::remove_file(&path);
    }
}

fn handle_event(event: Event, at: Instant, application: &mut Application) -> bool {
    crate::ui::textarea::stamp(Some(at));

    let tag = if engine::trace_on() {
        format!("start: {event:?}")
    } else {
        String::new()
    };

    let _w = engine::watch(move || tag);

    match event {
        Event::Key(key) if key.kind != KeyEventKind::Release => application.app.key(key),
        Event::Mouse(mouse) => application.app.mouse(mouse),
        Event::Paste(text) => application.app.paste(&text),
        _ => {}
    }

    if application.app.outcome.is_some() || application.app.suspend.is_some() {
        crate::ui::textarea::stamp(None);
        return true;
    }

    false
}
