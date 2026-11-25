use std::io::{self, Stdout, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::crossterm::event::{EnableBracketedPaste, EnableMouseCapture, Event, KeyEventKind};
use ratatui::crossterm::execute;
use ratatui::crossterm::terminal;

use crate::infra::terminal::{catch_signals, fail, read_ready, restore_on_panic};
use crate::screens::start_screen::{App, Config, Outcome, Suspend, Target};

use crate::{Args, engine};

use crate::infra::editor::edit_env as editor;
use crate::ui::terminal::Tty;
use thread::{open_view, wake};

mod thread;

type Term = Terminal<CrosstermBackend<crate::ui::frames::Frames>>;

pub(super) fn quote(text: &str) -> String {
    if !text.is_empty()
        && text
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "-_./=:@+,".contains(character))
    {
        text.to_string()
    } else {
        format!("'{}'", text.replace('\'', "'\\''"))
    }
}

pub(crate) fn start(args: Args) {
    let (cwd, cfg) = configuration(&args);

    prepare_runtime(&args);

    let mut session = start_terminal();

    // plain `codehead` (0 options, 0 title): the thread updated last; esc esc: the start screen
    let back = open_initial(&args, &cwd, &mut session);

    // a thread view ended: until the start screen's next frame
    let transition: Option<Instant> = back.is_some().then(Instant::now);

    let app = create_start_screen(&args, cfg, back);

    // draws only after an event, a background read or once per second (relative times):
    // an idle start screen builds 1 frame per second, not 12
    Application {
        app,
        args,
        cwd,
        session,
        transition,
        dirty: true,
        notes_v: crate::domain::notifications::version() + crate::infra::bus::version(),
        drawn: Instant::now(),
    }
    .run();
}

fn prepare_runtime(args: &Args) {
    restore_on_panic();
    catch_signals();

    // a plan window ends: its next one starts 1 min after (1 trigger message per harness)
    if !args.dry {
        engine::warm::watch();
    }

    // agents stopped on a usage limit whose host is gone (a reboot, a kill): their host again,
    // it resumes them at the reset
    if !args.dry {
        resume_hosts();
    }
}

fn resume_hosts() -> std::thread::JoinHandle<()> {
    std::thread::spawn(|| {
        for thread in engine::store::all()
            .into_iter()
            .filter(|thread| !thread.resume.is_empty())
        {
            let _ = engine::client::ensure(&thread.id);
        }
    })
}

fn start_terminal() -> Session {
    let mut out = io::stdout();
    let mut tty = Tty { enhanced: false };

    if let Err(error) = tty.enter(&mut out) {
        tty.leave(&mut out);
        fail(&format!("terminal: {error}"), 1);
    }

    let term = match Terminal::new(crate::ui::frames::backend()) {
        Ok(terminal) => terminal,
        Err(error) => {
            tty.leave(&mut out);
            fail(&format!("terminal: {error}"), 1)
        }
    };

    Session { term, tty, out }
}

fn open_initial(args: &Args, cwd: &Path, session: &mut Session) -> Option<ReturnTarget> {
    let plain = args.view.is_none()
        && args.title.is_empty()
        && args.agent.is_none()
        && args.branch.is_none()
        && !args.copy
        && !args.dry;
    let latest = plain.then(engine::store::latest).flatten().map(|thread| thread.id);

    // the thread view first: the start screen (App) is built when it ends
    crate::domain::notifications::start();
    crate::infra::watch::start();
    crate::domain::landed::start();
    let mut back: Option<ReturnTarget> = None;

    if let Some(id) = args.view.as_ref().or(latest.as_ref()) {
        // 1 thread.json (an id prefix: the newest match), 0 scan of every thread's host and log
        let found = engine::store::load(id).or_else(|| {
            engine::store::all()
                .into_iter()
                .find(|thread| thread.id.starts_with(id.as_str()))
        });

        let Some(thread) = found else {
            session.tty.leave(&mut session.out);
            fail(&format!("no thread {id}"), 2)
        };

        let dir = [thread.worktree.clone(), Some(thread.dir.clone())]
            .into_iter()
            .flatten()
            .map(PathBuf::from)
            .find(|path| path.is_dir())
            .unwrap_or_else(|| cwd.to_path_buf());

        // --view opens a done thread again; the auto-open leaves its state as it is
        if args.view.is_some() {
            wake(&thread.id);
        }

        back = Some(open_view(session, &thread.id, &dir, &thread.title, cwd, None));
    }

    back
}

fn create_start_screen(args: &Args, cfg: Config, back: Option<ReturnTarget>) -> App {
    engine::trace("app new …");
    let mut app = App::new(cfg, &args.title);

    // -b names a branch of the start dir's repo: the setting `repo` stays off then
    if args.branch.is_none()
        && let Some(repo) = crate::infra::config::default_repo()
    {
        app.use_default_repo(&repo);
    }

    engine::trace("app new");

    if let Some(target) = back {
        app.return_from_thread();

        if let ReturnTarget::Tab(tab) = target {
            app.go_home(&tab);
        }
    }

    app
}

fn configuration(args: &Args) -> (PathBuf, Config) {
    if let Err(error) = crate::infra::config::load() {
        fail(&error, 2);
    }

    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let repo = crate::domain::threads::repo_root(&cwd);

    if engine::harnesses::installed().is_empty() {
        let list: Vec<String> = engine::harnesses::ALL
            .iter()
            .map(|harness| format!("  {} ({}): {}", harness.label, harness.bin, harness.install))
            .collect();

        fail(
            &format!(
                "0 harnesses on PATH: codehead's agents run on one of these:\n{}",
                list.join("\n")
            ),
            1,
        );
    }

    engine::trace("harnesses");
    let (picks, agent) = requested_agents(args);

    if args.branch.is_some() && repo.is_none() {
        fail("-b needs a git repo. cd into one first", 2);
    }

    let notes = crate::infra::config::notes();
    let _ = std::fs::create_dir_all(&notes);
    let cfg = Config {
        notes,
        repo,
        cwd: cwd.clone(),
        agent,
        picks,
        branch: args.branch.clone(),
        prefix: crate::infra::config::prefix(),
        copy_mode: args.copy,
        logs: engine::logs_dir(),
        drafts: crate::screens::start_screen::drafts::dir(),
    };

    (cwd, cfg)
}

fn requested_agents(args: &Args) -> (Vec<String>, String) {
    let mut all = crate::domain::agents::cached();
    let picks: Vec<String> = args
        .agent
        .iter()
        .flat_map(|list| list.split(','))
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty())
        .collect();

    if picks.iter().any(|pick| !all.iter().any(|agent| &agent.name == pick)) {
        all = crate::domain::agents::roster_only();
    }

    let agent = crate::domain::agents::default_agent(&all).unwrap_or_default();

    if let Some(bad) = picks.iter().find(|pick| !all.iter().any(|agent| &agent.name == *pick)) {
        fail(
            &format!(
                "no agent named {bad}. agents: {}",
                all.iter()
                    .map(|agent| agent.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            2,
        );
    }

    (picks, agent)
}

struct Application {
    app: App,
    args: Args,
    cwd: PathBuf,
    session: Session,
    transition: Option<Instant>,
    dirty: bool,
    notes_v: u64,
    drawn: Instant,
}

mod run;

struct Session {
    term: Term,
    tty: Tty,
    out: Stdout,
}

enum ReturnTarget {
    Current,
    Tab(crate::screens::quick_menu::Home),
}
