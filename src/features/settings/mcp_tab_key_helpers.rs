use super::{EditForm, KeyCode, KeyEvent, KeyModifiers, McpTab, Modal, Server};

impl McpTab {}

pub(super) fn modal_key(key: KeyEvent, mcp_tab: &mut McpTab) {
    let Some(modal) = mcp_tab.modal.take() else {
        return;
    };

    mcp_tab.modal = match modal {
        Modal::Info { title, lines, login } => match key.code {
            KeyCode::Char('l') if login.is_some() => {
                let (harness, name) = login.unwrap_or_default();

                mcp_tab.login(harness, name);
                None
            }
            KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q') => None,
            _ => Some(Modal::Info { title, lines, login }),
        },
        Modal::Confirm { text, then } => match key.code {
            KeyCode::Char('y') | KeyCode::Enter => {
                mcp_tab.run(then);
                mcp_tab.modal.take()
            }
            KeyCode::Esc | KeyCode::Char('n') => None,
            _ => Some(Modal::Confirm { text, then }),
        },
        Modal::Agents(mut pick) => match key.code {
            KeyCode::Esc => None,
            KeyCode::Up => {
                pick.cursor = pick.cursor.saturating_sub(1);
                Some(Modal::Agents(pick))
            }
            KeyCode::Down => {
                pick.cursor = (pick.cursor + 1).min(pick.rows.len().saturating_sub(1));
                Some(Modal::Agents(pick))
            }
            KeyCode::Char(' ') => {
                if let Some(row) = pick.rows.get_mut(pick.cursor) {
                    row.1 = !row.1;
                }

                Some(Modal::Agents(pick))
            }
            // every agent of the harness of the cursor row: on, or off when all are on
            KeyCode::Char('h') => {
                if let Some(harness) = pick.rows.get(pick.cursor).map(|row| row.0.harness.clone()) {
                    let all = pick.rows.iter().filter(|row| row.0.harness == harness).all(|row| row.1);

                    toggle_harness_agents(&mut pick, all, &harness);
                }

                Some(Modal::Agents(pick))
            }
            KeyCode::Enter => {
                super::save_agents(&pick, mcp_tab);
                None
            }
            _ => Some(Modal::Agents(pick)),
        },
        Modal::Edit(form) => edit_server_key(key, mcp_tab, form),
    };
}

fn toggle_harness_agents(selection: &mut super::Pick, all: bool, harness: &str) {
    for row in selection.rows.iter_mut().filter(|row| row.0.harness == harness) {
        row.1 = !all;
    }
}

fn edit_server_key(key: KeyEvent, mcp_tab: &mut McpTab, mut form: Box<EditForm>) -> Option<Modal> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

    match key.code {
        // esc: closes; edits only on a 2nd esc (esc never drops edits unasked)
        KeyCode::Esc => {
            let dirty = form.name.edited() || form.target.edited() || form.secrets.edited();

            if crate::ui::textarea::esc_drops(&mut mcp_tab.esc_at, dirty) {
                None
            } else {
                super::say("esc again: drop the edits · ctrl+s: save", false, mcp_tab);
                Some(Modal::Edit(form))
            }
        }
        KeyCode::Char('s') if ctrl => super::save_edit(form, mcp_tab),
        KeyCode::Enter if form.field == 4 => super::save_edit(form, mcp_tab),
        KeyCode::Down if !(form.field == 3 && key.code == KeyCode::Down && !super::secrets_at_end(&form)) => {
            form.field = (form.field + 1).min(4);
            Some(Modal::Edit(form))
        }
        KeyCode::Up if !(form.field == 3 && key.code == KeyCode::Up && !super::secrets_at_start(&form)) => {
            form.field = form.field.saturating_sub(1);
            Some(Modal::Edit(form))
        }
        KeyCode::Left | KeyCode::Right if form.field == 1 => {
            form.kind = if key.code == KeyCode::Right {
                (form.kind + 1) % 3
            } else {
                (form.kind + 2) % 3
            };

            form.target.placeholder = EditForm::target_hint(form.kind).into();
            Some(Modal::Edit(form))
        }
        KeyCode::Char(' ') if form.field == 4 => {
            form.auto_allow = !form.auto_allow;
            Some(Modal::Edit(form))
        }
        KeyCode::Enter if form.field < 3 => {
            form.field += 1;
            Some(Modal::Edit(form))
        }
        _ => {
            match form.field {
                0 => {
                    form.name.key(key);
                }
                2 => {
                    form.target.key(key);
                }
                3 => {
                    form.secrets.key(key);
                }
                _ => {}
            }

            Some(Modal::Edit(form))
        }
    }
}

/// The save of form E ended with R (the server, every server): the lists, the agents picker
/// for a new server; an error: the form again.
pub(super) fn edit_saved(
    form: Box<EditForm>,
    result: Result<(Server, Vec<Server>), String>,
    mcp_tab: &mut McpTab,
) -> Option<Modal> {
    let (server, all) = match result {
        Ok(value) => value,
        Err(why) => {
            super::say(why, true, mcp_tab);
            return Some(Modal::Edit(form));
        }
    };

    mcp_tab.servers = all;
    super::say(
        format!("{}: saved · agents start it at their next message", server.name),
        false,
        mcp_tab,
    );
    super::probe(mcp_tab);

    if form.old.is_none() {
        super::open_agents(&server.name, mcp_tab);
        return mcp_tab.modal.take();
    }

    None
}
