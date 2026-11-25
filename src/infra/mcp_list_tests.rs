use super::*;
use serde_json::json;

#[test]
fn claude_list_lines_parse_with_states_sources_and_errors() {
    let text = "Checking MCP server health…\n\nclaude.ai Linear: https://mcp.linear.app/mcp - ✔ Connected\nclaude.ai Box: https://mcp.box.com - ! Needs authentication\nplugin:general:domerge: https://d.dev/mcp (HTTP) - ✘ Failed to connect — HTTP 525: Error POSTing\nfiles: npx -y fs /tmp - ⏸ Pending approval\n";
    let natives = parse_claude(text);

    assert_eq!(natives.len(), 4, "{natives:#?}");
    assert_eq!(
        (
            natives[0].name.as_str(),
            natives[0].source.as_str(),
            natives[0].status.state.as_str()
        ),
        ("claude.ai Linear", "claudeai", "connected")
    );
    assert_eq!(natives[1].status.state, "needs-auth");
    assert_eq!(
        (
            natives[2].name.as_str(),
            natives[2].source.as_str(),
            natives[2].status.state.as_str()
        ),
        ("plugin:general:domerge", "plugin", "failed")
    );
    assert_eq!(natives[2].target, "https://d.dev/mcp (HTTP)");
    assert!(
        natives[2].status.error.starts_with("HTTP 525"),
        "{}",
        natives[2].status.error
    );
    assert_eq!(
        (natives[3].source.as_str(), natives[3].status.state.as_str()),
        ("user", "pending")
    );
}

#[test]
fn codex_json_and_opencode_text_parse() {
    let natives = parse_codex(
        r#"[{"name":"docs","transport":{"type":"stdio","command":"npx","args":["-y","d"]},"enabled":true},{"name":"web","url":"https://w/mcp","enabled":false}]"#,
    );

    assert_eq!(natives.len(), 2);
    assert_eq!(
        (natives[0].target.as_str(), natives[0].status.state.as_str()),
        ("npx -y d", "unknown")
    );
    assert_eq!(
        (natives[1].target.as_str(), natives[1].status.state.as_str()),
        ("https://w/mcp", "off")
    );
    assert_eq!(parse_codex("[]"), [] as [crate::infra::mcp_list::Native; 0]);
    assert_eq!(
        parse_opencode("No MCP servers configured\n"),
        [] as [crate::infra::mcp_list::Native; 0]
    );

    let natives = parse_opencode("● ✓ sentry connected\n● ✗ jira failed: 401\n");

    assert_eq!(
        natives
            .iter()
            .map(|native| (native.name.as_str(), native.status.state.as_str()))
            .collect::<Vec<_>>(),
        [("sentry", "connected"), ("jira", "failed")]
    );
}

#[test]
fn entries_become_codehead_servers() {
    let server = from_json("fs", &sample());

    assert_eq!(
        (
            server.kind.as_str(),
            server.command.as_deref(),
            server.args.len(),
            server.env["K"].as_str()
        ),
        ("stdio", Some("npx"), 2, "v")
    );

    let http = from_json(
        "w",
        &json!({"type": "http", "url": "https://w/mcp", "headers": {"Authorization": "Bearer x"}}),
    );

    assert_eq!((http.kind.as_str(), http.headers.len()), ("http", 1));
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
    let server = Server {
        name: "t".into(),
        kind: "stdio".into(),
        command: Some("sh".into()),
        args: vec!["-c".into(), script.into()],
        auto_allow: true,
        ..Default::default()
    };
    let st = probe(&server);

    assert_eq!((st.state.as_str(), st.tools), ("connected", Some(3)), "{st:?}");

    let bad = Server {
        command: Some("/nonexistent/x".into()),
        ..server
    };

    assert_eq!(probe(&bad).state, "failed");
}

#[test]
fn event_stream_replies_parse() {
    let body = "event: message\ndata: {\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{\"tools\":[{}]}}\n\n";

    assert_eq!(reply_of(body).unwrap()["result"]["tools"].as_array().unwrap().len(), 1);
    assert_eq!(reply_of("{\"id\":1,\"result\":{}}").unwrap()["id"], 1);
}
