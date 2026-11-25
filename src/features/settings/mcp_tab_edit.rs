use super::{
    BTreeMap, EditForm, KINDS, McpTab, Modal, Msg, Server, harness_label, harnesses, key_helpers, mcp_list, roster,
    say, servers,
};

/// The env / header values of S in the keychain: plain values move in (OLD's entries a new value
/// replaces go). 0 keychain: Err, the owner writes `${VAR}` instead.
/// Import HARNESS's server NAME (from SOURCE) into codehead's SERVERS: saved, secrets in the
/// keychain, gone from the harness. (the server, the toast).
pub(super) fn import(
    harness: &str,
    name: &str,
    source: &str,
    servers: Vec<Server>,
) -> Result<(Server, String), String> {
    let mut server = mcp_list::import(harness, name)?;

    // a name codehead cannot use as a server name: its safe form
    if !servers::valid_name(&server.name) {
        server.name = server.name.chars().map(server_name_character).collect();
    }

    if servers.iter().any(|x| x.name == server.name) {
        return Err(format!("{}: codehead has a server with this name", server.name));
    }

    secure(&mut server, None)?;
    let mut all = servers;

    all.push(server.clone());
    servers::save(&all)?;

    let message = match mcp_list::remove(harness, name, source) {
        Ok(()) => format!("{}: imported, gone from {}", server.name, harness_label(harness)),
        Err(error) => format!(
            "{}: imported; {} still has it ({error})",
            server.name,
            harness_label(harness)
        ),
    };

    Ok((server, message))
}

fn server_name_character(character: char) -> char {
    if character.is_ascii_alphanumeric() || character == '-' || character == '_' {
        character
    } else {
        '-'
    }
}

/// Server S of the edit form (OLD: before the edit, named `OLD_NAME`) into SERVERS: its secrets
/// into the keychain, the file saved, a rename's agents follow. (the server, every server).
pub(super) fn commit_edit(
    mut server: Server,
    old: Option<&Server>,
    old_name: Option<&String>,
    servers: Vec<Server>,
) -> Result<(Server, Vec<Server>), String> {
    secure(&mut server, old)?;
    let mut all: Vec<Server> = servers.into_iter().filter(|x| Some(&x.name) != old_name).collect();

    all.push(server.clone());
    all.sort_by(|agent, bytes| agent.name.cmp(&bytes.name));
    servers::save(&all)?;

    // a rename: the agents follow
    if let Some(output) = &old_name
        && **output != server.name
    {
        for mut agent_4 in roster::active()
            .into_iter()
            .filter(|agent_3| agent_3.mcp.contains(output))
        {
            rename_agent_server(&server, &mut agent_4, output);

            let _ = roster::save(&agent_4);
        }
    }

    Ok((server, all))
}

fn rename_agent_server(server: &Server, agent: &mut roster::Agent, old_name: &String) {
    for name_2 in agent.mcp.iter_mut().filter(|name_value| **name_value == *old_name) {
        name_2.clone_from(&server.name);
    }
}

pub(super) fn secure(server: &mut Server, old: Option<&Server>) -> Result<(), String> {
    let name = server.name.clone();

    let olds: Vec<String> = old
        .map(|server_value| {
            server_value
                .env
                .values()
                .chain(server_value.headers.values())
                .cloned()
                .collect()
        })
        .unwrap_or_default();

    for map in [&mut server.env, &mut server.headers] {
        store_secrets(&name, map)?;
    }

    let now: Vec<&String> = server.env.values().chain(server.headers.values()).collect();

    for reference_value in olds.iter().filter(|reference| !now.contains(reference)) {
        servers::forget_secret(reference_value);
    }

    Ok(())
}

fn store_secrets(name: &str, map: &mut BTreeMap<String, String>) -> Result<(), String> {
    for (key, value) in map.iter_mut() {
        if servers::is_ref(value) {
            continue;
        }

        *value = servers::store_secret(name, key, value)
            .map_err(|error| format!("{error} · write {key}=${{VAR}} and set VAR in codehead's environment"))?;
    }

    Ok(())
}

/// The edit form saved: the server written (secrets into the keychain), then the agents
/// picker for a new server. Err: the form stays, the error shows.
pub(super) fn save_edit(edit_form: Box<EditForm>, mcp_tab: &mut McpTab) -> Option<Modal> {
    let old = edit_form
        .old
        .as_ref()
        .and_then(|count| mcp_tab.servers.iter().find(|text_value| &text_value.name == count))
        .cloned();

    let server_2 = match self::server(&edit_form) {
        Ok(server_value) => server_value,
        Err(why) => {
            self::say(why, true, mcp_tab);
            return Some(Modal::Edit(edit_form));
        }
    };

    if let Err(why) = server_2.check() {
        self::say(why, true, mcp_tab);
        return Some(Modal::Edit(edit_form));
    }

    if edit_form.old.as_deref() != Some(server_2.name.as_str())
        && mcp_tab.servers.iter().any(|x| x.name == server_2.name)
    {
        self::say(
            format!("{}: codehead has a server with this name", server_2.name),
            true,
            mcp_tab,
        );
        return Some(Modal::Edit(edit_form));
    }

    // the keychain (tens of ms per secret, a prompt maybe), the file, the agents: off the UI
    // thread (tests: at once); the form comes back on an error
    let servers = mcp_tab.servers.clone();
    let old_name = edit_form.old.clone();
    let run = move || commit_edit(server_2, old.as_ref(), old_name.as_ref(), servers);

    if cfg!(test) {
        let result = run();

        return key_helpers::edit_saved(edit_form, result, mcp_tab);
    }

    let name = edit_form.name.text().trim().to_string();

    mcp_tab.jobs.busy.push(name.clone());
    self::say(format!("saving {name}…"), false, mcp_tab);
    mcp_tab.saving = Some(edit_form);
    let sender = mcp_tab.jobs.tx.clone();

    std::thread::spawn(move || {
        let _ = sender.send(Msg::Saved(run()));
    });

    None
}

pub(super) fn secrets_at_end(edit_form: &EditForm) -> bool {
    let (row, _) = edit_form.secrets.cursor();

    row + 1 >= edit_form.secrets.text().lines().count().max(1)
}

pub(super) fn secrets_at_start(edit_form: &EditForm) -> bool {
    edit_form.secrets.cursor().0 == 0
}

/// The form as a server (secrets as typed: `secure` moves them into the keychain).
pub(super) fn server(edit_form: &EditForm) -> Result<Server, String> {
    let kind = KINDS[edit_form.kind].to_string();
    let target = edit_form.target.text().trim().to_string();
    let mut map = BTreeMap::new();

    for line_value in edit_form
        .secrets
        .text()
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        let (key, value) = line_value
            .split_once('=')
            .ok_or_else(|| format!("{line_value:?}: KEY=value"))?;

        map.insert(key.trim().to_string(), value.trim().to_string());
    }

    let mut text_value = Server {
        name: edit_form.name.text().trim().to_string(),
        kind,
        auto_allow: edit_form.auto_allow,
        ..Default::default()
    };

    if text_value.kind == "stdio" {
        let mut parts = harnesses::split(&target).into_iter();

        text_value.command = parts.next();
        text_value.args = parts.collect();
        text_value.env = map;
    } else {
        text_value.url = (!target.is_empty()).then_some(target);
        text_value.headers = map;
    }

    Ok(text_value)
}
