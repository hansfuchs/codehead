use super::{Item, Key, KeyCode, KeyEvent, KeyModifiers, McpTab, Modal, Then};

impl McpTab {
    /// A key. false: the tab did not use it (the parent's: Up at the top).
    pub(crate) fn key(&mut self, key: KeyEvent) -> bool {
        key_mcp_tab(self, key)
    }
}

fn confirm_server_delete(mcp_tab: &mut McpTab) {
    if let Some(Item {
        key: Key::Codehead(name),
        ..
    }) = mcp_tab.current()
    {
        mcp_tab.modal = Some(Modal::Confirm {
            text: format!("delete server {name}? its keychain secrets go too, its agents lose it"),
            then: Then::Delete(name),
        });
    }
}

fn filter_key(key: KeyEvent, mcp_tab: &mut McpTab) {
    mcp_tab.filter.key(key);
    mcp_tab.cursor = 0;
    mcp_tab.offset = 0;
}

fn key_mcp_tab(mcp_tab: &mut McpTab, key: KeyEvent) -> bool {
    if mcp_tab.modal.is_some() {
        super::key_helpers::modal_key(key, mcp_tab);
        return true;
    }

    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

    if mcp_tab.filtering {
        match key.code {
            KeyCode::Esc => {
                mcp_tab.filter.set_text("");
                mcp_tab.filtering = false;
            }
            KeyCode::Enter | KeyCode::Down | KeyCode::Tab => mcp_tab.filtering = false,
            KeyCode::Up => return false,
            _ => {
                filter_key(key, mcp_tab);
            }
        }

        return true;
    }

    let count = mcp_tab.items().0.len();

    match key.code {
        _ if crate::ui::textarea::is_filter_key(&key) => mcp_tab.filtering = true,
        KeyCode::Up if mcp_tab.cursor == 0 => return false,
        KeyCode::Up => mcp_tab.cursor -= 1,
        KeyCode::Down => mcp_tab.cursor = (mcp_tab.cursor + 1).min(count.saturating_sub(1)),
        KeyCode::PageUp => mcp_tab.cursor = mcp_tab.cursor.saturating_sub(10),
        KeyCode::PageDown => mcp_tab.cursor = (mcp_tab.cursor + 10).min(count.saturating_sub(1)),
        KeyCode::Home => mcp_tab.cursor = 0,
        KeyCode::End => mcp_tab.cursor = count.saturating_sub(1),
        KeyCode::Char('a') => super::open_add(mcp_tab),
        KeyCode::Char('r') if ctrl || key.modifiers.is_empty() => {
            mcp_tab.natives.clear();
            mcp_tab.enter();
            super::say("updating: harness lists, probe", false, mcp_tab);
        }
        KeyCode::Enter => super::open_current(mcp_tab),
        KeyCode::Char('l') => super::login_current(mcp_tab),
        KeyCode::Char('i') => super::import_current(mcp_tab),
        KeyCode::Char('g') => super::agents_current(mcp_tab),
        KeyCode::Char('t') => super::toggle_allow(mcp_tab),
        KeyCode::Char('d') | KeyCode::Delete => {
            confirm_server_delete(mcp_tab);
        }
        KeyCode::Esc if !mcp_tab.filter.text().is_empty() => mcp_tab.filter.set_text(""),
        _ => {}
    }

    true
}
