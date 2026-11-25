use super::*;
use serde_json::json;
use std::io::Write;

fn tool(id: &str, input: Value) -> Kind {
    Kind::Tool {
        id: id.into(),
        name: "Agent".into(),
        input,
    }
}

#[test]
fn foreground_start_and_end_by_the_tool_result() {
    let (mut thread, lost) = Tracker::from_log(&[]);

    assert_eq!(lost, [] as [(std::string::String, crate::engine::store::Kind); 0]);

    let logs = thread.event(
        "ada",
        &tool(
            "t1",
            json!({"description": "find the bug", "prompt": "look at x", "subagent_type": "Explore"}),
        ),
    );

    assert!(
        matches!(&logs[0].1, Kind::SubStart { id, description, agent_type, background: false, prompt, .. } if id == "t1" && description == "find the bug" && agent_type == "Explore" && prompt == "look at x")
    );
    assert_eq!(thread.masters(), HashSet::from(["ada".to_string()]));

    // another agent's result with the same id: 0 end
    assert_eq!(
        thread.event(
            "bob",
            &Kind::ToolResult {
                id: "t1".into(),
                ok: true,
                output: "x".into()
            }
        ),
        [] as [(std::string::String, crate::engine::store::Kind); 0]
    );

    let logs = thread.event(
        "ada",
        &Kind::ToolResult {
            id: "t1".into(),
            ok: true,
            output: "found it".into(),
        },
    );

    assert!(matches!(&logs[0].1, Kind::SubEnd { state, result, .. } if state == "done" && result == "found it"));
    assert!(!thread.any());

    // the same call again (a replay): 0 start
    assert_eq!(
        thread.event("ada", &tool("t1", json!({}))),
        [] as [(std::string::String, crate::engine::store::Kind); 0]
    );
}

#[test]
fn background_launch_gives_the_task_id_then_the_meta_file_ends_it() {
    let root = std::env::temp_dir().join(format!("codehead-subs-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let dir = root.join("proj/s1/subagents");

    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(root.join("proj/s1.jsonl"), "").unwrap();
    let (mut thread, _) = Tracker::from_log(&[]);

    thread.since = 0;
    thread.event(
        "ada",
        &tool("t2", json!({"description": "long job", "run_in_background": true})),
    );
    let logs = thread.event(
        "ada",
        &Kind::ToolResult {
            id: "t2".into(),
            ok: true,
            output: "Async agent launched successfully.\nagentId: abc123 (internal)".into(),
        },
    );

    assert!(matches!(&logs[0].1, Kind::SubInfo { task, .. } if task == "abc123"));
    assert!(thread.any(), "a background launch: still running");

    // its files: running, with a nested subagent of its own
    let line = |value: Value| value.to_string() + "\n";

    std::fs::write(
        dir.join("agent-abc123.meta.json"),
        json!({"toolUseId": "t2", "description": "long job", "requestShape": "background"}).to_string(),
    )
    .unwrap();
    std::fs::write(dir.join("agent-abc123.jsonl"), line(json!({"type": "user", "message": {"content": "do the job"}})) + &line(json!({"type": "assistant", "message": {"content": [{"type": "tool_use", "id": "t3", "name": "Agent", "input": {"description": "helper"}}]}}))).unwrap();
    std::fs::write(
        dir.join("agent-def456.meta.json"),
        json!({"toolUseId": "t3", "description": "helper", "agentType": "Explore"}).to_string(),
    )
    .unwrap();
    std::fs::write(dir.join("agent-def456.jsonl"), line(json!({"type": "user", "message": {"content": "help"}})) + &line(json!({"type": "assistant", "message": {"content": [{"type": "tool_use", "id": "x", "name": "Bash", "input": {"command": "cargo test"}}]}}))).unwrap();
    let sessions = [("ada".to_string(), "s1".to_string())];
    let logs = thread.scan(&root, &sessions);

    assert!(
        logs.iter()
            .any(|(_, key)| matches!(key, Kind::SubInfo { id, task, .. } if id == "t2" && task == "abc123")),
        "{logs:?}"
    );
    assert!(logs.iter().any(|(_, key_value)| matches!(key_value, Kind::SubStart { id, parent: Some(parent), prompt, .. } if id == "t3" && parent == "t2" && prompt == "help")), "{logs:?}");

    verify_background_launch_gives_the_task_id_then_the_meta_file_ends_it(&root, &dir, thread, line, &sessions);
}

fn verify_background_launch_gives_the_task_id_then_the_meta_file_ends_it(
    root: &PathBuf,
    dir: &Path,
    mut thread: Tracker,
    line: impl Fn(Value) -> String,
    sessions: &[(String, String); 1],
) {
    assert_eq!(
        activity_of(&dir.join("agent-def456.jsonl")).as_deref(),
        Some("Bash cargo test")
    );
    assert!(thread.scan(root, sessions).is_empty(), "0 news: 0 events");

    // both finish: the report of each
    std::fs::write(
        dir.join("agent-def456.meta.json"),
        json!({"toolUseId": "t3", "finished": true}).to_string(),
    )
    .unwrap();
    std::fs::write(
        dir.join("agent-abc123.meta.json"),
        json!({"toolUseId": "t2", "finished": true}).to_string(),
    )
    .unwrap();
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(dir.join("agent-abc123.jsonl"))
        .unwrap();

    write!(file, "{}", line(json!({"type": "assistant", "message": {"content": [{"type": "tool_use", "name": "SubagentHandback", "input": {"message": "all done"}}]}}))).unwrap();
    let logs = thread.scan(root, sessions);

    assert!(
        logs.iter()
            .any(|(_, key_2)| matches!(key_2, Kind::SubEnd { id, result, .. } if id == "t2" && result == "all done")),
        "{logs:?}"
    );
    assert!(
        logs.iter()
            .any(|(_, key_3)| matches!(key_3, Kind::SubEnd { id, .. } if id == "t3")),
        "{logs:?}"
    );
    assert!(!thread.any());

    let _ = std::fs::remove_dir_all(root);
}

/// Claude Code's default: a background subagent without `run_in_background` in the input.
/// Its launch result keeps it running (its master stays busy); its meta file ends it.
#[test]
fn a_launch_result_without_run_in_background_keeps_it_running() {
    let (mut thread, _) = Tracker::from_log(&[]);

    thread.event("ada", &tool("t1", json!({"description": "map keys", "prompt": "read"})));
    let out = "Async agent launched successfully. (This tool result is internal metadata.)\nagentId: abb626af5fcd5a2af (internal ID)\nThe agent is working in the background.";
    let logs = thread.event(
        "ada",
        &Kind::ToolResult {
            id: "t1".into(),
            ok: true,
            output: out.into(),
        },
    );

    assert!(
        matches!(&logs[0].1, Kind::SubInfo { task, .. } if task == "abb626af5fcd5a2af"),
        "{logs:?}"
    );
    assert!(
        thread.any() && thread.masters().contains("ada"),
        "still running: the master is busy"
    );
    assert!(thread.running["t1"].background);

    // a report that names an agent id (0 launch): the end
    thread.event("ada", &tool("t2", json!({"description": "x"})));
    let logs = thread.event(
        "ada",
        &Kind::ToolResult {
            id: "t2".into(),
            ok: true,
            output: "done: see agentId: x1 notes".into(),
        },
    );

    assert!(matches!(&logs[0].1, Kind::SubEnd { state, .. } if state == "done"));
}

#[test]
fn a_dead_process_or_a_restarted_host_loses_its_subagents() {
    let (mut thread, _) = Tracker::from_log(&[]);

    thread.event("ada", &tool("t1", json!({"description": "a"})));
    thread.event("bob", &tool("t2", json!({"description": "b"})));
    let logs = thread.exit("ada", "the harness process ended");

    assert_eq!(logs.len(), 1);
    assert!(matches!(&logs[0].1, Kind::SubEnd { id, state, .. } if id == "t1" && state == "lost"));
    assert_eq!(thread.masters(), HashSet::from(["bob".to_string()]));

    // a new host: the log's open subagents are lost
    let ev = |agent: &str, kind: Kind| Event {
        ts: 1,
        agent: Some(agent.into()),
        kind,
    };
    let log = [
        ev(
            "ada",
            Kind::SubStart {
                id: "a".into(),
                parent: None,
                description: "x".into(),
                agent_type: String::new(),
                background: true,
                prompt: String::new(),
            },
        ),
        ev(
            "ada",
            Kind::SubStart {
                id: "b".into(),
                parent: None,
                description: "y".into(),
                agent_type: String::new(),
                background: false,
                prompt: String::new(),
            },
        ),
        ev(
            "ada",
            Kind::SubEnd {
                id: "b".into(),
                state: "done".into(),
                result: String::new(),
            },
        ),
    ];
    let (thread_value, logs) = Tracker::from_log(&log);

    assert_eq!(logs.len(), 1);
    assert!(matches!(&logs[0].1, Kind::SubEnd { id, state, .. } if id == "a" && state == "lost"));
    assert!(!thread_value.any());
}

/// A background subagent ended; its master's `SendMessage` to its agent id resumes it. A
/// `finished` meta file from before the resume does not end it; a new one does.
#[test]
fn a_message_to_an_ended_subagent_resumes_it_until_its_meta_file_ends_it_again() {
    let root = std::env::temp_dir().join(format!("codehead-subs-resume-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let dir = root.join("proj/s1/subagents");

    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(root.join("proj/s1.jsonl"), "").unwrap();
    let sessions = [("ada".to_string(), "s1".to_string())];
    let (mut thread_value, _) = Tracker::from_log(&[]);

    thread_value.since = 0;
    thread_value.event("ada", &tool("t1", json!({"description": "stream C"})));
    thread_value.event(
        "ada",
        &Kind::ToolResult {
            id: "t1".into(),
            ok: true,
            output: "Async agent launched successfully.\nagentId: c1 (internal)".into(),
        },
    );
    std::fs::write(
        dir.join("agent-c1.meta.json"),
        json!({"toolUseId": "t1", "finished": true}).to_string(),
    )
    .unwrap();
    std::fs::write(dir.join("agent-c1.jsonl"), "").unwrap();

    assert!(
        thread_value
            .scan(&root, &sessions)
            .iter()
            .any(|(_, key)| matches!(key, Kind::SubEnd { id, .. } if id == "t1"))
    );
    assert!(!thread_value.any() && thread_value.resumable());

    // a message to another id or of another agent: 0 resume
    let send = |to: &str| Kind::Tool {
        id: format!("m-{to}"),
        name: "SendMessage".into(),
        input: json!({"to": to, "message": "go on"}),
    };

    assert_eq!(
        thread_value.event("ada", &send("x9")),
        [] as [(std::string::String, crate::engine::store::Kind); 0]
    );

    verify_a_message_to_an_ended_subagent_resumes_it_until_its_meta_file_ends_it_again(
        &root,
        &dir,
        &sessions,
        thread_value,
        send,
    );
}

fn verify_a_message_to_an_ended_subagent_resumes_it_until_its_meta_file_ends_it_again(
    root: &PathBuf,
    dir: &Path,
    sessions: &[(String, String); 1],
    mut thread_value: Tracker,
    send: impl Fn(&str) -> Kind,
) {
    assert_eq!(
        thread_value.event("bob", &send("c1")),
        [] as [(std::string::String, crate::engine::store::Kind); 0]
    );

    let logs = thread_value.event("ada", &send("c1"));

    assert!(
        matches!(&logs[..], [(agent, Kind::SubResume { id })] if agent == "ada" && id == "t1"),
        "{logs:?}"
    );
    assert_eq!(thread_value.masters(), HashSet::from(["ada".to_string()]));

    // the old `finished` meta file (from before the resume): still running
    assert_eq!(
        thread_value.scan(root, sessions),
        [] as [(std::string::String, crate::engine::store::Kind); 0]
    );
    assert!(thread_value.any());

    // a 2nd message while it runs: 0 2nd resume
    assert_eq!(
        thread_value.event("ada", &send("c1")),
        [] as [(std::string::String, crate::engine::store::Kind); 0]
    );

    // its new end
    std::thread::sleep(std::time::Duration::from_millis(20));
    std::fs::write(
        dir.join("agent-c1.meta.json"),
        json!({"toolUseId": "t1", "finished": true}).to_string(),
    )
    .unwrap();
    let logs = thread_value.scan(root, sessions);

    assert!(
        logs.iter()
            .any(|(_, key_value)| matches!(key_value, Kind::SubEnd { id, state, .. } if id == "t1" && state == "done")),
        "{logs:?}"
    );
    assert!(!thread_value.any());

    let _ = std::fs::remove_dir_all(root);
}

/// A resume the events missed: the meta file of an ended subagent changed after its end, 0
/// `finished`, its transcript grew: the scan resumes it.
#[test]
fn a_scan_resumes_an_ended_subagent_whose_files_changed_after_its_end() {
    let root = std::env::temp_dir().join(format!("codehead-subs-rescan-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let dir = root.join("proj/s1/subagents");

    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(root.join("proj/s1.jsonl"), "").unwrap();
    std::fs::write(dir.join("agent-d1.meta.json"), json!({"toolUseId": "t1"}).to_string()).unwrap();
    std::fs::write(dir.join("agent-d1.jsonl"), "").unwrap();
    let sessions = [("ada".to_string(), "s1".to_string())];
    let ev = |ts: u64, kind: Kind| Event {
        ts,
        agent: Some("ada".into()),
        kind,
    };
    let now = crate::engine::now_ms();
    let transcript = dir.join("agent-d1.jsonl").display().to_string();

    // the log: it ended a minute ago (its files: changed since, 0 `finished`)
    let log = ended_subagent_events(now, transcript, ev);

    let (mut thread, lost) = Tracker::from_log(&log);

    assert!(lost.is_empty() && thread.resumable());

    let logs = thread.scan(&root, &sessions);

    assert!(
        matches!(&logs[..], [(_, Kind::SubResume { id })] if id == "t1"),
        "{logs:?}"
    );
    assert!(thread.any());

    // a log with the resume, the host restarts: lost, resumable again
    let mut log = log.to_vec();

    log.push(ev(now - 30_000, Kind::SubResume { id: "t1".into() }));
    let (thread_value, lost) = Tracker::from_log(&log);

    assert!(
        matches!(&lost[..], [(_, Kind::SubEnd { id, state, .. })] if id == "t1" && state == "lost"),
        "{lost:?}"
    );
    assert!(!thread_value.any() && thread_value.resumable());

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn task_tools_of_the_harnesses() {
    for count in ["Agent", "Task", "task", "spawn_agent"] {
        assert!(is_sub_tool(count), "{count}");
    }

    for count_value in ["Bash", "TaskStop", "TodoWrite", "agents"] {
        assert!(!is_sub_tool(count_value), "{count_value}");
    }
}

fn ended_subagent_events(now: u64, transcript: String, event: impl Fn(u64, Kind) -> Event) -> [Event; 3] {
    [
        event(
            now - 120_000,
            Kind::SubStart {
                id: "t1".into(),
                parent: None,
                description: "stream D".into(),
                agent_type: String::new(),
                background: true,
                prompt: String::new(),
            },
        ),
        event(
            now - 120_000,
            Kind::SubInfo {
                id: "t1".into(),
                transcript,
                task: "d1".into(),
            },
        ),
        event(
            now - 60_000,
            Kind::SubEnd {
                id: "t1".into(),
                state: "done".into(),
                result: "r".into(),
            },
        ),
    ]
}
