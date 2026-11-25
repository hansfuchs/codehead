use super::*;

#[test]
fn codex_acp_016_config_options_and_api_errors() {
    // codex-acp 0.16: the model as a config option
    let result = json!({"sessionId": "s", "configOptions": [{"id": "model", "category": "model", "type": "select", "currentValue": "gpt-5.6-sol", "options": []}]});

    assert_eq!(session_model(&result), ("gpt-5.6-sol".to_string(), true));

    // older: models.currentModelId
    assert_eq!(
        session_model(&json!({"models": {"currentModelId": "gpt-5.5"}})),
        ("gpt-5.5".to_string(), false)
    );

    // the API's 400 behind "Internal error"
    let error = json!({"code": -32603, "message": "Internal error", "data": {"message": "{\"type\":\"error\",\"status\":400,\"error\":{\"type\":\"invalid_request_error\",\"message\":\"The 'gpt-6.1-sol' model is not supported when using Codex with a ChatGPT account.\"}}"}});

    assert_eq!(
        error_text(&error),
        "Internal error: The 'gpt-6.1-sol' model is not supported when using Codex with a ChatGPT account."
    );
    assert_eq!(
        error_text(&json!({"code": -32601, "message": "Method not found", "data": "session/set_model"})),
        "Method not found: session/set_model"
    );
}
#[test]
fn tool_calls_map_to_familiar_names() {
    let (name, input) =
        tool_of(&json!({"kind": "execute", "title": "Run tests", "rawInput": {"command": "cargo test"}}));

    assert_eq!(name, "Bash");
    assert_eq!(input["command"], "cargo test");
    assert_eq!(input["description"], "Run tests");

    let (untyped_name, _) = tool_of(&json!({"kind": "other", "title": "codehead_send"}));

    assert_eq!(untyped_name, "codehead_send");
}

#[test]
fn thought_chunks_become_1_thought_before_the_next_text_or_tool() {
    let (tx, rx) = std::sync::mpsc::channel();
    let (stdin, written) = std::sync::mpsc::channel::<String>();
    let setup = Setup {
        cwd: "/".into(),
        resume: None,
        model: None,
        mcp: (String::new(), Vec::new(), Vec::new()),
        preamble: String::new(),
        harness: "x".into(),
        servers: Vec::new(),
    };
    let mut state = State {
        stdin,
        next: 0,
        pending: HashMap::new(),
        session: None,
        loading: false,
        capabilities: Capabilities::default(),
        busy: true,
        queue: VecDeque::new(),
        preamble: None,
        text: String::new(),
        thought: String::new(),
        asks: HashMap::new(),
        setup,
        tx,
        name: "ada".into(),
        dead: false,
    };
    let chunk = |kind: &str, text: &str| json!({"sessionUpdate": kind, "content": {"type": "text", "text": text}});

    // empty chunks: 0 events
    state.update(&chunk("agent_thought_chunk", ""));

    for piece in ["Cause", " found,", " fixing."] {
        state.update(&chunk("agent_thought_chunk", piece));
    }

    state.update(&json!({"sessionUpdate": "tool_call", "toolCallId": "t1", "kind": "execute", "title": "Run", "rawInput": {"command": "ls"}}));
    state.update(&chunk("agent_thought_chunk", "Then the text."));
    state.update(&chunk("agent_message_chunk", "Done."));
    state.flush_text();
    let kinds: Vec<Kind> = rx
        .try_iter()
        .filter_map(|(_, out)| if let Out::Event(kind) = out { Some(kind) } else { None })
        .collect();

    assert_eq!(
        kinds[0],
        Kind::Thought {
            text: "Cause found, fixing.".into()
        }
    );
    assert!(matches!(kinds[1], Kind::Tool { .. }));
    assert_eq!(
        kinds[2],
        Kind::Thought {
            text: "Then the text.".into()
        }
    );
    assert_eq!(kinds[3], Kind::Text { text: "Done.".into() });
    assert_eq!(kinds.len(), 4);

    drop(written);
}
