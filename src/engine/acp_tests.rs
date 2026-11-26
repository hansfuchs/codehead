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
    let (mut state, written, rx) = session_state(true, "x");

    state.busy = true;
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

/// A session up on MODEL (NEW, else loaded): the state, the lines it writes, its outputs.
fn session_state(
    new: bool,
    model: &str,
) -> (
    State,
    std::sync::mpsc::Receiver<String>,
    std::sync::mpsc::Receiver<(String, Out)>,
) {
    let (tx, rx) = std::sync::mpsc::channel();
    let (stdin, lines) = std::sync::mpsc::channel::<String>();
    let setup = Setup {
        cwd: "/".into(),
        resume: None,
        model: None,
        mcp: (String::new(), Vec::new(), Vec::new()),
        preamble: String::new(),
        harness: "opencode".into(),
        servers: Vec::new(),
    };
    let mut state = state(stdin, setup, "ada".into(), tx);

    state.session = Some("s".into());
    state.usage.session_started(new, model);

    (state, lines, rx)
}

/// 1 prompt on STATE: the usage updates UPDATES (used, size, $ of the session) while it runs,
/// then its end with the turn's own `totalTokens` (input + output) → (cost, ctx, window).
fn prompt_turn(
    state: &mut State,
    rx: &std::sync::mpsc::Receiver<(String, Out)>,
    updates: &[(u64, u64, Option<f64>)],
) -> (f64, u64, u64) {
    state.queue.push_back("go".into());
    state.pump().unwrap();

    for (used, size, amount) in updates {
        let mut update = json!({"sessionUpdate": "usage_update", "used": used, "size": size});

        if let Some(amount) = amount {
            update["cost"] = json!({"amount": amount, "currency": "USD"});
        }

        state.update(&update);
    }

    let id = state.next;

    state.incoming(&json!({"jsonrpc": "2.0", "id": id, "result": {"stopReason": "end_turn", "usage": {"totalTokens": 1_100_000, "inputTokens": 1_000_000, "outputTokens": 100_000}}})).unwrap();

    rx.try_iter()
        .find_map(|(_, out)| match out {
            Out::Event(Kind::TurnEnd { cost, ctx, window, .. }) => Some((cost, ctx, window)),
            _ => None,
        })
        .unwrap()
}

#[test]
fn usage_updates_give_the_turn_its_context_window_and_reported_cost() {
    let (mut state, _lines, rx) = session_state(true, "big-pickle");

    // ctx: the last `used`, never `totalTokens`; $: the rise of the session's $
    assert_eq!(
        prompt_turn(
            &mut state,
            &rx,
            &[(12_000, 200_000, Some(0.25)), (15_000, 200_000, Some(0.75))]
        ),
        (0.75, 15_000, 200_000)
    );
    assert_eq!(
        prompt_turn(&mut state, &rx, &[(18_000, 0, Some(1.0))]),
        (0.25, 18_000, 200_000)
    );

    // 0 $ reported, 0 price: 0 $
    assert_eq!(prompt_turn(&mut state, &rx, &[]), (0.0, 18_000, 200_000));
}

#[test]
fn agents_without_a_reported_cost_get_the_configured_price() {
    let (mut state, _lines, rx) = session_state(false, "openrouter/gpt-6.1-sol");

    state.usage.prices.insert(
        "gpt-6.1-sol".into(),
        crate::infra::config::Price {
            input: 1.0,
            cached: 0.1,
            output: 5.0,
            cache_write: 1.0,
        },
    );

    // a loaded session: its 1st $ holds the turns before → the price (1M × 1 + 100k × 5)
    let (loaded, _, _) = prompt_turn(&mut state, &rx, &[(9_000, 100_000, Some(7.0))]);

    assert!((loaded - 1.5).abs() < 1e-9, "{loaded}");

    // then the rise of the reported $
    let (reported, _, _) = prompt_turn(&mut state, &rx, &[(9_500, 100_000, Some(7.5))]);

    assert!((reported - 0.5).abs() < 1e-9, "{reported}");
}
