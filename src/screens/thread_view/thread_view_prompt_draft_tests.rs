use super::*;

#[test]
fn the_unsent_prompt_and_unlogged_sends_survive_a_new_view() {
    let id = "drafttest-0000";
    let (tx, _rx) = crate::infra::bus::channel();
    let mut view = View::new(id, Path::new("/tmp"), "t", tx.clone());

    view.prompt.input.set_text("half a message");
    view.save_prompt();

    // a send the host never logged (the process died before the reply)
    view.prompt
        .pending
        .push(("sent, never logged".into(), Instant::now(), None));
    view.save_prompt();
    drop(view);
    let mut view = View::new(id, Path::new("/tmp"), "t", tx.clone());

    assert_eq!(view.prompt.input.text(), "sent, never logged\n\nhalf a message");

    // an emptied prompt: 0 file
    view.prompt.input.set_text("");
    view.save_prompt();

    assert!(!prompt_path(id).exists());

    // a failed new-thread start: the task waits in the prompt
    save_prompt_draft(id, "the task");
    let view = View::new(id, Path::new("/tmp"), "t", tx);

    assert_eq!(view.prompt.input.text(), "the task");

    let _ = std::fs::remove_dir_all(store::thread_dir(id));
}

/// A new thread's view opens before its worktree: the host's `worktree` result moves the
/// view (dir, changes, files) there. 0 host after the start: the task waits in the prompt.
#[test]
fn the_hosts_worktree_moves_the_view_and_a_missing_host_puts_the_task_in_the_prompt() {
    let root = std::env::temp_dir().join(format!("codehead-view-wt-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);

    std::fs::create_dir_all(&root).unwrap();

    let sh = |script: &str| {
        assert!(
            std::process::Command::new("sh")
                .arg("-c")
                .arg(script)
                .current_dir(&root)
                .output()
                .unwrap()
                .status
                .success(),
            "{script}"
        );
    };

    sh(
        "git init -q -b main r && cd r && git -c user.email=t@t -c user.name=t commit -q --allow-empty -m 1 && git worktree add -q -b feat ../w",
    );
    let (repo, wt) = (root.join("r"), root.join("w"));
    let mut thread = store::Thread::new("wt view", "the task", &repo, vec!["zzwt".into()]);

    thread.worktree_pending = Some("feat".into());
    thread.kickoff = true;
    std::fs::create_dir_all(store::thread_dir(&thread.id)).unwrap();
    thread.save().unwrap();
    let (tx, _rx) = crate::infra::bus::channel();
    let mut view = View::new(&thread.id, &repo, "wt view", tx);

    view.load_checkouts();

    assert_missing_host(&root, &repo, &wt, &thread, view);
}

fn assert_missing_host(root: &Path, repo: &Path, wt: &Path, thread: &store::Thread, mut view: View) {
    assert_eq!(view.panes.changes.dir(), repo);

    // the host: thread.json first, then the log line
    store::update(&thread.id, |thread| {
        thread.worktree = Some(wt.display().to_string());
        thread.worktree_pending = None;
    })
    .unwrap();

    store::append(
        &thread.id,
        &store::Event::new(
            Some("setup"),
            store::Kind::ToolResult {
                id: "worktree".into(),
                ok: true,
                output: String::new(),
            },
        ),
    )
    .unwrap();

    assert!(view.read());
    assert_eq!(
        (
            view.cwd.as_path(),
            view.panes.changes.dir(),
            view.shell_state.shell_cwd.as_path()
        ),
        (wt, wt, wt)
    );

    // 0 host came: the task in the prompt (a typed text stays), kickoff off
    view.kickoff_failed("the task");

    assert_eq!(view.prompt.input.text(), "the task");
    assert!(!store::load(&thread.id).unwrap().kickoff);

    view.prompt.input.set_text("my own words");
    view.kickoff_failed("the task");

    assert_eq!(view.prompt.input.text(), "my own words");

    let _ = std::fs::remove_dir_all(store::thread_dir(&thread.id));
    let _ = std::fs::remove_dir_all(root);
}
