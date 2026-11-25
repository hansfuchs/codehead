use super::*;

#[test]
fn system_prompt_asks_for_fenced_blocks_for_commands() {
    let agent = crate::engine::roster::Agent {
        name: "ada".into(),
        ..Default::default()
    };
    let thread = crate::engine::store::Thread::new("t", "task", std::path::Path::new("/tmp"), vec!["ada".into()]);
    let text = system_prompt(&agent, &thread, &[]);

    assert!(
        text.contains("a fenced code block with a language tag (```sh)") && text.contains("never inline code"),
        "{text}"
    );
}

#[test]
fn home_block_points_every_harness_to_agents_md() {
    let bytes = home_block(std::path::Path::new("/h/bonfire"));

    assert!(
        bytes.contains("# Home repo")
            && bytes.contains("`/h/bonfire/AGENTS.md`")
            && bytes.contains("$CODEHEAD_HOME_REPO"),
        "{bytes}"
    );
}

#[test]
fn json_lines_skip_a_non_utf8_line_and_go_on() {
    let mut bytes = b"{\"a\":1}\n".to_vec();

    bytes.extend_from_slice(b"{\"b\":\"\xff\xfe\"}\n");
    bytes.extend_from_slice(b"{\"c\":3}\n");
    let mut seen = Vec::new();

    json_lines(&bytes[..], |value| {
        seen.push(value);
        true
    });

    assert_eq!(seen, vec![json!({"a": 1}), json!({"c": 3})]);
}

#[test]
fn kill_ends_the_harness_children_too() {
    use std::os::unix::process::CommandExt;

    // a harness with a background child (a dev server, a sleep)
    let mut child = Command::new("sh")
        .args(["-c", "sleep 60 & echo $!; wait"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .process_group(0)
        .spawn()
        .unwrap();
    let mut line = String::new();

    BufReader::new(child.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    let grandchild: i32 = line.trim().parse().unwrap();
    let stdin = child.stdin.take().unwrap();
    let mut text = Session {
        child,
        driver: Box::new(ClaudeStream(stdin)),
    };

    text.kill();

    // SAFETY: signal 0 only checks that the pid exists.
    let alive = || unsafe { libc::kill(grandchild, 0) } == 0;
    let at = std::time::Instant::now();

    while alive() && at.elapsed() < std::time::Duration::from_secs(3) {
        std::thread::sleep(std::time::Duration::from_millis(50));
    }

    assert!(!alive(), "grandchild {grandchild} still runs");
}

#[test]
fn a_running_turn_reports_its_tokens_per_model_call() {
    let mut path = Parser::default();

    let use_of = |output: Vec<Out>| {
        output.into_iter().find_map(|output_value| {
            a_running_turn_reports_its_tokens_per_model_call_if_let_out_usage_user_output(&output_value)
        })
    };

    let msg = |id: &str, out: u64| json!({"type": "assistant", "parent_tool_use_id": null, "message": {"id": id, "usage": {"input_tokens": 2, "output_tokens": out, "cache_read_input_tokens": 100}, "content": []}});

    assert_eq!(
        use_of(path.line(&msg("m1", 5))),
        Some(TurnUse {
            input: 2,
            output: 5,
            cache_read: 100,
            cache_write: 0,
            ctx: 102
        })
    );

    // the next block of the same message: its usage again, 0 double count
    assert_eq!(
        use_of(path.line(&msg("m1", 9))).map(|user_value| user_value.output),
        Some(9)
    );
    assert_eq!(
        use_of(path.line(&msg("m2", 1))).map(|user_2| (user_2.input, user_2.output, user_2.cache_read)),
        Some((4, 10, 200))
    );

    // the turn end: a new turn starts at 0
    path.line(&json!({"type": "result", "subtype": "success", "usage": {}}));

    assert_eq!(use_of(path.line(&msg("m3", 1))).map(|user_3| user_3.output), Some(1));
}

fn a_running_turn_reports_its_tokens_per_model_call_if_let_out_usage_user_output(
    output_value: &Out,
) -> Option<TurnUse> {
    if let Out::Usage(user) = output_value {
        Some(*user)
    } else {
        None
    }
}

#[test]
fn parser_maps_stream_json() {
    let mut path = Parser::default();

    let ev = |output: Vec<Out>| -> Vec<Kind> {
        output
            .into_iter()
            .filter_map(parser_maps_stream_json_if_let_out_event_key_output)
            .collect()
    };

    let init = json!({"type": "system", "subtype": "init", "session_id": "s1", "model": "claude-opus-5-5"});

    assert_eq!(
        ev(path.line(&init)),
        vec![Kind::Session {
            session: "s1".into(),
            model: "claude-opus-5-5".into()
        }]
    );

    let agent = json!({"type": "assistant", "parent_tool_use_id": null, "message": {"usage": {"input_tokens": 3, "cache_read_input_tokens": 100}, "content": [
        {"type": "thinking", "thinking": ""}, {"type": "text", "text": "hi"}, {"type": "tool_use", "id": "t1", "name": "Bash", "input": {"command": "ls"}}]}});
    let key_value = ev(path.line(&agent));

    assert_eq!(key_value.len(), 2);
    assert_eq!(key_value[0], Kind::Text { text: "hi".into() });

    // a thinking block with a summary: 1 thought, trimmed
    let agent_value = json!({"type": "assistant", "parent_tool_use_id": null, "message": {"content": [
        {"type": "thinking", "thinking": "  ", "signature": "x"}, {"type": "thinking", "thinking": "CI=true missing, fixing it.\n\n", "signature": "y"}]}});

    assert_eq!(
        ev(path.line(&agent_value)),
        vec![Kind::Thought {
            text: "CI=true missing, fixing it.".into()
        }]
    );

    // token chunks: text deltas of the main agent only
    let chunk = |parent: Value, kind: &str| json!({"type": "stream_event", "parent_tool_use_id": parent, "event": {"type": "content_block_delta", "index": 1, "delta": {"type": kind, "text": "Hel", "thinking": "x"}}});

    assert!(matches!(path.line(&chunk(Value::Null, "text_delta")).as_slice(), [Out::Delta(text)] if text == "Hel"));

    verify_parser_maps_stream_json(path, ev, chunk);
}

fn verify_parser_maps_stream_json(
    mut path: Parser,
    ev: impl Fn(Vec<Out>) -> Vec<Kind>,
    chunk: impl Fn(Value, &str) -> Value,
) {
    assert!(path.line(&chunk(json!("t9"), "text_delta")).is_empty());
    assert!(path.line(&chunk(Value::Null, "thinking_delta")).is_empty());

    let sub = json!({"type": "assistant", "parent_tool_use_id": "t9", "message": {"content": [{"type": "text", "text": "sub agent"}]}});

    assert_eq!(ev(path.line(&sub)), [] as [crate::engine::store::Kind; 0]);

    let result = json!({"type": "user", "parent_tool_use_id": null, "message": {"content": [{"type": "tool_result", "tool_use_id": "t1", "content": [{"type": "text", "text": "a\nb"}]}]}});

    assert_eq!(
        ev(path.line(&result)),
        vec![Kind::ToolResult {
            id: "t1".into(),
            ok: true,
            output: "a\nb".into()
        }]
    );

    let end = |character: f64| json!({"type": "result", "subtype": "success", "total_cost_usd": character, "duration_ms": 5, "usage": {"input_tokens": 1, "output_tokens": 2}});

    let Kind::TurnEnd { cost, ctx, window, .. } = ev(path.line(&end(0.5))).remove(0) else {
        panic!()
    };

    assert_eq!((cost, ctx, window), (0.5, 103, 0));

    // the window: the session model's `modelUsage` entry, not a subagent's
    let mut error = end(0.5);

    error["modelUsage"] =
        json!({"claude-haiku-5-5": {"contextWindow": 200_000}, "claude-opus-5-5": {"contextWindow": 1_000_000}});

    let Kind::TurnEnd { window, .. } = ev(path.line(&error)).remove(0) else {
        panic!()
    };

    assert_eq!(window, 1_000_000);

    let Kind::TurnEnd { cost, .. } = ev(path.line(&end(0.75))).remove(0) else {
        panic!()
    };

    assert_eq!(cost, 0.25);

    let character_value =
        json!({"type": "control_request", "request_id": "r1", "request": {"subtype": "can_use_tool"}});

    assert!(matches!(&path.line(&character_value)[0], Out::Control(id, _) if id == "r1"));
}

fn parser_maps_stream_json_if_let_out_event_key_output(output_value: Out) -> Option<Kind> {
    if let Out::Event(key) = output_value {
        Some(key)
    } else {
        None
    }
}
