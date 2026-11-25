use super::*;

/// The thread's host watches its sub-threads (0 view needed): one deleted without its end and
/// one whose host is down 20 s after its start end as failed; the held messages go.
#[test]
pub(crate) fn the_host_ends_gone_and_lost_sub_threads() {
    let env_lock = crate::engine::ENV_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut lost = store::Thread::new("lost one", "x", Path::new("/tmp"), vec!["ada".into()]);

    lost.created = crate::engine::now_ms() - 60_000;
    std::fs::create_dir_all(store::thread_dir(&lost.id)).unwrap();
    lost.save().unwrap();
    let mut young = store::Thread::new("young one", "x", Path::new("/tmp"), vec!["ada".into()]);

    young.of = Some("x".into());
    std::fs::create_dir_all(store::thread_dir(&young.id)).unwrap();
    young.save().unwrap();
    let mut thread = store::Thread::new("main", "task", Path::new("/tmp"), vec!["ada".into()]);

    thread.kids = vec!["gone-kid".into(), lost.id.clone(), young.id.clone()];
    std::fs::create_dir_all(store::thread_dir(&thread.id)).unwrap();
    thread.save().unwrap();
    let (htx, harness_rx) = mpsc::channel();
    let mut host = test_host(thread.id.clone(), htx);

    assert!(host.has_kids());

    // 5 s between looks
    host.kids_check();

    assert_eq!(store::load(&thread.id).unwrap().kids.len(), 3);

    host.kids_at = Instant::now().checked_sub(Duration::from_secs(6)).unwrap();
    host.kids_check();

    // the gone one and the lost one end; the young one (host not up yet) runs on
    assert_eq!(store::load(&thread.id).unwrap().kids, [young.id.clone()]);
    assert!(store::load(&lost.id).is_none(), "the lost one's thread goes");

    let ends: Vec<(String, String)> = store::events(&thread.id)
        .into_iter()
        .filter_map(|event| match event.kind {
            Kind::SubThread { state, text, .. } => Some((state, text)),
            _ => None,
        })
        .collect();

    assert_eq!(ends.len(), 2, "{ends:?}");
    assert!(
        ends.iter().all(|(state, _)| state == "failed") && ends[1].1.contains("host ended"),
        "{ends:?}"
    );

    for id in [&thread.id, &young.id] {
        let _ = std::fs::remove_dir_all(store::thread_dir(id));
    }

    drop(harness_rx);
    drop(env_lock);
}

/// A running sub-thread holds the thread's messages (same checkout); its end logs its result
/// and lets them go; an end of a sub-thread not running: 0 effect.
#[test]
pub(crate) fn a_sub_thread_holds_messages_until_it_reports_its_end() {
    let env_lock = crate::engine::ENV_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let thread = store::Thread::new("main", "task", Path::new("/tmp"), vec!["ada".into()]);

    std::fs::create_dir_all(store::thread_dir(&thread.id)).unwrap();
    thread.save().unwrap();
    let (htx, harness_rx) = mpsc::channel();
    let mut host = test_host(thread.id.clone(), htx);

    host.cmd(&json!({"op": "sub_start", "id": "kid1", "title": "commit it", "text": "@ada · commit it"}))
        .unwrap();

    assert_eq!(store::load(&thread.id).unwrap().kids, ["kid1"]);

    // 0 harness spawned: the message waits
    host.deliver("ada", "next task").unwrap();
    host.deliver("ada", "and more").unwrap();

    assert_eq!(host.held.len(), 2);

    // an unknown end: 0 log, still held
    host.cmd(&json!({"op": "sub_done", "id": "other", "state": "done", "text": "x"}))
        .unwrap();

    assert_eq!(host.held.len(), 2);

    host.cmd(&json!({"op": "sub_done", "id": "kid1", "title": "commit it", "agent": "ada", "state": "done", "text": "3 commits"})).unwrap();

    // its end: a message of its agent
    assert_eq!(
        store::events(&thread.id)
            .last()
            .and_then(|event| event.agent.clone())
            .as_deref(),
        Some("ada")
    );
    assert!(store::load(&thread.id).unwrap().kids.is_empty() && host.held.is_empty());
    assert_eq!(
        host.later,
        [
            ("ada".to_string(), "next task".to_string()),
            ("ada".to_string(), "and more".to_string())
        ]
    );

    let subs: Vec<(String, String)> = store::events(&thread.id)
        .into_iter()
        .filter_map(|event| match event.kind {
            Kind::SubThread { state, text, .. } => Some((state, text)),
            _ => None,
        })
        .collect();

    assert_eq!(
        subs.iter().map(|sub| sub.0.as_str()).collect::<Vec<_>>(),
        ["running", "held", "done"],
        "1 held line, not 1 per message: {subs:?}"
    );
    assert_eq!(subs[2].1, "3 commits");

    let _ = std::fs::remove_dir_all(store::thread_dir(&thread.id));

    drop(harness_rx);
    drop(env_lock);
}

#[test]
pub(crate) fn subagents_go_into_the_log_and_end_lost_with_their_process() {
    let env_lock = crate::engine::ENV_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let thread = store::Thread::new("subs", "task", Path::new("/tmp"), vec!["ada".into()]);

    std::fs::create_dir_all(store::thread_dir(&thread.id)).unwrap();
    thread.save().unwrap();
    let (htx, harness_rx) = mpsc::channel();
    let mut host = test_host(thread.id.clone(), htx);

    host.resume = thread.resume.clone().into_iter().collect();

    host.harness(
        "ada".into(),
        Out::Event(Kind::Tool {
            id: "t1".into(),
            name: "Agent".into(),
            input: json!({"description": "scan", "run_in_background": true, "prompt": "scan it"}),
        }),
    );
    host.harness(
        "ada".into(),
        Out::Event(Kind::ToolResult {
            id: "t1".into(),
            ok: true,
            output: "Async agent launched\nagentId: a1".into(),
        }),
    );

    // the master's turn ended: the subagent keeps the host alive, the master counts as busy
    assert!(host.subagents.any());

    let ping = host.cmd(&json!({"op": "ping"})).unwrap();

    assert!(ping["busy"].as_array().unwrap().is_empty(), "0 session: 0 busy entry");

    host.harness("ada".into(), Out::Exit(String::new()));
    let kinds: Vec<Kind> = store::events(&thread.id).into_iter().map(|event| event.kind).collect();

    assert!(
        matches!(&kinds[1], Kind::SubStart { id, background: true, prompt, .. } if id == "t1" && prompt == "scan it"),
        "{kinds:?}"
    );
    assert!(
        kinds
            .iter()
            .any(|kind| matches!(kind, Kind::SubInfo { task, .. } if task == "a1")),
        "{kinds:?}"
    );
    assert!(
        matches!(kinds.last(), Some(Kind::SubEnd { state, .. }) if state == "lost"),
        "{kinds:?}"
    );
    assert!(!host.subagents.any());

    let _ = std::fs::remove_dir_all(store::thread_dir(&thread.id));

    drop(harness_rx);
    drop(env_lock);
}
