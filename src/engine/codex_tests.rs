use super::*;

fn test_state() -> (
    State,
    std::sync::mpsc::Receiver<String>,
    std::sync::mpsc::Receiver<(String, Out)>,
) {
    let (tx, rx) = std::sync::mpsc::channel();
    let (stdin, lines) = std::sync::mpsc::channel::<String>();
    let setup = Setup {
        cwd: "/w".into(),
        resume: None,
        fork: false,
        model: Some("gpt-6.1-sol".into()),
        effort: Some("high".into()),
        instructions: "You are ada.".into(),
        config: json!({}),
        sandbox: "danger-full-access",
    };

    (state(stdin, setup, "ada".into(), tx), lines, rx)
}

fn sent(lines: &std::sync::mpsc::Receiver<String>) -> Vec<Value> {
    lines
        .try_iter()
        .map(|line| serde_json::from_str(&line).unwrap())
        .collect()
}

fn events(rx: &std::sync::mpsc::Receiver<(String, Out)>) -> Vec<Out> {
    rx.try_iter().map(|(_, out)| out).collect()
}

#[test]
fn setup_starts_a_thread_with_model_instructions_and_mcp_then_runs_the_queue() {
    let (mut state, lines, rx) = test_state();

    state.queue.push_back("hi".into());
    state.request("initialize", &json!({}), Pending::Init).unwrap();
    state.incoming(&json!({"id": 1, "result": {"userAgent": "x"}})).unwrap();

    let setup_requests = sent(&lines);

    assert_eq!(setup_requests[1]["method"], "initialized");
    assert_eq!(setup_requests[2]["method"], "thread/start");
    assert_eq!(setup_requests[2]["params"]["model"], "gpt-6.1-sol");
    assert_eq!(setup_requests[2]["params"]["developerInstructions"], "You are ada.");
    assert_eq!(setup_requests[2]["params"]["approvalPolicy"], "untrusted");

    state
        .incoming(&json!({"id": 2, "result": {"model": "gpt-6.1-sol", "thread": {"id": "t1"}}}))
        .unwrap();

    assert!(
        matches!(&events(&rx)[0], Out::Event(Kind::Session { session, model }) if session == "t1" && model == "gpt-6.1-sol")
    );

    let turn_requests = sent(&lines);

    assert_eq!(turn_requests[0]["method"], "turn/start");
    assert_eq!(turn_requests[0]["params"]["threadId"], "t1");
    assert_eq!(turn_requests[0]["params"]["input"][0]["text"], "hi");
    assert_eq!(turn_requests[0]["params"]["effort"], "high");

    // a 2nd prompt waits for the turn end
    state.queue.push_back("next".into());
    state.pump().unwrap();

    assert_eq!(sent(&lines), [] as [serde_json::Value; 0]);
}

#[test]
fn a_lost_thread_on_resume_starts_a_new_one() {
    let (mut state, lines, rx) = test_state();

    state.setup.resume = Some("old".into());
    state.request("initialize", &json!({}), Pending::Init).unwrap();
    state.incoming(&json!({"id": 1, "result": {}})).unwrap();

    let requests = sent(&lines);

    assert_eq!(
        (
            requests[2]["method"].as_str(),
            requests[2]["params"]["threadId"].as_str()
        ),
        (Some("thread/resume"), Some("old"))
    );

    state
        .incoming(&json!({"id": 2, "error": {"code": -32600, "message": "thread not found"}}))
        .unwrap();

    assert_eq!(sent(&lines)[0]["method"], "thread/start");

    drop(rx);
}

#[test]
fn a_turn_maps_text_thoughts_tools_usage_and_its_end() {
    let (mut state, lines, rx) = test_state();

    state.thread = Some("t1".into());
    state.queue.push_back("go".into());
    state.total = TurnUse {
        input: 100,
        output: 10,
        cache_read: 50,
        cache_write: 0,
        ctx: 0,
    };
    state.pump().unwrap();
    sent(&lines);

    let notification = |method: &str, params: Value| json!({"method": method, "params": params});

    state
        .incoming(&notification("turn/started", json!({"turn": {"id": "u1"}})))
        .unwrap();
    state
        .incoming(&notification(
            "item/completed",
            json!({"item": {"type": "reasoning", "id": "r", "summary": ["Cause found."], "content": []}}),
        ))
        .unwrap();
    state.incoming(&notification("item/started", json!({"item": {"type": "commandExecution", "id": "c1", "command": "cargo test", "cwd": "/w", "status": "inProgress"}}))).unwrap();
    state.incoming(&notification("item/completed", json!({"item": {"type": "commandExecution", "id": "c1", "command": "cargo test", "status": "completed", "exitCode": 1, "aggregatedOutput": "1 failed"}}))).unwrap();
    state
        .incoming(&notification("item/agentMessage/delta", json!({"delta": "Do"})))
        .unwrap();
    state
        .incoming(&notification(
            "item/completed",
            json!({"item": {"type": "agentMessage", "id": "m", "text": "Done."}}),
        ))
        .unwrap();
    let tokens = json!({"total": {"inputTokens": 1100, "cachedInputTokens": 850, "outputTokens": 30, "totalTokens": 1130}, "last": {"inputTokens": 900, "cachedInputTokens": 800, "outputTokens": 20, "totalTokens": 920}, "modelContextWindow": 272_000});

    state
        .incoming(&notification(
            "thread/tokenUsage/updated",
            json!({"tokenUsage": tokens}),
        ))
        .unwrap();
    state
        .incoming(&notification(
            "turn/completed",
            json!({"turn": {"id": "u1", "status": "completed", "error": null, "durationMs": 4530}}),
        ))
        .unwrap();

    let outs = events(&rx);

    assert!(matches!(&outs[0], Out::Event(Kind::Thought { text }) if text == "Cause found."));
    assert!(
        matches!(&outs[1], Out::Event(Kind::Tool { id, name, input }) if id == "c1" && name == "Bash" && input["command"] == "cargo test")
    );
    assert!(matches!(&outs[2], Out::Event(Kind::ToolResult { ok: false, output, .. }) if output == "1 failed"));
    assert!(matches!(&outs[3], Out::Delta(text) if text == "Do"));

    assert_text_usage_turn_end_and_a_failed_turn(state, &rx, notification, &outs);
}

fn assert_text_usage_turn_end_and_a_failed_turn(
    mut state: State,
    rx: &std::sync::mpsc::Receiver<(String, Out)>,
    notification: impl Fn(&str, Value) -> Value,
    outs: &[Out],
) {
    assert!(matches!(&outs[4], Out::Event(Kind::Text { text }) if text == "Done."));
    assert!(
        matches!(&outs[5], Out::Usage(usage) if usage.input == 150 && usage.cache_read == 800 && usage.output == 20 && usage.ctx == 920)
    );

    let Out::Event(Kind::TurnEnd {
        input,
        output,
        cache_read,
        ctx,
        window,
        ms,
        error,
        ..
    }) = &outs[6]
    else {
        panic!("{:?}", outs[6])
    };

    assert_eq!(
        (*input, *output, *cache_read, *ctx, *window, *ms, error.clone()),
        (150, 20, 800, 920, 272_000, 4530, None)
    );
    assert!(!state.busy && state.turn.is_none());

    // a failed turn: its error
    state.queue.push_back("again".into());
    state.pump().unwrap();
    state
        .incoming(&notification(
            "turn/completed",
            json!({"turn": {"id": "u2", "status": "failed", "error": {"message": "The 'x' model is not supported"}}}),
        ))
        .unwrap();

    assert!(events(rx).iter().any(
        |out| matches!(out, Out::Event(Kind::TurnEnd { error: Some(error), .. }) if error.contains("not supported"))
    ));
}

#[test]
fn asks_go_to_the_host_and_answers_go_back_by_kind() {
    let (mut state, lines, rx) = test_state();

    state.thread = Some("t1".into());
    let item = json!({"method": "item/started", "params": {"item": {"type": "fileChange", "id": "f1", "status": "inProgress", "changes": [{"path": "/w/a.rs", "kind": {"type": "update"}, "diff": "+x"}]}}});

    state.incoming(&item).unwrap();
    events(&rx);

    state.incoming(&json!({"id": 7, "method": "item/fileChange/requestApproval", "params": {"itemId": "f1", "threadId": "t1", "turnId": "u", "startedAtMs": 1}})).unwrap();

    let Out::Control(id, req) = events(&rx).remove(0) else {
        panic!()
    };

    assert_eq!(req["tool_name"], "Edit");
    assert_eq!(req["input"]["_kind"], "edit");
    assert_eq!(req["input"]["file_path"], "/w/a.rs");

    state.answer(&id, true, &json!({})).unwrap();

    assert_eq!(sent(&lines)[0], json!({"id": 7, "result": {"decision": "accept"}}));

    // a command, declined
    state.incoming(&json!({"id": 8, "method": "item/commandExecution/requestApproval", "params": {"itemId": "c", "command": "rm -rf ~/x", "threadId": "t1", "turnId": "u", "startedAtMs": 1}})).unwrap();

    let Out::Control(id, req) = events(&rx).remove(0) else {
        panic!()
    };

    assert_eq!(
        (req["tool_name"].as_str(), req["input"]["command"].as_str()),
        (Some("Bash"), Some("rm -rf ~/x"))
    );

    state.answer(&id, false, &Value::Null).unwrap();

    assert_eq!(sent(&lines)[0]["result"]["decision"], "decline");

    // a question: answers by question text → by question id
    let question = json!({"id": 9, "method": "item/tool/requestUserInput", "params": {"isBlocking": true, "itemId": "q", "threadId": "t1", "turnId": "u", "questions": [{"id": "q1", "header": "Branch", "question": "Which branch?", "options": [{"label": "main", "description": "x"}]}]}});

    state.incoming(&question).unwrap();

    let Out::Control(id, req) = events(&rx).remove(0) else {
        panic!()
    };

    assert_eq!(req["tool_name"], "AskUserQuestion");
    assert_eq!(req["input"]["questions"][0]["options"][0]["label"], "main");

    state
        .answer(&id, true, &json!({"answers": {"Which branch?": "main"}}))
        .unwrap();

    assert_eq!(
        sent(&lines)[0]["result"],
        json!({"answers": {"q1": {"answers": ["main"]}}})
    );

    // an unknown request: an error reply
    state
        .incoming(&json!({"id": 10, "method": "attestation/generate", "params": {}}))
        .unwrap();

    assert_eq!(sent(&lines)[0]["error"]["code"], -32601);
}

#[test]
fn mcp_config_holds_codehead_and_the_agent_servers() {
    let acp = vec![
        json!({"name": "linear", "command": "npx", "args": ["x"], "env": [{"name": "K", "value": "v"}]}),
        json!({"type": "http", "name": "docs", "url": "https://d", "headers": [{"name": "Authorization", "value": "b"}]}),
    ];
    let config = mcp_config("/bin/codehead", &[("CODEHEAD_AGENT".into(), "ada".into())], &acp);

    assert_eq!(
        config["codehead"],
        json!({"command": "/bin/codehead", "args": ["mcp"], "env": {"CODEHEAD_AGENT": "ada"}})
    );
    assert_eq!(config["linear"]["env"]["K"], "v");
    assert_eq!(
        config["docs"],
        json!({"url": "https://d", "http_headers": {"Authorization": "b"}})
    );
}

/// Live: `codex app-server` runs the agent's model. `cargo test codex_live -- --ignored`
#[test]
#[ignore = "requires a live Codex harness"]
fn codex_live_turn_on_the_current_model() {
    use crate::engine::harness::Driver;

    let dir = std::env::temp_dir().join(format!("codehead-codex-live-{}", std::process::id()));

    std::fs::create_dir_all(&dir).unwrap();
    let mut child = std::process::Command::new("codex")
        .arg("app-server")
        .current_dir(&dir)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let tail = crate::engine::harness::stderr_tail(child.stderr.take().unwrap());
    let (tx, rx) = std::sync::mpsc::channel();
    let setup = Setup {
        cwd: dir.display().to_string(),
        resume: None,
        fork: false,
        model: std::env::var("CODEX_MODEL").ok(),
        effort: None,
        instructions: String::new(),
        config: json!({}),
        sandbox: "read-only",
    };
    let mut conn = start(
        child.stdin.take().unwrap(),
        child.stdout.take().unwrap(),
        tail,
        setup,
        "ada".into(),
        tx,
    );

    conn.prompt("reply with the word pong only").unwrap();
    let mut text = String::new();

    let end = loop {
        match rx.recv_timeout(std::time::Duration::from_secs(90)).unwrap().1 {
            Out::Event(Kind::Text { text: chunk }) => text.push_str(&chunk),
            Out::Event(Kind::TurnEnd { error, .. }) => break error,
            Out::Exit(reason) => panic!("exit: {reason}"),
            _ => {}
        }
    };

    let _ = child.kill();
    let _ = child.wait();

    assert_eq!(end, None);
    assert!(text.to_lowercase().contains("pong"), "{text}");
}

/// Live: a command asks (policy `untrusted`), the allow runs it, its result comes back.
/// `cargo test codex_live -- --ignored`
#[test]
#[ignore = "requires a live Codex harness"]
fn codex_live_command_asks_and_runs_on_allow() {
    use crate::engine::harness::Driver;

    let dir = std::env::temp_dir().join(format!("codehead-codex-ask-{}", std::process::id()));

    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("marker.txt"), "x").unwrap();
    let mut child = std::process::Command::new("codex")
        .arg("app-server")
        .current_dir(&dir)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let tail = crate::engine::harness::stderr_tail(child.stderr.take().unwrap());
    let (tx, rx) = std::sync::mpsc::channel();
    let setup = Setup {
        cwd: dir.display().to_string(),
        resume: None,
        fork: false,
        model: std::env::var("CODEX_MODEL").ok(),
        effort: None,
        instructions: String::new(),
        config: json!({}),
        sandbox: "danger-full-access",
    };
    let mut conn = start(
        child.stdin.take().unwrap(),
        child.stdout.take().unwrap(),
        tail,
        setup,
        "ada".into(),
        tx,
    );

    conn.prompt("Run the shell command `touch made-by-codex.txt` in the current directory, then reply with done.")
        .unwrap();
    let (mut asked, mut ran) = (false, false);

    let end = loop {
        match rx.recv_timeout(std::time::Duration::from_secs(120)).unwrap().1 {
            Out::Control(id, req) => {
                asked = true;

                assert!(req["tool_name"].is_string(), "{req}");

                conn.allow(&id, &json!({})).unwrap();
            }
            Out::Event(Kind::ToolResult { .. }) => ran = true,
            Out::Event(Kind::TurnEnd { error, .. }) => break error,
            Out::Exit(reason) => panic!("exit: {reason}"),
            _ => {}
        }
    };

    let _ = child.kill();
    let _ = child.wait();

    assert_eq!(end, None);
    assert!(asked && ran, "asked {asked}, ran {ran}");
    assert!(dir.join("made-by-codex.txt").exists());
}
