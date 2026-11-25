use super::{
    EnableBracketedPaste, EnableMouseCapture, Path, ReturnTarget, Session, Write, engine, execute, fail, terminal,
};

/// Append 1 line to the view log (why the thread view did not open).
pub(super) fn view_log(msg: &str) {
    let dir = engine::logs_dir();
    let _ = std::fs::create_dir_all(&dir);

    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("view.log"))
    {
        let _ = writeln!(file, "{} {msg}", engine::now_ms() / 1000);
    }
}

/// Thread ID before its view: a done thread opens again. Its host starts with the next message.
pub(super) fn wake(id: &str) {
    if engine::store::load(id).is_some_and(|thread| thread.state != "open") {
        let _ = engine::store::update(id, |thread_value| thread_value.state = "open".into());
    }
}

/// Thread view.
/// Esc esc: the retained start screen in HOME, without another process. Some(tab): the menu asked
/// for that tab of the start screen.
pub(super) fn open_view(
    session: &mut Session,
    id: &str,
    cwd: &Path,
    title: &str,
    home: &Path,
    note: Option<String>,
) -> ReturnTarget {
    let (mut id, mut cwd, mut title) = (id.to_string(), cwd.to_path_buf(), title.to_string());

    // the thread header keeps the thread opened first at index 0
    let first = id.clone();
    let mut note = note;
    let _ = terminal::enable_raw_mode();
    let _ = execute!(&mut session.out, EnableMouseCapture, EnableBracketedPaste);

    loop {
        let _ = std::env::set_current_dir(&cwd);

        match crate::screens::thread_view::run(&mut session.term, &id, &cwd, &title, note.as_deref(), &first) {
            Ok(crate::screens::thread_view::Exit::Quit) => {
                session.tty.leave(&mut session.out);
                std::process::exit(0)
            }
            Ok(crate::screens::thread_view::Exit::Open {
                id: new,
                dir,
                title: new_title,
                note: new_note,
            }) => {
                note = Some(new_note);
                wake(&new);

                // the thread's own work dir (a menu row without a dir: the dir of the thread before)
                cwd = engine::store::load(&new)
                    .map(|thread| thread.work_dir())
                    .filter(|path| path.is_dir())
                    .unwrap_or(dir);
                id = new;
                title = new_title;
            }
            Ok(crate::screens::thread_view::Exit::Back) => {
                // Keep App, terminal modes, lists, previews and form state in memory.
                let _ = std::env::set_current_dir(home);

                return ReturnTarget::Current;
            }
            Ok(crate::screens::thread_view::Exit::Home(tab)) => {
                let _ = std::env::set_current_dir(home);

                return ReturnTarget::Tab(tab);
            }
            Err(error) => {
                view_log(&format!("thread {id}: view failed: {error}"));
                session.tty.leave(&mut session.out);
                fail(&format!("thread view: {error}"), 1)
            }
        }
    }
}
