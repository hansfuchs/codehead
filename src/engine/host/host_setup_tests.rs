use super::*;

#[test]
pub(crate) fn a_prompt_replies_first_and_a_pending_pull_holds_the_first_turn() {
    let _env = crate::engine::ENV_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let base = std::env::temp_dir().join(format!("codehead-host-pull-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);

    std::fs::create_dir_all(&base).unwrap();

    let sh = |character: &str| {
        assert!(
            std::process::Command::new("sh")
                .arg("-c")
                .arg(character)
                .current_dir(&base)
                .output()
                .unwrap()
                .status
                .success(),
            "{character}"
        );
    };

    let cfg = "git config user.email t@t && git config user.name t";

    sh(&format!(
        "git init -q --bare -b main origin.git && git clone -q origin.git a && cd a && {cfg} && git commit -q --allow-empty -m 1 && git push -q origin main"
    ));
    sh(&format!(
        "git clone -q origin.git b && cd b && {cfg} && git commit -q --allow-empty -m 2 && git push -q origin main"
    ));
    let agent = base.join("a");
    let mut thread = store::Thread::new("pull test", "task", &agent, vec!["nobody-agent".into()]);

    thread.pull = true;
    std::fs::create_dir_all(store::thread_dir(&thread.id)).unwrap();
    thread.save().unwrap();
    verify_a_prompt_replies_first_and_a_pending_pull_holds_the_first_turn(base, &thread);
}

fn verify_a_prompt_replies_first_and_a_pending_pull_holds_the_first_turn(base: PathBuf, thread: &store::Thread) {
    let (htx, _hrx) = mpsc::channel();
    let mut path = test_host(thread.id.clone(), htx);
    let (tx, rx) = mpsc::channel();

    path.start_pull(thread, tx);

    // the reply comes before any delivery: the message waits in LATER
    let result = path
        .cmd(&json!({"op": "prompt", "to": ["nobody-agent"], "text": "go"}))
        .unwrap();

    assert_eq!(result["to"], json!(["nobody-agent"]));
    assert_eq!((path.later.len(), path.queue.len()), (1, 0));

    // after the reply: the pull still runs, so the message waits for it
    path.deliver_later();

    assert_eq!((path.later.len(), path.queue.len()), (0, 1));

    let Ok(Input::Pull(ok, out)) = rx.recv_timeout(Duration::from_secs(10)) else {
        panic!("0 pull end")
    };

    assert_eq!((ok, out.as_str()), (true, "main up to date"));

    path.pull_done(ok, out);

    assert!(!path.setup_running && path.queue.is_empty());
    assert!(!store::load(&thread.id).unwrap().pull);

    let rev = |directory: &str| {
        String::from_utf8(
            std::process::Command::new("git")
                .args(["-C", directory, "rev-parse", "HEAD"])
                .current_dir(&base)
                .output()
                .unwrap()
                .stdout,
        )
        .unwrap()
    };

    assert_eq!(rev("a"), rev("b"), "main pulled");

    let _ = std::fs::remove_dir_all(store::thread_dir(&thread.id));
    let _ = std::fs::remove_dir_all(base);
}
/// A new thread (`kickoff`, `worktree_pending`): the host logs the task at once, the task waits
/// for the worktree; the worktree lands in thread.json (path, branch, port), then the task goes
/// out. A failed worktree: the task goes with a note on it.
#[test]
pub(crate) fn a_kickoff_logs_the_task_and_a_pending_worktree_holds_it() {
    let _env = crate::engine::ENV_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let base = std::env::temp_dir().join(format!("codehead-host-kickoff-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);

    std::fs::create_dir_all(&base).unwrap();

    assert_shell_success(
        &base,
        "git init -q -b main r && cd r && git -c user.email=t@t -c user.name=t commit -q --allow-empty -m 1",
    );
    let repo = base.join("r");
    let branch = format!("kick-{}", std::process::id());
    let mut thread = store::Thread::new("kickoff test", "do the thing", &repo, vec!["nobody-agent".into()]);

    thread.kickoff = true;
    thread.worktree_pending = Some(branch.clone());
    std::fs::create_dir_all(store::thread_dir(&thread.id)).unwrap();
    thread.save().unwrap();
    let (htx, _hrx) = mpsc::channel();
    let mut path = test_host(thread.id.clone(), htx);
    let (tx, rx) = mpsc::channel();

    path.start_worktree(&thread, tx.clone());
    path.kickoff(&thread);

    let prompts = |id: &str| {
        store::events(id)
            .into_iter()
            .filter(|error_value| matches!(&error_value.kind, Kind::Prompt { text, .. } if text == "do the thing"))
            .count()
    };

    assert_eq!(prompts(&thread.id), 1, "the task in the log at once");
    assert!(!store::load(&thread.id).unwrap().kickoff);
    assert_eq!(
        (path.later.len(), path.queue.len()),
        (0, 1),
        "the task waits for the worktree"
    );

    // a 2nd host start (a crash, a restart): 0 2nd copy
    path.kickoff(&store::load(&thread.id).unwrap());

    assert_eq!(prompts(&thread.id), 1);

    let Ok(Input::Worktree(result)) = rx.recv_timeout(Duration::from_secs(20)) else {
        panic!("0 worktree end")
    };

    path.worktree_done(result, tx.clone());
    let after = store::load(&thread.id).unwrap();
    let wt = PathBuf::from(after.worktree.clone().expect("worktree in thread.json"));

    assert!(wt.join(".git").is_file());
    assert_eq!(
        (
            after.branch.as_deref(),
            after.worktree_pending.as_deref(),
            after.port.is_some()
        ),
        (Some(branch.as_str()), None, true)
    );

    verify_a_kickoff_logs_the_task_and_a_pending_worktree_holds_it(base, &repo, &branch, &thread, path, (tx, &rx), &wt);
}

fn verify_a_kickoff_logs_the_task_and_a_pending_worktree_holds_it(
    base: PathBuf,
    repo: &Path,
    branch: &str,
    thread: &store::Thread,
    mut path: Host,
    (tx, rx): (Sender<Input>, &mpsc::Receiver<Input>),
    wt: &Path,
) {
    assert!(
        !path.setup_running && path.queue.is_empty(),
        "the task went out (its delivery failed: 0 such agent)"
    );

    // the branch's worktree is there: a 2nd one fails, the task goes with a note
    let mut t2 = store::Thread::new("kickoff test 2", "again", repo, vec!["nobody-agent".into()]);

    t2.worktree_pending = Some(branch.to_string());
    std::fs::create_dir_all(store::thread_dir(&t2.id)).unwrap();
    t2.save().unwrap();
    path.id = t2.id.clone();
    path.start_worktree(&t2, tx.clone());
    path.queue.push(("nobody-agent".into(), "again".into()));

    let Ok(Input::Worktree(result_value)) = rx.recv_timeout(Duration::from_secs(20)) else {
        panic!("0 worktree end")
    };

    assert!(result_value.is_err());

    path.worktree_done(result_value, tx);

    assert!(!path.setup_running && path.queue.is_empty() && store::load(&t2.id).unwrap().worktree_pending.is_none());

    let failed = store::events(&t2.id)
        .into_iter()
        .any(|error_2| matches!(&error_2.kind, Kind::ToolResult { id, ok: false, .. } if id == "worktree"));

    assert!(failed);

    let _ = std::process::Command::new("git")
        .args([
            "-C",
            &repo.display().to_string(),
            "worktree",
            "remove",
            "--force",
            &wt.display().to_string(),
        ])
        .output();
    let _ = std::fs::remove_dir_all(store::thread_dir(&thread.id));
    let _ = std::fs::remove_dir_all(store::thread_dir(&t2.id));
    let _ = std::fs::remove_dir_all(base);
}
#[test]
pub(crate) fn a_pending_setup_runs_first_and_messages_wait_for_it() {
    // the data dir stays put while this runs
    let _env = crate::engine::ENV_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let wt = std::env::temp_dir().join(format!("codehead-host-setup-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&wt);

    std::fs::create_dir_all(wt.join(".codehead")).unwrap();
    std::fs::write(wt.join(".codehead/setup"), "echo installed on $CODEHEAD_PORT\n").unwrap();
    let mut thread = store::Thread::new("setup test", "task", &wt, vec!["nobody-agent".into()]);

    thread.worktree = Some(wt.display().to_string());
    thread.port = Some(40090);
    thread.setup = Some("pending".into());
    std::fs::create_dir_all(store::thread_dir(&thread.id)).unwrap();
    thread.save().unwrap();
    let (htx, _hrx) = mpsc::channel();
    let mut path = test_host(thread.id.clone(), htx);

    path.resume = thread.resume.clone().into_iter().collect();
    let (tx, rx) = mpsc::channel();

    path.start_setup(&thread, tx);

    assert!(path.setup_running);

    // a message now: it waits
    path.deliver("nobody-agent", "go").unwrap();

    assert_eq!(path.queue.len(), 1);

    let Ok(Input::Setup(label, ok, out)) = rx.recv_timeout(Duration::from_secs(10)) else {
        panic!("0 setup end")
    };

    assert_eq!(
        (label.as_str(), ok, out.trim()),
        (".codehead/setup", true, "installed on 40090")
    );

    path.setup_done(&label, ok, out);

    assert!(!path.setup_running && path.queue.is_empty());
    assert_eq!(store::load(&thread.id).unwrap().setup.as_deref(), Some("done"));

    let kinds: Vec<String> = store::events(&thread.id)
        .iter()
        .map(|error_value| {
            format!("{:?}", error_value.kind)
                .split([' ', '{', '('])
                .next()
                .unwrap()
                .to_string()
        })
        .collect();

    assert!(
        kinds.starts_with(&["Tool".to_string(), "ToolResult".to_string()]),
        "{kinds:?}"
    );

    let _ = std::fs::remove_dir_all(store::thread_dir(&thread.id));
    let _ = std::fs::remove_dir_all(wt);
}

fn assert_shell_success(base: &Path, character: &str) {
    assert!(
        std::process::Command::new("sh")
            .arg("-c")
            .arg(character)
            .current_dir(base)
            .output()
            .unwrap()
            .status
            .success(),
        "{character}"
    );
}
