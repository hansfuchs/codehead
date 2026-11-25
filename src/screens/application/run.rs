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
        value @ Outcome::New { .. } => new_thread(value, application),
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
    if let Some(group) = &plan.prep {
        let line = group.iter().map(|text_3| quote(text_3)).collect::<Vec<_>>().join(" ");

        if plan.stash {
            println!(
                "{line} || {{ git -C {} stash push --include-untracked -m 'codehead: before switch' && {line}; }}",
                quote(&repo.display().to_string())
            );
        } else {
            println!("{line}");
        }
    }

    if let Some(bytes_2) = &plan.worktree {
        println!(
            "git -C {} worktree add <codehead data>/worktrees/{} {}",
            quote(&repo.display().to_string()),
            quote(bytes_2),
            quote(bytes_2)
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
        let _ = application.session.term.draw(|file_2| {
            crate::screens::start_screen::draw_status(file_2, &format!("cloning {url}…"), 0);
        });

        if let Err(error_3) = crate::infra::home::clone(url, path) {
            application.session.tty.leave(&mut application.session.out);
            fail(&error_3, 1)
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

    let text_4 = crate::screens::start_screen::start::Start {
        repo: &repo,
        target: &target,
        message: &message,
        agents: &picked,
        flows: &flows,
        projects: projects.as_deref(),
        title_set,
    };

    let (thread_9, note) = match crate::screens::start_screen::start::launch(&text_4, &plan, engine::client::start) {
        Ok(result_value) => result_value,
        Err(error_4) => {
            application.session.tty.leave(&mut application.session.out);
            fail(&error_4, 1)
        }
    };

    let title = thread_9.title.clone();

    // the thread has the message (or its draft): the form starts empty again
    application.app.clear_task();

    Some(open_view(
        &mut application.session,
        &thread_9.id,
        &thread_9.work_dir(),
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
        let _ = application.session.term.draw(|file_3| {
            crate::screens::start_screen::draw_status(
                file_3,
                &format!("starting {} · {}…", plan.label, picked.join(", ")),
                0,
            );
        });
    }
}

fn resume(worktree: &crate::domain::threads::Worktree, application: &mut Application) -> Option<ReturnTarget> {
    let Some(thread_10) = worktree.thread.clone() else {
        application.session.tty.leave(&mut application.session.out);
        return None;
    };

    let dir = PathBuf::from(&worktree.path);

    if application.args.dry {
        application.session.tty.leave(&mut application.session.out);
        println!("codehead --view {}", thread_10.id);
        return None;
    }

    wake(&thread_10.id);
    Some(open_view(
        &mut application.session,
        &thread_10.id,
        &dir,
        &thread_10.title,
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
            Ok(bytes) if bytes.is_empty() => break,
            Ok(bytes_value) => bytes_value,
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
    let nv = crate::domain::notifications::version() + crate::infra::bus::version();

    if nv != application.notes_v {
        application.notes_v = nv;
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
    let _ = application.session.term.draw(|file| application.app.draw(file));

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
        Suspend::Note(path_4) => {
            editor(&path_4);
            application.app.reload_notes();
        }
        Suspend::Prompt => edit_prompt(application),
    }

    let _ = application.session.tty.enter(&mut application.session.out);
    let _ = application.session.term.clear();

    application.dirty = true;
}

fn edit_prompt(application: &mut Application) {
    let file_value = std::env::temp_dir().join(format!("codehead-prompt-{}.md", std::process::id()));

    if std::fs::write(&file_value, application.app.prompt_text()).is_ok() {
        editor(&file_value);

        if let Ok(thread_8) = std::fs::read_to_string(&file_value) {
            application.app.set_prompt(thread_8.trim_end_matches('\n'));
        }

        let _ = std::fs::remove_file(&file_value);
    }
}

fn handle_event(ev: Event, at: Instant, application: &mut Application) -> bool {
    crate::ui::textarea::stamp(Some(at));

    let tag = if engine::trace_on() {
        format!("start: {ev:?}")
    } else {
        String::new()
    };

    let _w = engine::watch(move || tag);

    match ev {
        Event::Key(key_value) if key_value.kind != KeyEventKind::Release => application.app.key(key_value),
        Event::Mouse(message_value) => application.app.mouse(message_value),
        Event::Paste(text_2) => application.app.paste(&text_2),
        _ => {}
    }

    if application.app.outcome.is_some() || application.app.suspend.is_some() {
        crate::ui::textarea::stamp(None);
        return true;
    }

    false
}
