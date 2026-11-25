use super::{
    Agent, EditForm, Item, Key, McpTab, Modal, Msg, Pick, Server, Then, harness_label, harness_order, info_for,
    mcp_list, roster, servers,
};

pub(super) fn say(text: impl Into<String>, error: bool, mcp_tab: &mut McpTab) {
    mcp_tab.status = text.into();
    mcp_tab.error = error;
}

pub(super) fn probe(mcp_tab: &mut McpTab) {
    if mcp_tab.jobs.probing || mcp_tab.servers.is_empty() || cfg!(test) {
        return;
    }

    mcp_tab.jobs.probing = true;
    let list = mcp_tab.servers.clone();
    let sender = mcp_tab.jobs.tx.clone();

    std::thread::spawn(move || {
        mcp_list::probe_all(&list);
        let _ = sender.send(Msg::Probed);
    });
}

pub(super) fn open_current(mcp_tab: &mut McpTab) {
    let Some(item) = mcp_tab.current() else { return };

    match &item.key {
        Key::Codehead(name) => {
            open_current_key_codehead_name(mcp_tab, &item, name);
        }
        Key::Native { .. } => mcp_tab.modal = Some(self::info_for(&item, mcp_tab)),
    }
}

fn open_current_key_codehead_name(mcp_tab: &mut McpTab, item: &Item, name: &String) {
    if let Some(text_value) = mcp_tab.servers.iter().find(|text| &text.name == name).cloned() {
        if !item.action.is_empty() && !item.action.starts_with("plain text") {
            mcp_tab.modal = Some(self::info_for(item, mcp_tab));
        } else {
            mcp_tab.modal = Some(Modal::Edit(Box::new(EditForm::of(Some(&text_value)))));
        }
    }
}

pub(super) fn login_current(mcp_tab: &mut McpTab) {
    let Some(Item {
        key: Key::Native { harness, name },
        ..
    }) = mcp_tab.current()
    else {
        self::say("l: sign in works on a harness's own server", true, mcp_tab);
        return;
    };

    mcp_tab.login(harness, name);
}

pub(super) fn import_current(mcp_tab: &mut McpTab) {
    let Some(item) = mcp_tab.current() else { return };

    let Key::Native { harness, name } = item.key else {
        self::say("i: import works on a harness's own server", true, mcp_tab);
        return;
    };

    let source = mcp_tab
        .natives
        .iter()
        .find(|count| count.harness == harness && count.name == name)
        .map(|count_value| count_value.source.clone())
        .unwrap_or_default();

    if harness == "claude" && (source == "claudeai" || source == "plugin") {
        self::say("connectors and plugins stay with Claude Code", true, mcp_tab);
        return;
    }

    mcp_tab.modal = Some(Modal::Confirm {
        text: format!(
            "import {name} from {}? codehead runs it for every harness, its secrets go into the keychain, it leaves {}'s config",
            harness_label(&harness),
            harness_label(&harness)
        ),
        then: Then::Import { harness, name, source },
    });
}

pub(super) fn agents_current(mcp_tab: &mut McpTab) {
    if let Some(Item {
        key: Key::Codehead(name),
        ..
    }) = mcp_tab.current()
    {
        self::open_agents(&name, mcp_tab);
    } else {
        self::say("g: agents work on a codehead server", true, mcp_tab);
    }
}

pub(super) fn open_agents(name: &str, mcp_tab: &mut McpTab) {
    mcp_tab.agents = roster::active();
    let mut rows: Vec<(Agent, bool)> = mcp_tab
        .agents
        .iter()
        .map(|agent| (agent.clone(), agent.mcp.iter().any(|message| message == name)))
        .collect();

    rows.sort_by(|agent_value, bytes| {
        (harness_order(&agent_value.0.harness), &agent_value.0.name)
            .cmp(&(harness_order(&bytes.0.harness), &bytes.0.name))
    });

    mcp_tab.modal = Some(Modal::Agents(Pick {
        name: name.to_string(),
        rows,
        cursor: 0,
    }));
}

pub(super) fn toggle_allow(mcp_tab: &mut McpTab) {
    let Some(Item {
        key: Key::Codehead(name),
        ..
    }) = mcp_tab.current()
    else {
        return;
    };

    let mut all = mcp_tab.servers.clone();

    if let Some(text_value) = all.iter_mut().find(|text| text.name == name) {
        text_value.auto_allow = !text_value.auto_allow;
        let selected = text_value.auto_allow;

        match servers::save(&all) {
            Ok(()) => {
                mcp_tab.servers = all;
                self::say(
                    format!(
                        "{name}: {}",
                        if selected {
                            "auto-allow on: its tools run without an ask"
                        } else {
                            "auto-allow off: the agent's mode decides"
                        }
                    ),
                    false,
                    mcp_tab,
                );
            }
            Err(error) => self::say(error, true, mcp_tab),
        }
    }
}

pub(super) fn open_add(mcp_tab: &mut McpTab) {
    mcp_tab.modal = Some(Modal::Edit(Box::new(EditForm::of(None))));
}

/// The import of HARNESS's server NAME ended: R (the server, the toast) in the lists.
pub(super) fn imported(harness: &str, name: &str, result: Result<(Server, String), String>, mcp_tab: &mut McpTab) {
    match result {
        Ok((text, message)) => {
            mcp_tab.load();
            mcp_tab.natives.retain(|x| !(x.harness == harness && x.name == name));
            self::say(message, false, mcp_tab);
            self::probe(mcp_tab);
            self::open_agents(&text.name, mcp_tab);
        }
        Err(error) => self::say(error, true, mcp_tab),
    }
}

pub(super) fn save_agents(selection: &Pick, mcp_tab: &mut McpTab) {
    let mut count = 0;

    for (agent, selected) in &selection.rows {
        let Some(mut cur) = roster::get(&agent.name) else {
            continue;
        };

        let has = cur.mcp.contains(&selection.name);

        if has == *selected {
            count += usize::from(*selected);
            continue;
        }

        if *selected {
            cur.mcp.push(selection.name.clone());
            count += 1;
        } else {
            cur.mcp.retain(|message| *message != selection.name);
        }

        if let Err(error) = roster::save(&cur) {
            return self::say(error, true, mcp_tab);
        }
    }

    mcp_tab.agents = roster::active();
    self::say(
        format!(
            "{}: {count} agent{} · they start it at their next message",
            selection.name,
            if count == 1 { "" } else { "s" }
        ),
        false,
        mcp_tab,
    );
}
