use super::*;
use serde_json::json;

#[test]
fn claude_list_lines_parse_with_states_sources_and_errors() {
    let text = "Checking MCP server health…\n\nclaude.ai Linear: https://mcp.linear.app/mcp - ✔ Connected\nclaude.ai Box: https://mcp.box.com - ! Needs authentication\nplugin:general:domerge: https://d.dev/mcp (HTTP) - ✘ Failed to connect — HTTP 525: Error POSTing\nfiles: npx -y fs /tmp - ⏸ Pending approval\n";
    let value = parse_claude(text);

    assert_eq!(value.len(), 4, "{value:#?}");
    assert_eq!(
        (
            value[0].name.as_str(),
            value[0].source.as_str(),
            value[0].status.state.as_str()
        ),
        ("claude.ai Linear", "claudeai", "connected")
    );
    assert_eq!(value[1].status.state, "needs-auth");
    assert_eq!(
        (
            value[2].name.as_str(),
            value[2].source.as_str(),
            value[2].status.state.as_str()
        ),
        ("plugin:general:domerge", "plugin", "failed")
    );
    assert_eq!(value[2].target, "https://d.dev/mcp (HTTP)");
    assert!(
        value[2].status.error.starts_with("HTTP 525"),
        "{}",
        value[2].status.error
    );
    assert_eq!(
        (value[3].source.as_str(), value[3].status.state.as_str()),
        ("user", "pending")
    );
}

#[test]
fn codex_json_and_opencode_text_parse() {
    let value = parse_codex(
        r#"[{"name":"docs","transport":{"type":"stdio","command":"npx","args":["-y","d"]},"enabled":true},{"name":"web","url":"https://w/mcp","enabled":false}]"#,
    );

    assert_eq!(value.len(), 2);
    assert_eq!(
        (value[0].target.as_str(), value[0].status.state.as_str()),
        ("npx -y d", "unknown")
    );
    assert_eq!(
        (value[1].target.as_str(), value[1].status.state.as_str()),
        ("https://w/mcp", "off")
    );
    assert_eq!(parse_codex("[]"), [] as [crate::infra::mcp_list::Native; 0]);
    assert_eq!(
        parse_opencode("No MCP servers configured\n"),
        [] as [crate::infra::mcp_list::Native; 0]
    );

    let output = parse_opencode("● ✓ sentry connected\n● ✗ jira failed: 401\n");

    assert_eq!(
        output
            .iter()
            .map(|count| (count.name.as_str(), count.status.state.as_str()))
            .collect::<Vec<_>>(),
        [("sentry", "connected"), ("jira", "failed")]
    );
}

#[test]
fn entries_become_codehead_servers() {
    let text = from_json("fs", &sample());

    assert_eq!(
        (
            text.kind.as_str(),
            text.command.as_deref(),
            text.args.len(),
            text.env["K"].as_str()
        ),
        ("stdio", Some("npx"), 2, "v")
    );

    let height = from_json(
        "w",
        &json!({"type": "http", "url": "https://w/mcp", "headers": {"Authorization": "Bearer x"}}),
    );

    assert_eq!((height.kind.as_str(), height.headers.len()), ("http", 1));
    assert_eq!(
        from_json("c", &json!({"type": "streamable_http", "url": "https://c"})).kind,
        "http"
    );
    assert_eq!(
        login_command("opencode", "x").unwrap(),
        ["opencode", "mcp", "auth", "x"]
    );
    assert!(login_command("goose", "x").is_none());
}

#[test]
fn probe_counts_the_tools_of_a_stdio_server() {
    // a tiny MCP server in sh: the init reply, then the tool list
    let script = r#"read a; echo '{"jsonrpc":"2.0","id":1,"result":{"capabilities":{}}}'; read b; read c; echo '{"jsonrpc":"2.0","id":2,"result":{"tools":[{"name":"a"},{"name":"b"},{"name":"c"}]}}'; sleep 1"#;
    let text = Server {
        name: "t".into(),
        kind: "stdio".into(),
        command: Some("sh".into()),
        args: vec!["-c".into(), script.into()],
        auto_allow: true,
        ..Default::default()
    };
    let st = probe(&text);

    assert_eq!((st.state.as_str(), st.tools), ("connected", Some(3)), "{st:?}");

    let bad = Server {
        command: Some("/nonexistent/x".into()),
        ..text
    };

    assert_eq!(probe(&bad).state, "failed");
}

#[test]
fn event_stream_replies_parse() {
    let body = "event: message\ndata: {\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{\"tools\":[{}]}}\n\n";

    assert_eq!(reply_of(body).unwrap()["result"]["tools"].as_array().unwrap().len(), 1);
    assert_eq!(reply_of("{\"id\":1,\"result\":{}}").unwrap()["id"], 1);
}
