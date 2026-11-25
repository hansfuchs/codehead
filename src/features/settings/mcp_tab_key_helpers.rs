use super::{EditForm, KeyCode, KeyEvent, KeyModifiers, McpTab, Modal, Server};

impl McpTab {}

pub(super) fn modal_key(key_value: KeyEvent, mcp_tab: &mut McpTab) {
    let Some(message) = mcp_tab.modal.take() else {
        return;
    };

    mcp_tab.modal = match message {
        Modal::Info { title, lines, login } => match key_value.code {
            KeyCode::Char('l') if login.is_some() => {
                let (height, count) = login.unwrap_or_default();

                mcp_tab.login(height, count);
                None
            }
            KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q') => None,
            _ => Some(Modal::Info { title, lines, login }),
        },
        Modal::Confirm { text, then } => match key_value.code {
            KeyCode::Char('y') | KeyCode::Enter => {
                mcp_tab.run(then);
                mcp_tab.modal.take()
            }
            KeyCode::Esc | KeyCode::Char('n') => None,
            _ => Some(Modal::Confirm { text, then }),
        },
        Modal::Agents(mut path) => match key_value.code {
            KeyCode::Esc => None,
            KeyCode::Up => {
                path.cursor = path.cursor.saturating_sub(1);
                Some(Modal::Agents(path))
            }
            KeyCode::Down => {
                path.cursor = (path.cursor + 1).min(path.rows.len().saturating_sub(1));
                Some(Modal::Agents(path))
            }
            KeyCode::Char(' ') => {
                if let Some(row) = path.rows.get_mut(path.cursor) {
                    row.1 = !row.1;
                }

                Some(Modal::Agents(path))
            }
            // every agent of the harness of the cursor row: on, or off when all are on
            KeyCode::Char('h') => {
                if let Some(harness_value) = path.rows.get(path.cursor).map(|row_value| row_value.0.harness.clone()) {
                    let all = path
                        .rows
                        .iter()
                        .filter(|row_2| row_2.0.harness == harness_value)
                        .all(|result_2| result_2.1);

                    toggle_harness_agents(&mut path, all, &harness_value);
                }

                Some(Modal::Agents(path))
            }
            KeyCode::Enter => {
                super::save_agents(&path, mcp_tab);
                None
            }
            _ => Some(Modal::Agents(path)),
        },
        Modal::Edit(error) => edit_server_key(key_value, mcp_tab, error),
    };
}

fn toggle_harness_agents(selection: &mut super::Pick, all: bool, harness_value: &str) {
    for row_2 in selection.rows.iter_mut().filter(|row| row.0.harness == harness_value) {
        row_2.1 = !all;
    }
}

fn edit_server_key(key_value: KeyEvent, mcp_tab: &mut McpTab, mut error: Box<EditForm>) -> Option<Modal> {
    let ctrl = key_value.modifiers.contains(KeyModifiers::CONTROL);

    match key_value.code {
        // esc: closes; edits only on a 2nd esc (esc never drops edits unasked)
        KeyCode::Esc => {
            let dirty = error.name.edited() || error.target.edited() || error.secrets.edited();

            if crate::ui::textarea::esc_drops(&mut mcp_tab.esc_at, dirty) {
                None
            } else {
                super::say("esc again: drop the edits · ctrl+s: save", false, mcp_tab);
                Some(Modal::Edit(error))
            }
        }
        KeyCode::Char('s') if ctrl => super::save_edit(error, mcp_tab),
        KeyCode::Enter if error.field == 4 => super::save_edit(error, mcp_tab),
        KeyCode::Down if !(error.field == 3 && key_value.code == KeyCode::Down && !super::secrets_at_end(&error)) => {
            error.field = (error.field + 1).min(4);
            Some(Modal::Edit(error))
        }
        KeyCode::Up if !(error.field == 3 && key_value.code == KeyCode::Up && !super::secrets_at_start(&error)) => {
            error.field = error.field.saturating_sub(1);
            Some(Modal::Edit(error))
        }
        KeyCode::Left | KeyCode::Right if error.field == 1 => {
            error.kind = if key_value.code == KeyCode::Right {
                (error.kind + 1) % 3
            } else {
                (error.kind + 2) % 3
            };

            error.target.placeholder = EditForm::target_hint(error.kind).into();
            Some(Modal::Edit(error))
        }
        KeyCode::Char(' ') if error.field == 4 => {
            error.auto_allow = !error.auto_allow;
            Some(Modal::Edit(error))
        }
        KeyCode::Enter if error.field < 3 => {
            error.field += 1;
            Some(Modal::Edit(error))
        }
        _ => {
            match error.field {
                0 => {
                    error.name.key(key_value);
                }
                2 => {
                    error.target.key(key_value);
                }
                3 => {
                    error.secrets.key(key_value);
                }
                _ => {}
            }

            Some(Modal::Edit(error))
        }
    }
}

/// The save of form E ended with R (the server, every server): the lists, the agents picker
/// for a new server; an error: the form again.
pub(super) fn edit_saved(
    error: Box<EditForm>,
    result: Result<(Server, Vec<Server>), String>,
    mcp_tab: &mut McpTab,
) -> Option<Modal> {
    let (text, all) = match result {
        Ok(value) => value,
        Err(why) => {
            super::say(why, true, mcp_tab);
            return Some(Modal::Edit(error));
        }
    };

    mcp_tab.servers = all;
    super::say(
        format!("{}: saved · agents start it at their next message", text.name),
        false,
        mcp_tab,
    );
    super::probe(mcp_tab);

    if error.old.is_none() {
        super::open_agents(&text.name, mcp_tab);
        return mcp_tab.modal.take();
    }

    None
}
