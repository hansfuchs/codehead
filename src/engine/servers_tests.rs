use super::*;

fn server(name: &str, kind: &str) -> Server {
    Server {
        name: name.into(),
        kind: kind.into(),
        auto_allow: true,
        ..Default::default()
    }
}

#[test]
fn check_names_and_kinds() {
    assert!(valid_name("linear-2_x"));
    assert!(!valid_name("2x"));
    assert!(!valid_name("a b"));

    let mut stdio = server("files", "stdio");

    assert!(stdio.check().is_err());

    stdio.command = Some("npx".into());

    assert!(stdio.check().is_ok());

    let mut http = server("lin", "http");

    http.url = Some("ftp://x".into());

    assert!(http.check().is_err());

    http.url = Some("https://x/mcp".into());

    assert!(http.check().is_ok());
    assert!(server("x", "ws").check().is_err());
}

#[test]
fn toml_round_trip_with_defaults() {
    let text = "[servers.lin]\ntype = \"http\"\nurl = \"https://x/mcp\"\nheaders = { Authorization = \"${LIN}\" }\n\n[servers.files]\ncommand = \"npx\"\nauto_allow = false\n";
    let file: FileRec = toml::from_str(text).unwrap();
    let lin = &file.servers["lin"];

    assert_eq!((lin.kind.as_str(), lin.auto_allow), ("http", true));

    let files = &file.servers["files"];

    assert_eq!((files.kind.as_str(), files.auto_allow), ("stdio", false));

    let back = toml::to_string_pretty(&file).unwrap();

    assert!(back.contains("type = \"http\"") && !back.contains("name"), "{back}");
}

#[test]
fn references_resolve_and_plain_values_show() {
    assert!(is_ref("${A_1}") && is_ref("keychain:x/y") && !is_ref("abc") && !is_ref("${}") && !is_ref("${a-b}"));

    // SAFETY: tests of this module set only this var.
    unsafe { std::env::set_var("CODEHEAD_TEST_SECRET_9", "s3") };

    assert_eq!(resolve("${CODEHEAD_TEST_SECRET_9}").unwrap(), "s3");
    assert!(resolve("${CODEHEAD_TEST_UNSET_9}").is_err());
    assert_eq!(resolve("plain").unwrap(), "plain");

    let mut stdio = server("x", "stdio");

    stdio.env.insert("A".into(), "${X}".into());
    stdio.env.insert("B".into(), "plain".into());

    assert_eq!(stdio.plain_secrets(), ["B"]);
}

#[test]
fn claude_config_keeps_secrets_out_of_the_argument() {
    unsafe { std::env::set_var("CODEHEAD_TEST_SECRET_8", "tok") };
    let mut linear = server("lin", "http");

    linear.url = Some("https://x/mcp".into());
    linear
        .headers
        .insert("Authorization".into(), "${CODEHEAD_TEST_SECRET_8}".into());
    let mut bad = server("bad", "stdio");

    bad.command = Some("x".into());
    bad.env.insert("K".into(), "${CODEHEAD_TEST_UNSET_8}".into());
    let (entries, env, errors) = claude_config(&[linear, bad]);

    assert_eq!(entries["lin"]["headers"]["Authorization"], "${CODEHEAD_MCP_0}");
    assert!(!Value::Object(entries.clone()).to_string().contains("tok"));
    assert_eq!(env, [("CODEHEAD_MCP_0".to_string(), "tok".to_string())]);
    assert!(!entries.contains_key("bad"));
    assert_eq!(errors[0].0, "bad");
}

#[test]
fn acp_config_skips_transports_the_agent_lacks() {
    let mut linear = server("lin", "http");

    linear.url = Some("https://x/mcp".into());
    let mut file = server("files", "stdio");

    file.command = Some("npx".into());
    let (out, errors) = acp_config(&[linear.clone(), file.clone()], false, false);

    assert_eq!(out.len(), 1);
    assert_eq!(out[0]["name"], "files");
    assert!(errors[0].1.contains("HTTP"), "{errors:?}");

    let (out, errors) = acp_config(&[linear, file], true, false);

    assert_eq!((out.len(), errors.len()), (2, 0));
    assert_eq!(out[0]["type"], "http");
}

/// The OS keychain for real: `cargo test keychain -- --ignored` (writes 1 entry, deletes it).
#[test]
#[ignore = "writes to the OS keychain"]
fn keychain_round_trip() {
    let reference = store_secret("codehead-test", "TOKEN", "s3cret").expect("keychain");

    assert_eq!(reference, "keychain:codehead-test/TOKEN");
    assert_eq!(resolve(&reference).unwrap(), "s3cret");

    forget_secret(&reference);

    assert!(resolve(&reference).is_err());
}

#[test]
fn acp_tool_titles_match_their_server() {
    let mut off = server("slack", "http");

    off.auto_allow = false;
    let servers = [server("linear", "http"), off];

    assert!(acp_auto_allowed("linear__save_issue", &servers));
    assert!(acp_auto_allowed("mcp__linear__get_issue", &servers));
    assert!(acp_auto_allowed("Linear/get_issue", &servers));
    assert!(!acp_auto_allowed("linearize", &servers));
    assert!(!acp_auto_allowed("slack__send", &servers));
}

#[test]
fn init_events_give_states_and_tool_counts() {
    // the shape Claude Code 2.x sends
    let value = json!({"mcp_servers": [{"name": "claude.ai Linear", "status": "connected", "source": "claudeai"}, {"name": "plugin:general:figma", "status": "needs-auth", "source": "plugin"},
        {"name": "codehead", "status": "connected"}, {"name": "slow", "status": "pending"}, {"name": "plugin:x:y", "status": "connected", "source": "plugin"}],
        "tools": ["Bash", "mcp__claude_ai_Linear__get_issue", "mcp__claude_ai_Linear__save_issue", "mcp__codehead__send", "mcp__plugin_x_y__a"]});
    let rows = from_init(&value);

    assert_eq!(
        rows.iter().map(|row| row.0.as_str()).collect::<Vec<_>>(),
        ["claude.ai Linear", "plugin:general:figma", "plugin:x:y"]
    );
    assert_eq!(
        (rows[0].1.state.as_str(), rows[0].1.tools, rows[0].1.source.as_str()),
        ("connected", Some(2), "claudeai")
    );
    assert_eq!((rows[1].1.state.as_str(), rows[1].1.tools), ("needs-auth", None));
    assert_eq!(rows[2].1.tools, Some(1));
}
