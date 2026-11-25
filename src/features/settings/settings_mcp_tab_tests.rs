use super::*;

fn tab(servers: Vec<Server>, natives: Vec<Native>) -> McpTab {
    let mut thread = McpTab::new();

    thread.servers = servers;
    thread.natives = natives;
    thread.statuses = BTreeMap::new();
    thread.agents = Vec::new();

    thread
}

fn native(height: &str, name: &str, source: &str, state: &str) -> Native {
    Native {
        harness: height.into(),
        name: name.into(),
        source: source.into(),
        target: String::new(),
        status: Status {
            state: state.into(),
            ..Default::default()
        },
    }
}

#[test]
fn rows_group_by_source_with_needs_action_first() {
    let text = Server {
        name: "files".into(),
        kind: "stdio".into(),
        command: Some("npx".into()),
        auto_allow: true,
        ..Default::default()
    };
    let thread = tab(
        vec![text],
        vec![
            native("claude", "claude.ai Linear", "claudeai", "connected"),
            native("claude", "plugin:general:figma", "plugin", "needs-auth"),
            native("claude", "local-db", "user", "failed"),
        ],
    );
    let (items, actions) = thread.items();

    assert_eq!(actions, 2, "{items:#?}");
    assert_eq!(items[0].group, "needs action");

    let groups: Vec<&str> = items[actions..].iter().map(|i| i.group).collect();

    assert_eq!(
        groups,
        [
            "codehead · every harness",
            "Claude Code · own config",
            "claude.ai connectors · Claude Code only",
            "Claude Code plugins · Claude Code only"
        ]
    );
    assert!(items[1].action.contains("sign") || items[0].action.contains("sign"));
}

#[test]
fn connectors_never_connected_go_to_unused_not_needs_action() {
    let mut thread = tab(
        Vec::new(),
        vec![
            native("claude", "claude.ai Linear", "claudeai", "needs-auth"),
            native("claude", "claude.ai Box", "claudeai", "needs-auth"),
        ],
    );

    thread.ever.insert("claude.ai Linear".into());
    let (items, actions) = thread.items();

    assert_eq!(actions, 1, "{items:#?}");
    assert_eq!(items[0].name, "claude.ai Linear");

    let box_row = items.iter().find(|i| i.name == "claude.ai Box").unwrap();

    assert_eq!((box_row.group, box_row.action.as_str()), (UNUSED, ""));
    assert_eq!(items.last().unwrap().group, UNUSED);
}

#[test]
fn the_filter_keeps_fuzzy_matches_of_names_and_groups() {
    let mut thread = tab(
        Vec::new(),
        vec![
            native("claude", "claude.ai Linear", "claudeai", "connected"),
            native("claude", "claude.ai Slack", "claudeai", "connected"),
            native("codex", "docs", "config", "unknown"),
        ],
    );

    thread.filter.set_text("slk");

    assert_eq!(
        thread.items().0.iter().map(|i| i.name.as_str()).collect::<Vec<_>>(),
        ["claude.ai Slack"]
    );

    thread.filter.set_text("codex");

    assert_eq!(
        thread.items().0.iter().map(|i| i.name.as_str()).collect::<Vec<_>>(),
        ["docs"]
    );
}

#[test]
fn a_harness_failure_marks_a_codehead_server() {
    let text = Server {
        name: "web".into(),
        kind: "http".into(),
        url: Some("https://w/mcp".into()),
        auto_allow: true,
        ..Default::default()
    };
    let mut thread = tab(vec![text], Vec::new());
    let mut message = BTreeMap::new();

    message.insert(
        "web".to_string(),
        Status {
            state: "failed".into(),
            error: "harness: 0 HTTP MCP support (ACP mcpCapabilities.http)".into(),
            source: "codehead".into(),
            ..Default::default()
        },
    );
    thread.statuses.insert("codex".into(), message);
    let (items, actions) = thread.items();

    assert_eq!(actions, 1);
    assert!(
        items[0].action.starts_with("Codex: harness: 0 HTTP"),
        "{}",
        items[0].action
    );

    let Modal::Info { lines, .. } = super::info_for(&items[0], &thread) else {
        panic!()
    };

    assert!(
        lines
            .iter()
            .any(|line| line.contains("codex mcp add web --url https://w/mcp")),
        "{lines:#?}"
    );
}

#[test]
fn the_edit_form_builds_stdio_and_http_servers() {
    let mut error = EditForm::of(None);

    error.name.set_text("fs");
    error.target.set_text("npx -y '@m/server fs' /tmp");
    error.secrets.set_text("TOKEN=abc\n\nOTHER=${X}\n");
    let text_value = super::server(&error).unwrap();

    assert_eq!(
        (text_value.command.as_deref(), text_value.args.clone()),
        (
            Some("npx"),
            vec!["-y".to_string(), "@m/server fs".into(), "/tmp".into()]
        )
    );
    assert_eq!(text_value.env.len(), 2);
    assert!(text_value.check().is_ok());

    error.kind = 1;
    error.target.set_text("https://x/mcp");
    error.secrets.set_text("Authorization=${TOK}");
    let height = super::server(&error).unwrap();

    assert_eq!(
        (height.kind.as_str(), height.url.as_deref(), height.headers.len()),
        ("http", Some("https://x/mcp"), 1)
    );

    error.secrets.set_text("no-equals");

    assert!(super::server(&error).is_err());

    // an edit shows references, never resolved values
    let back = EditForm::of(Some(&height));

    assert_eq!(back.secrets.text(), "Authorization=${TOK}");
}

/// The tab with this machine's harness lists: `cargo test live_lists -- --ignored --nocapture`.
#[test]
#[ignore = "requires local harness CLI configuration"]
fn live_lists_render() {
    let found = mcp_list::refresh();
    let mut thread = tab(Vec::new(), found.natives);

    thread.jobs.cli_errors = found.errors;
    thread.ever = read_ever(&crate::infra::config::home().join(".claude.json"));
    let mut term = ratatui::Terminal::new(ratatui::backend::TestBackend::new(120, 50)).unwrap();

    term.draw(|file| thread.draw(file, file.area(), true)).unwrap();
    let bytes = term.backend().buffer().clone();

    let screen: Vec<String> = (0..50)
        .map(|y| {
            (0..120)
                .map(|x| bytes[(x, y)].symbol())
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect();

    println!("{}", screen.join("\n"));

    assert!(screen.iter().any(|line| line.contains("needs action")));
}

#[test]
fn env_references_need_no_keychain() {
    let mut text = Server {
        name: "x".into(),
        kind: "stdio".into(),
        command: Some("a".into()),
        auto_allow: true,
        ..Default::default()
    };

    text.env.insert("K".into(), "${VAR}".into());

    assert!(secure(&mut text, None).is_ok());
    assert_eq!(text.env["K"], "${VAR}");
}
