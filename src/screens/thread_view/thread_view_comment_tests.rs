use super::*;

#[test]
fn a_leading_bang_runs_a_shell_command_its_output_goes_along_with_the_next_message() {
    let (tx, rx) = crate::infra::bus::channel();
    let mut view = View::new("shelltest-0000", Path::new("/tmp"), "t", tx);
    let mut thread = Terminal::new(ratatui::backend::TestBackend::new(110, 30)).unwrap();

    thread.draw(|file| view.draw(file)).unwrap();

    let shown = |thread: &Terminal<ratatui::backend::TestBackend>| {
        thread
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|character| character.symbol().to_string())
            .collect::<String>()
    };

    assert!(shown(&thread).contains("! for shell mode"));

    for character in "!echo hi; echo oops >&2; exit 3".chars() {
        view.key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
    }

    thread.draw(|file| view.draw(file)).unwrap();

    assert!(shown(&thread).contains("! shell mode"), "{}", shown(&thread));

    // enter runs it: 0 message to the agents
    view.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert!(view.prompt.input.is_empty() && view.ops.borrow().is_empty());

    let Ok(Msg::Shell(cmd, out, code, dir)) = rx.recv_timeout(Duration::from_secs(10)) else {
        panic!("0 shell result")
    };

    assert_eq!((cmd.as_str(), code), ("echo hi; echo oops >&2; exit 3", 3));
    assert!(out.contains("hi") && out.contains("oops"), "{out}");

    view.shell_done(cmd, out, code, &dir);
    thread.draw(|file| view.draw(file)).unwrap();

    assert!(
        shown(&thread).contains("exit 3") && shown(&thread).contains("⎿ hi") && shown(&thread).contains("in /tmp"),
        "{}",
        shown(&thread)
    );

    // the next message takes the output along (for the agents only), once
    view.prompt.input.set_text("why does it fail?");
    view.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL));
    let sent = view.ops.borrow().last().cloned().unwrap();

    assert_eq!(sent["text"], "why does it fail?");

    let ctx = sent["context"].as_str().unwrap();

    assert!(
        ctx.contains("$ echo hi; echo oops >&2; exit 3  (exit 3, in /tmp)") && ctx.contains("oops"),
        "{ctx}"
    );

    view.prompt.input.set_text("again");
    view.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL));

    assert_eq!(view.ops.borrow().last().unwrap()["context"], "");

    let _ = std::fs::remove_dir_all(crate::engine::store::thread_dir("shelltest-0000"));
}

#[test]
fn shell_mode_completes_project_tasks_and_suggests_from_the_history() {
    let dir = std::env::temp_dir().join(format!("ch-shellcomp-view-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);

    std::fs::create_dir_all(dir.join(".git")).unwrap();
    std::fs::write(
        dir.join("justfile"),
        "# build the app\nbuild:\n    true\n\n# run the tests\ntest:\n    true\n",
    )
    .unwrap();
    let (tx, _rx) = crate::infra::bus::channel();
    let mut view = View::new("shelltest-0002", &dir, "t", tx);

    view.shell_state.shell_cwd = dir.clone();
    view.prompt.comp.catalog.history = Some(std::sync::Arc::new(Vec::new()));
    let mut thread = Terminal::new(ratatui::backend::TestBackend::new(110, 30)).unwrap();

    let shown = |thread: &Terminal<ratatui::backend::TestBackend>| {
        thread
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|character| character.symbol().to_string())
            .collect::<String>()
    };

    let key = |view: &mut View, character: KeyCode| {
        view.key(KeyEvent::new(character, KeyModifiers::NONE));
    };

    // `just `: its recipes at once, with their docs; tab: the 1st in the prompt, enter takes it
    for character in "!just ".chars() {
        key(&mut view, KeyCode::Char(character));
    }

    thread.draw(|file| view.draw(file)).unwrap();

    assert!(
        shown(&thread).contains("build the app") && shown(&thread).contains("run the tests"),
        "{}",
        shown(&thread)
    );

    key(&mut view, KeyCode::Tab);
    key(&mut view, KeyCode::Tab);

    assert_eq!(view.prompt.input.text(), "! just test");

    key(&mut view, KeyCode::Enter);

    assert_eq!(view.prompt.input.text(), "! just test ", "enter took the row, 0 run");
    assert_eq!(view.shell_state.shell_running, 0);

    // the thread's commands: the rest in gray after the cursor, → takes it
    view.thread.items.push(chat::Item::Shell {
        command: "just test --nocapture".into(),
        output: String::new(),
        code: 0,
        ts: 0,
        dir: None,
    });
    thread.draw(|file| view.draw(file)).unwrap();

    assert!(shown(&thread).contains("! just test --nocapture"), "{}", shown(&thread));

    key(&mut view, KeyCode::Right);

    assert_eq!(view.prompt.input.text(), "! just test --nocapture");

    // ↑ with text: the thread's commands, 0 dropdown
    view.prompt.input.set_text("! just b");
    key(&mut view, KeyCode::Up);

    assert_eq!(view.prompt.input.text(), "! just test --nocapture");

    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(crate::engine::store::thread_dir("shelltest-0002"));
}

#[test]
fn shell_mode_cycles_its_commands_and_down_below_them_edits_the_dir() {
    let (tx, _rx) = crate::infra::bus::channel();
    let mut view = View::new("shelltest-0001", Path::new("/tmp"), "t", tx);

    let key = |view: &mut View, character: KeyCode| {
        view.key(KeyEvent::new(character, KeyModifiers::NONE));
    };

    // `!` on an empty prompt: a space after it
    key(&mut view, KeyCode::Char('!'));

    assert_eq!(view.prompt.input.text(), "! ");

    // 1 backspace: shell mode off, the prompt empty
    key(&mut view, KeyCode::Backspace);

    assert!(view.prompt.input.text().is_empty() && !view.shell_mode());

    key(&mut view, KeyCode::Char('!'));

    // a command typed: backspace deletes 1 char only
    key(&mut view, KeyCode::Char('l'));
    key(&mut view, KeyCode::Backspace);

    assert_eq!(view.prompt.input.text(), "! ");

    for character in ["ls", "pwd", "ls"] {
        view.thread.items.push(chat::Item::Shell {
            command: character.into(),
            output: String::new(),
            code: 0,
            ts: 0,
            dir: None,
        });
    }

    view.prompt.input.set_text("! gi");

    // ↑: newest first (a repeat once), ↓ back to the draft
    key(&mut view, KeyCode::Up);

    assert_eq!(view.prompt.input.text(), "! ls");

    key(&mut view, KeyCode::Up);

    assert_eq!(view.prompt.input.text(), "! pwd");

    key(&mut view, KeyCode::Up);

    assert_eq!(view.prompt.input.text(), "! pwd");

    assert_shell_directory(view, key);
}

fn assert_shell_directory(mut view: View, key: impl Fn(&mut View, KeyCode)) {
    key(&mut view, KeyCode::Down);
    key(&mut view, KeyCode::Down);

    assert_eq!(view.prompt.input.text(), "! gi");

    // ↓ on the draft: the dir input; enter: shell mode's dir, kept for the thread
    key(&mut view, KeyCode::Down);

    assert_eq!(
        view.shell_state.cwd_edit.as_ref().map(TextArea::text).as_deref(),
        Some("/tmp/")
    );

    let dir = std::env::temp_dir().join(format!("ch-cwd-{}", std::process::id()));

    std::fs::create_dir_all(&dir).unwrap();
    view.shell_state
        .cwd_edit
        .as_mut()
        .unwrap()
        .set_text(&dir.display().to_string());
    key(&mut view, KeyCode::Enter);

    assert!(view.shell_state.cwd_edit.is_none());
    assert_eq!(view.shell_state.shell_cwd, std::fs::canonicalize(&dir).unwrap());
    assert_eq!(view.prompt.input.text(), "! gi");

    // `cd ..` alone: the dir too, 0 command run
    view.prompt.input.set_text("! cd ..");
    key(&mut view, KeyCode::Enter);

    assert_eq!(
        view.shell_state.shell_cwd,
        std::fs::canonicalize(std::env::temp_dir()).unwrap()
    );
    assert_eq!(view.shell_state.shell_running, 0);

    let (tx, _rx) = crate::infra::bus::channel();
    let width = View::new("shelltest-0001", Path::new("/"), "t", tx);

    assert_eq!(
        width.shell_state.shell_cwd,
        std::fs::canonicalize(std::env::temp_dir()).unwrap(),
        "the pick stays"
    );

    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(crate::engine::store::thread_dir("shelltest-0001"));
}

#[test]
fn cd_alone_changes_the_dir_other_commands_run() {
    assert_eq!(cd_target("cd src"), Some("src"));
    assert_eq!(cd_target("cd"), Some(""));
    assert_eq!(cd_target("cd '~/x'"), Some("~/x"));
    assert_eq!(cd_target("cd src && ls"), None);
    assert_eq!(cd_target("cdk deploy"), None);
    assert_eq!(cd_target("cd a b"), None);
    assert_eq!(cd_target("cd \"My Notes/x\""), Some("My Notes/x"));
}

#[test]
fn a_comment_opens_a_modal_and_the_prompt_keeps_its_draft() {
    let (tx, _rx) = crate::infra::bus::channel();
    let mut view = View::new("commenttest-0000", Path::new("/tmp"), "t", tx);

    view.set_pane(Pane::Changes);
    view.panes.changes.set_files(crate::features::changes::parse(
        "diff --git a/src/a.rs b/src/a.rs\n--- a/src/a.rs\n+++ b/src/a.rs\n@@ -1,3 +1,3 @@\n x\n-a\n+b\n y\n",
    ));
    view.prompt.input.set_text("half a message");
    let mut thread = Terminal::new(ratatui::backend::TestBackend::new(100, 30)).unwrap();

    thread.draw(|file| view.draw(file)).unwrap();
    view.key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE));

    assert!(
        !view.panes.prompt_focus && view.panes.changes.typing(),
        "the modal has the keys"
    );

    view.key(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::NONE));
    view.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL));

    assert_eq!(view.panes.changes.comment_count(), 1);
    assert!(!view.panes.prompt_focus && view.prompt.input.text() == "half a message");

    // esc on a typed comment: asks (the text stays); esc again cancels: 0 comment, 0
    // interrupt timer
    view.key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE));
    view.key(KeyEvent::new(KeyCode::Char('z'), KeyModifiers::NONE));
    view.key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

    assert!(view.panes.changes.typing(), "1 esc: the comment stays");

    view.key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

    assert_eq!(view.panes.changes.comment_count(), 1);
    assert!(!view.panes.changes.typing() && view.last_esc.is_none());
    assert_eq!(view.prompt.input.text(), "half a message");

    let _ = std::fs::remove_dir_all(crate::engine::store::thread_dir("commenttest-0000"));
}

#[test]
fn the_commit_message_lives_in_the_panel() {
    let (tx, _rx) = crate::infra::bus::channel();
    let mut view = View::new("committest-0000", Path::new("/tmp"), "t", tx);

    view.set_pane(Pane::Changes);
    view.prompt.input.set_text("draft");
    view.key(KeyEvent::new(KeyCode::Char('m'), KeyModifiers::NONE));

    assert!(view.panes.changes.typing() && !view.panes.prompt_focus);

    view.key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));

    // 0 staged files: 0 commit, the message stays
    view.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL));

    assert!(view.panes.changes.typing());

    view.key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

    assert!(!view.panes.changes.typing() && view.prompt.input.text() == "draft" && !view.panes.prompt_focus);
}

#[test]
fn a_message_from_the_changes_view_takes_the_selection_along() {
    let (tx, _rx) = crate::infra::bus::channel();
    let mut view = View::new("contexttest-0000", Path::new("/tmp"), "t", tx);

    view.set_pane(Pane::Changes);
    view.panes.changes.set_files(crate::features::changes::parse(
        "diff --git a/src/a.rs b/src/a.rs\n--- a/src/a.rs\n+++ b/src/a.rs\n@@ -1,3 +1,3 @@\n x\n-a\n+b\n y\n",
    ));
    let mut thread = Terminal::new(ratatui::backend::TestBackend::new(100, 30)).unwrap();

    thread.draw(|file| view.draw(file)).unwrap();

    // ↓ ↓ ↓ ↓: `+ b`; space: selected
    for _ in 0..4 {
        view.key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    }

    view.key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE));
    view.key(KeyEvent::new(KeyCode::Char('m'), KeyModifiers::CONTROL));
    view.prompt.input.set_text("stage only these");
    view.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL));
    let sent = view.ops.borrow().last().cloned().unwrap();
    let text = sent["text"].as_str().unwrap();

    assert!(
        text.starts_with("stage only these\n\n[codehead: context from the changes view]") && text.contains("+ b"),
        "{text}"
    );

    // the chat above the prompt shows (narrow: 0 split)
    let text_value = {
        thread.draw(|file| view.draw(file)).unwrap();
        thread.backend().buffer().clone()
    };

    let rows: Vec<String> = (0..30)
        .map(|y| (0..100).map(|x| text_value[(x, y)].symbol().to_string()).collect())
        .collect();

    assert!(rows.iter().any(|line| line.contains("▾ chat · t: fold")), "{rows:#?}");

    let _ = std::fs::remove_dir_all(crate::engine::store::thread_dir("contexttest-0000"));
}

#[test]
fn an_agent_named_in_a_comment_gets_it_at_once_with_its_thread() {
    let id = "mentiontest-0000";
    let _ = std::fs::remove_dir_all(crate::engine::store::thread_dir(id));
    let (tx, _rx) = crate::infra::bus::channel();
    let mut view = View::new(id, Path::new("/tmp"), "t", tx);

    view.agents.members = vec!["REV".into(), "LEAD".into()];
    view.changes_agents();
    view.set_pane(Pane::Changes);
    view.panes.changes.set_files(crate::features::changes::parse(
        "diff --git a/src/a.rs b/src/a.rs\n--- a/src/a.rs\n+++ b/src/a.rs\n@@ -1,3 +1,3 @@\n x\n-a\n+b\n y\n",
    ));
    let mut thread = Terminal::new(ratatui::backend::TestBackend::new(100, 30)).unwrap();

    thread.draw(|file| view.draw(file)).unwrap();

    // ↓ ↓ ↓ ↓: `+ b`; c: the comment modal; a draft in the prompt stays
    view.prompt.input.set_text("my draft");

    for _ in 0..4 {
        view.key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    }

    view.key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE));

    for ch in "@REV why b?".chars() {
        view.key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
    }

    view.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL));

    let sent = view.ops.borrow().last().cloned().unwrap();

    assert_eq!(
        (sent["op"].as_str(), sent["to"].clone()),
        (Some("prompt"), json!(["REV"])),
        "{sent}"
    );
    assert!(
        sent["text"]
            .as_str()
            .unwrap()
            .starts_with("on comment c1 `src/a.rs:2`: @REV why b?"),
        "{sent}"
    );
    assert!(
        sent["context"].as_str().unwrap().contains("- owner: @REV why b?")
            && sent["context"].as_str().unwrap().contains("`comment_reply`"),
        "{sent}"
    );
    assert_eq!(view.prompt.input.text(), "my draft", "the prompt's draft stays");

    let _ = std::fs::remove_dir_all(crate::engine::store::thread_dir(id));
}
