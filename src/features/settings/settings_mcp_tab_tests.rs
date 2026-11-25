use super::*;

fn tab(servers: Vec<Server>, natives: Vec<Native>) -> McpTab {
    let mut mcp_tab = McpTab::new();

    mcp_tab.servers = servers;
    mcp_tab.natives = natives;
    mcp_tab.statuses = BTreeMap::new();
    mcp_tab.agents = Vec::new();

    mcp_tab
}

fn native(harness: &str, name: &str, source: &str, state: &str) -> Native {
    Native {
        harness: harness.into(),
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
    let mcp_tab = tab(
        vec![text],
        vec![
            native("claude", "claude.ai Linear", "claudeai", "connected"),
            native("claude", "plugin:general:figma", "plugin", "needs-auth"),
            native("claude", "local-db", "user", "failed"),
        ],
    );
    let (items, actions) = mcp_tab.items();

    assert_eq!(actions, 2, "{items:#?}");
    assert_eq!(items[0].group, "needs action");

    let groups: Vec<&str> = items[actions..].iter().map(|item| item.group).collect();

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
    let mut mcp_tab = tab(
        Vec::new(),
        vec![
            native("claude", "claude.ai Linear", "claudeai", "needs-auth"),
            native("claude", "claude.ai Box", "claudeai", "needs-auth"),
        ],
    );

    mcp_tab.ever.insert("claude.ai Linear".into());
    let (items, actions) = mcp_tab.items();

    assert_eq!(actions, 1, "{items:#?}");
    assert_eq!(items[0].name, "claude.ai Linear");

    let box_row = items.iter().find(|item| item.name == "claude.ai Box").unwrap();

    assert_eq!((box_row.group, box_row.action.as_str()), (UNUSED, ""));
    assert_eq!(items.last().unwrap().group, UNUSED);
}

#[test]
fn the_filter_keeps_fuzzy_matches_of_names_and_groups() {
    let mut mcp_tab = tab(
        Vec::new(),
        vec![
            native("claude", "claude.ai Linear", "claudeai", "connected"),
            native("claude", "claude.ai Slack", "claudeai", "connected"),
            native("codex", "docs", "config", "unknown"),
        ],
    );

    mcp_tab.filter.set_text("slk");

    assert_eq!(
        mcp_tab
            .items()
            .0
            .iter()
            .map(|item| item.name.as_str())
            .collect::<Vec<_>>(),
        ["claude.ai Slack"]
    );

    mcp_tab.filter.set_text("codex");

    assert_eq!(
        mcp_tab
            .items()
            .0
            .iter()
            .map(|item| item.name.as_str())
            .collect::<Vec<_>>(),
        ["docs"]
    );
}

#[test]
fn a_harness_failure_marks_a_codehead_server() {
    let server = Server {
        name: "web".into(),
        kind: "http".into(),
        url: Some("https://w/mcp".into()),
        auto_allow: true,
        ..Default::default()
    };
    let mut mcp_tab = tab(vec![server], Vec::new());
    let mut statuses = BTreeMap::new();

    statuses.insert(
        "web".to_string(),
        Status {
            state: "failed".into(),
            error: "harness: 0 HTTP MCP support (ACP mcpCapabilities.http)".into(),
            source: "codehead".into(),
            ..Default::default()
        },
    );
    mcp_tab.statuses.insert("codex".into(), statuses);
    let (items, actions) = mcp_tab.items();

    assert_eq!(actions, 1);
    assert!(
        items[0].action.starts_with("Codex: harness: 0 HTTP"),
        "{}",
        items[0].action
    );

    let Modal::Info { lines, .. } = super::info_for(&items[0], &mcp_tab) else {
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
    let mut form = EditForm::of(None);

    form.name.set_text("fs");
    form.target.set_text("npx -y '@m/server fs' /tmp");
    form.secrets.set_text("TOKEN=abc\n\nOTHER=${X}\n");
    let stdio = super::server(&form).unwrap();

    assert_eq!(
        (stdio.command.as_deref(), stdio.args.clone()),
        (
            Some("npx"),
            vec!["-y".to_string(), "@m/server fs".into(), "/tmp".into()]
        )
    );
    assert_eq!(stdio.env.len(), 2);
    assert!(stdio.check().is_ok());

    form.kind = 1;
    form.target.set_text("https://x/mcp");
    form.secrets.set_text("Authorization=${TOK}");
    let http = super::server(&form).unwrap();

    assert_eq!(
        (http.kind.as_str(), http.url.as_deref(), http.headers.len()),
        ("http", Some("https://x/mcp"), 1)
    );

    form.secrets.set_text("no-equals");

    assert!(super::server(&form).is_err());

    // an edit shows references, never resolved values
    let back = EditForm::of(Some(&http));

    assert_eq!(back.secrets.text(), "Authorization=${TOK}");
}

/// The tab with this machine's harness lists: `cargo test live_lists -- --ignored --nocapture`.
#[test]
#[ignore = "requires local harness CLI configuration"]
fn live_lists_render() {
    let found = mcp_list::refresh();
    let mut mcp_tab = tab(Vec::new(), found.natives);

    mcp_tab.jobs.cli_errors = found.errors;
    mcp_tab.ever = read_ever(&crate::infra::config::home().join(".claude.json"));
    let mut term = ratatui::Terminal::new(ratatui::backend::TestBackend::new(120, 50)).unwrap();

    term.draw(|frame| mcp_tab.draw(frame, frame.area(), true)).unwrap();
    let buffer = term.backend().buffer().clone();

    let screen: Vec<String> = (0..50)
        .map(|y| {
            (0..120)
                .map(|x| buffer[(x, y)].symbol())
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
    let mut server = Server {
        name: "x".into(),
        kind: "stdio".into(),
        command: Some("a".into()),
        auto_allow: true,
        ..Default::default()
    };

    server.env.insert("K".into(), "${VAR}".into());

    assert!(secure(&mut server, None).is_ok());
    assert_eq!(server.env["K"], "${VAR}");
}
