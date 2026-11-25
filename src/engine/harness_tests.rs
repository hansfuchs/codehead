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
    let block = home_block(std::path::Path::new("/h/bonfire"));

    assert!(
        block.contains("# Home repo")
            && block.contains("`/h/bonfire/AGENTS.md`")
            && block.contains("$CODEHEAD_HOME_REPO"),
        "{block}"
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
    let mut session = Session {
        child,
        driver: Box::new(ClaudeStream(stdin)),
    };

    session.kill();

    // SAFETY: signal 0 only checks that the pid exists.
    let alive = || unsafe { libc::kill(grandchild, 0) } == 0;
    let started = std::time::Instant::now();

    while alive() && started.elapsed() < std::time::Duration::from_secs(3) {
        std::thread::sleep(std::time::Duration::from_millis(50));
    }

    assert!(!alive(), "grandchild {grandchild} still runs");
}

#[test]
fn a_running_turn_reports_its_tokens_per_model_call() {
    let mut parser = Parser::default();

    let use_of = |output: Vec<Out>| output.into_iter().find_map(|output| usage_of(&output));

    let message = |id: &str, out: u64| json!({"type": "assistant", "parent_tool_use_id": null, "message": {"id": id, "usage": {"input_tokens": 2, "output_tokens": out, "cache_read_input_tokens": 100}, "content": []}});

    assert_eq!(
        use_of(parser.line(&message("m1", 5))),
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
        use_of(parser.line(&message("m1", 9))).map(|usage| usage.output),
        Some(9)
    );
    assert_eq!(
        use_of(parser.line(&message("m2", 1))).map(|usage| (usage.input, usage.output, usage.cache_read)),
        Some((4, 10, 200))
    );

    // the turn end: a new turn starts at 0
    parser.line(&json!({"type": "result", "subtype": "success", "usage": {}}));

    assert_eq!(
        use_of(parser.line(&message("m3", 1))).map(|usage| usage.output),
        Some(1)
    );
}

fn usage_of(output: &Out) -> Option<TurnUse> {
    if let Out::Usage(usage) = output {
        Some(*usage)
    } else {
        None
    }
}

#[test]
fn parser_maps_stream_json() {
    let mut parser = Parser::default();

    let events = |output: Vec<Out>| -> Vec<Kind> { output.into_iter().filter_map(event_of).collect() };

    let init = json!({"type": "system", "subtype": "init", "session_id": "s1", "model": "claude-opus-5-5"});

    assert_eq!(
        events(parser.line(&init)),
        vec![Kind::Session {
            session: "s1".into(),
            model: "claude-opus-5-5".into()
        }]
    );

    let assistant = json!({"type": "assistant", "parent_tool_use_id": null, "message": {"usage": {"input_tokens": 3, "cache_read_input_tokens": 100}, "content": [
        {"type": "thinking", "thinking": ""}, {"type": "text", "text": "hi"}, {"type": "tool_use", "id": "t1", "name": "Bash", "input": {"command": "ls"}}]}});
    let kinds = events(parser.line(&assistant));

    assert_eq!(kinds.len(), 2);
    assert_eq!(kinds[0], Kind::Text { text: "hi".into() });

    // a thinking block with a summary: 1 thought, trimmed
    let thinking = json!({"type": "assistant", "parent_tool_use_id": null, "message": {"content": [
        {"type": "thinking", "thinking": "  ", "signature": "x"}, {"type": "thinking", "thinking": "CI=true missing, fixing it.\n\n", "signature": "y"}]}});

    assert_eq!(
        events(parser.line(&thinking)),
        vec![Kind::Thought {
            text: "CI=true missing, fixing it.".into()
        }]
    );

    // token chunks: text deltas of the main agent only
    let chunk = |parent: Value, kind: &str| json!({"type": "stream_event", "parent_tool_use_id": parent, "event": {"type": "content_block_delta", "index": 1, "delta": {"type": kind, "text": "Hel", "thinking": "x"}}});

    assert!(matches!(parser.line(&chunk(Value::Null, "text_delta")).as_slice(), [Out::Delta(text)] if text == "Hel"));

    assert_subagents_results_and_turn_ends(parser, events, chunk);
}

fn assert_subagents_results_and_turn_ends(
    mut parser: Parser,
    events: impl Fn(Vec<Out>) -> Vec<Kind>,
    chunk: impl Fn(Value, &str) -> Value,
) {
    assert!(parser.line(&chunk(json!("t9"), "text_delta")).is_empty());
    assert!(parser.line(&chunk(Value::Null, "thinking_delta")).is_empty());

    let subagent = json!({"type": "assistant", "parent_tool_use_id": "t9", "message": {"content": [{"type": "text", "text": "sub agent"}]}});

    assert_eq!(events(parser.line(&subagent)), [] as [crate::engine::store::Kind; 0]);

    let result = json!({"type": "user", "parent_tool_use_id": null, "message": {"content": [{"type": "tool_result", "tool_use_id": "t1", "content": [{"type": "text", "text": "a\nb"}]}]}});

    assert_eq!(
        events(parser.line(&result)),
        vec![Kind::ToolResult {
            id: "t1".into(),
            ok: true,
            output: "a\nb".into()
        }]
    );

    let end = |total: f64| json!({"type": "result", "subtype": "success", "total_cost_usd": total, "duration_ms": 5, "usage": {"input_tokens": 1, "output_tokens": 2}});

    let Kind::TurnEnd { cost, ctx, window, .. } = events(parser.line(&end(0.5))).remove(0) else {
        panic!()
    };

    assert_eq!((cost, ctx, window), (0.5, 103, 0));

    // the window: the session model's `modelUsage` entry, not a subagent's
    let mut with_windows = end(0.5);

    with_windows["modelUsage"] =
        json!({"claude-haiku-5-5": {"contextWindow": 200_000}, "claude-opus-5-5": {"contextWindow": 1_000_000}});

    let Kind::TurnEnd { window, .. } = events(parser.line(&with_windows)).remove(0) else {
        panic!()
    };

    assert_eq!(window, 1_000_000);

    let Kind::TurnEnd { cost, .. } = events(parser.line(&end(0.75))).remove(0) else {
        panic!()
    };

    assert_eq!(cost, 0.25);

    let control = json!({"type": "control_request", "request_id": "r1", "request": {"subtype": "can_use_tool"}});

    assert!(matches!(&parser.line(&control)[0], Out::Control(id, _) if id == "r1"));
}

fn event_of(output: Out) -> Option<Kind> {
    if let Out::Event(kind) = output {
        Some(kind)
    } else {
        None
    }
}
