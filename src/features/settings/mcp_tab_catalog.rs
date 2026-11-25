use super::{HashSet, Item, Key, McpTab, Modal, Native, Server, Style, UNUSED, harnesses, mcp_list, theme};

pub(super) fn icon(state: &str) -> (&'static str, Style) {
    let palette = theme::pal();

    match state {
        "connected" => ("✔", Style::default().fg(palette.success())),
        "needs-auth" => ("⚠", Style::default().fg(palette.attention())),
        "failed" => ("✘", Style::default().fg(palette.error())),
        "pending" => ("⏸", Style::default().fg(palette.progress())),
        "off" => ("○", theme::muted()),
        _ => ("·", theme::muted()),
    }
}

pub(super) fn harness_label(harness: &str) -> &'static str {
    harnesses::get(harness).map_or("harness", |x| x.label)
}

/// `claudeAiMcpEverConnected` of ~/.claude.json: connectors that were connected once.
pub(super) fn ever_connected() -> std::collections::HashSet<String> {
    if cfg!(test) {
        return HashSet::default();
    }

    read_ever(&crate::infra::config::home().join(".claude.json"))
}

pub(super) fn read_ever(path: &std::path::Path) -> std::collections::HashSet<String> {
    let value: serde_json::Value = std::fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default();

    value["claudeAiMcpEverConnected"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|text| text.as_str().map(String::from))
        .collect()
}

pub(super) fn harness_order(harness: &str) -> usize {
    harnesses::ALL.iter().position(|x| x.name == harness).unwrap_or(99)
}

pub(super) fn codehead_item(server: &Server, mcp_tab: &McpTab) -> Item {
    let probe = mcp_tab
        .statuses
        .get("codehead")
        .and_then(|statuses_value| statuses_value.get(&server.name));

    // a harness that could not run it (a session's report)
    let failed: Vec<(String, String)> = mcp_tab
        .statuses
        .iter()
        .filter(|(harness, _)| *harness != "codehead")
        .filter_map(|(harness_value, statuses_2)| {
            statuses_2
                .get(&server.name)
                .filter(|status| status.source == "codehead" && !status.error.is_empty())
                .map(|status_value| (harness_value.clone(), status_value.error.clone()))
        })
        .collect();

    let users = mcp_tab
        .agents
        .iter()
        .filter(|agent| agent.mcp.contains(&server.name))
        .count();
    let plain = server.plain_secrets();
    let mut state = probe.map_or("unknown".to_string(), |status_2| status_2.state.clone());
    let mut action = String::new();

    if let Some((harness_2, error_2)) = failed.first() {
        action = format!("{}: {error_2}", harness_label(harness_2));

        if state == "connected" || state == "unknown" {
            state = "failed".into();
        }
    } else if let Some(failed_probe) = probe.filter(|probe_value| probe_value.state != "connected") {
        action.clone_from(&failed_probe.error);
    }

    if !plain.is_empty() {
        action = format!(
            "plain text secrets: {} (edit to move into the keychain)",
            plain.join(", ")
        );
    }

    let tools = probe
        .and_then(|status_3| status_3.tools)
        .map(|tool_count| format!("{tool_count} tools · "))
        .unwrap_or_default();
    let allow = if server.auto_allow { " · auto-allow" } else { "" };

    let who = if users == 0 {
        "0 agents".to_string()
    } else {
        format!("{users} agent{}", if users == 1 { "" } else { "s" })
    };

    Item {
        key: Key::Codehead(server.name.clone()),
        group: "codehead · every harness",
        name: server.name.clone(),
        state,
        detail: format!("{tools}{who}{allow}"),
        action,
    }
}

pub(super) fn native_item(native: &Native, mcp_tab: &McpTab) -> Item {
    // a connector the account offers but Claude Code never connected: Claude Code's
    // "unused connectors", 0 action needed
    let unused =
        native.source == "claudeai" && native.status.state == "needs-auth" && !mcp_tab.ever.contains(&native.name);

    if unused {
        let action = String::new();

        return Item {
            key: Key::Native {
                harness: native.harness.clone(),
                name: native.name.clone(),
            },
            group: UNUSED,
            name: native.name.clone(),
            state: "off".into(),
            detail: "not connected · l: connect".into(),
            action,
        };
    }

    let group = match (native.harness.as_str(), native.source.as_str()) {
        ("claude", "claudeai") => "claude.ai connectors · Claude Code only",
        ("claude", "plugin") => "Claude Code plugins · Claude Code only",
        ("claude", _) => "Claude Code · own config",
        ("codex", _) => "Codex · own config",
        _ => "OpenCode · own config",
    };

    // the tool count: from the last session's init (Claude Code), else the list's state
    let status_value = mcp_tab
        .statuses
        .get(&native.harness)
        .and_then(|statuses_value| statuses_value.get(&native.name))
        .unwrap_or(&native.status);
    let state = native.status.state.clone();

    let detail = match state.as_str() {
        "connected" => status_value
            .tools
            .map_or_else(|| "connected".into(), |tool_count| format!("{tool_count} tools")),
        "needs-auth" => "needs sign-in".into(),
        "pending" => "pending approval".into(),
        "off" => "disabled".into(),
        "failed" => "failed".into(),
        _ => String::new(),
    };

    let action = match state.as_str() {
        "needs-auth" => format!("needs sign-in · l: sign in through {}", harness_label(&native.harness)),
        "failed" => native_item_failed(native),
        "pending" => "pending approval in the project".into(),
        _ => String::new(),
    };

    Item {
        key: Key::Native {
            harness: native.harness.clone(),
            name: native.name.clone(),
        },
        group,
        name: native.name.clone(),
        state,
        detail,
        action,
    }
}

fn native_item_failed(native: &Native) -> String {
    if native.status.error.is_empty() {
        "failed to connect".into()
    } else {
        native.status.error.clone()
    }
}

/// What the owner can do about row IT: the error, the steps.
pub(super) fn info_for(item: &Item, mcp_tab: &McpTab) -> Modal {
    match &item.key {
        Key::Codehead(name) => {
            let server_value = mcp_tab.servers.iter().find(|server| &server.name == name);
            let target = server_value.map(Server::target).unwrap_or_default();
            let mut lines = vec![format!("{name}: {}", item.action), String::new()];

            if item.action.contains("HTTP") || item.action.contains("SSE") {
                lines.extend([
                    "This harness cannot run HTTP or SSE MCP servers that codehead hands it.".to_string(),
                    "Add the server in the harness itself, then sign in there:".to_string(),
                    format!("  codex mcp add {name} --url {target}"),
                    format!("  codex mcp login {name}"),
                    "Then turn it off for that harness here (g: agents).".to_string(),
                ]);
            } else if item.state == "needs-auth" {
                lines.extend([
                    "codehead cannot sign in to an OAuth server for a harness.".to_string(),
                    "Add it in each harness and sign in there:".to_string(),
                    format!("  claude mcp add --transport http --scope user {name} {target}"),
                    format!("  claude mcp login {name}"),
                    "Then delete it here (d): the harness has it on its own.".to_string(),
                ]);
            } else {
                lines.extend([
                    "Check the command or URL and its secrets (enter on the row: edit).".to_string(),
                    "Run it by hand in a terminal to see its error, or add it in the harness".to_string(),
                    "and connect it there (claude: /mcp).".to_string(),
                ]);
            }

            Modal::Info {
                title: format!("{name} · cannot connect"),
                lines,
                login: None,
            }
        }
        Key::Native { harness, name } => native_server_info(item, harness, name),
    }
}

fn native_server_info(item: &Item, harness: &String, name: &String) -> Modal {
    let label = harness_label(harness);
    let login = mcp_list::login_command(harness, name);
    let mut lines = vec![
        format!(
            "{name} · {label} · {}",
            if item.action.is_empty() {
                item.detail.clone()
            } else {
                item.action.clone()
            }
        ),
        String::new(),
    ];

    match item.state.as_str() {
        "needs-auth" if login.is_some() => lines.extend([
            "l: sign in now (the browser opens), or in a terminal:".to_string(),
            format!("  {}", login.clone().unwrap_or_default().join(" ")),
        ]),
        "needs-auth" | "failed" | "pending" => lines.extend([
            format!("codehead cannot fix this for {label}. Open {label} and connect it there:"),
            if harness == "claude" {
                "  claude, then /mcp: pick the server, sign in or see its error".to_string()
            } else {
                format!("  {label}: its MCP settings")
            },
            "r: refresh this list after".to_string(),
        ]),
        _ => {
            lines.push("i: import into codehead (every harness gets it, secrets into the keychain).".into());
        }
    }

    Modal::Info {
        title: name.clone(),
        lines,
        login: login
            .filter(|_| item.state == "needs-auth")
            .map(|_| (harness.clone(), name.clone())),
    }
}
