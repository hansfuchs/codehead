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
    assert_eq!(setup_requests[3]["method"], "thread/start");
    assert_eq!(setup_requests[3]["params"]["model"], "gpt-6.1-sol");
    assert_eq!(setup_requests[3]["params"]["developerInstructions"], "You are ada.");
    assert_eq!(setup_requests[3]["params"]["approvalPolicy"], "untrusted");

    state
        .incoming(&json!({"id": 3, "result": {"model": "gpt-6.1-sol", "thread": {"id": "t1"}}}))
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
            requests[3]["method"].as_str(),
            requests[3]["params"]["threadId"].as_str()
        ),
        (Some("thread/resume"), Some("old"))
    );

    state
        .incoming(&json!({"id": 3, "error": {"code": -32600, "message": "thread not found"}}))
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

fn limit_reports(rx: &std::sync::mpsc::Receiver<(String, Out)>) -> Vec<Vec<(String, f64, Option<u64>)>> {
    events(rx)
        .into_iter()
        .filter_map(|out| {
            if let Out::Limits(windows) = out {
                Some(windows)
            } else {
                None
            }
        })
        .collect()
}

#[test]
fn rate_limits_are_read_after_initialize_and_their_windows_go_to_the_host() {
    let (mut state, lines, rx) = test_state();

    state.request("initialize", &json!({}), Pending::Init).unwrap();
    state.incoming(&json!({"id": 1, "result": {}})).unwrap();
    let requests = sent(&lines);

    assert_eq!(
        (&requests[2]["method"], &requests[2]["id"]),
        (&json!("account/rateLimits/read"), &json!(2))
    );

    // codex-cli 0.170's reply on a business plan: the weekly window only
    let snapshot = json!({"limitId": "codex", "limitName": null, "primary": {"usedPercent": 19, "windowDurationMins": 10080, "resetsAt": 1_796_039_040_u64}, "secondary": null, "credits": {"hasCredits": true, "unlimited": false, "balance": null}, "planType": "self_serve_business_prolite"});
    let update = |snapshot: Value| json!({"method": "account/rateLimits/updated", "params": {"rateLimits": snapshot}});

    state
        .incoming(&json!({"id": 2, "result": {"rateLimits": snapshot, "rateLimitsByLimitId": {"codex": snapshot}}}))
        .unwrap();
    state.incoming(&update(json!({"limitId": "codex", "primary": {"usedPercent": 42.5, "windowDurationMins": 300, "resetsAt": 1_796_000_000_u64}, "secondary": {"usedPercent": 20, "windowDurationMins": 10080, "resetsAt": null}}))).unwrap();

    // another bucket, a sparse update with 0 windows: 0 reports
    state
        .incoming(&update(
            json!({"limitId": "other", "primary": {"usedPercent": 99, "windowDurationMins": 300}}),
        ))
        .unwrap();
    state
        .incoming(&update(json!({"limitId": null, "primary": null, "secondary": null})))
        .unwrap();

    assert_eq!(
        limit_reports(&rx),
        [
            vec![("seven_day".to_string(), 19.0, Some(1_796_039_040))],
            vec![
                ("five_hour".to_string(), 42.5, Some(1_796_000_000)),
                ("seven_day".to_string(), 20.0, None)
            ],
        ]
    );

    // an API key login: the read fails, 0 reports, the thread starts all the same
    let (mut keyed, keyed_lines, keyed_rx) = test_state();

    keyed.request("initialize", &json!({}), Pending::Init).unwrap();
    keyed.incoming(&json!({"id": 1, "result": {}})).unwrap();
    keyed
        .incoming(&json!({"id": 2, "error": {"code": -32600, "message": "not signed in with ChatGPT"}}))
        .unwrap();

    assert_eq!(limit_reports(&keyed_rx), [] as [Vec<(String, f64, Option<u64>)>; 0]);
    assert_eq!(sent(&keyed_lines)[3]["method"], "thread/start");
}

#[test]
fn a_turn_costs_the_configured_price_of_its_model() {
    let (mut state, lines, rx) = test_state();
    let price = crate::infra::config::Price {
        input: 1.25,
        cached: 0.125,
        output: 10.0,
        cache_write: 1.25,
    };
    let notification = |method: &str, params: Value| json!({"method": method, "params": params});
    let tokens = json!({"tokenUsage": {"total": {"inputTokens": 1_000_000, "cachedInputTokens": 600_000, "outputTokens": 100_000, "totalTokens": 1_100_000}, "last": {"inputTokens": 1_000_000, "cachedInputTokens": 600_000, "outputTokens": 100_000, "totalTokens": 1_100_000}}});

    let turn = |state: &mut State, id: &str| {
        state.queue.push_back("go".into());
        state.pump().unwrap();
        state
            .incoming(&notification("thread/tokenUsage/updated", tokens.clone()))
            .unwrap();
        state
            .incoming(&notification(
                "turn/completed",
                json!({"turn": {"id": id, "status": "completed"}}),
            ))
            .unwrap();
        events(&rx).into_iter().find_map(|out| match out {
            Out::Event(Kind::TurnEnd { cost, .. }) => Some(cost),
            _ => None,
        })
    };

    state.prices.insert("gpt-6.1-sol".into(), price);
    state.thread = Some("t1".into());
    let priced = turn(&mut state, "u1").unwrap();

    // 400k input × 1.25 + 600k cached × 0.125 + 100k output × 10 (per 1M)
    assert!((priced - 1.575).abs() < 1e-9, "{priced}");

    // the thread runs another model (`thread/start` named it): 0 price, 0 $
    state.model = "gpt-6.1-mini".into();
    state.total = TurnUse::default();

    assert_eq!(turn(&mut state, "u2"), Some(0.0));

    drop(lines);
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
