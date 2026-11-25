use super::{Action, KeyCode, KeyEvent, Level, Menu, Tab};

impl Menu {
    pub(crate) fn key(&mut self, key_value: KeyEvent) -> Action {
        dispatch_key(self, key_value)
    }
}

fn dispatch_key(widget_state: &mut Menu, key_value: KeyEvent) -> Action {
    widget_state.poll();
    let ctrl = key_value
        .modifiers
        .contains(ratatui::crossterm::event::KeyModifiers::CONTROL);

    // the thread list's form / question / dropdown: every key is its own (esc, tab too)
    if widget_state.tab == Tab::Threads && widget_state.level != Level::Bar && widget_state.list.modal() {
        let out = widget_state.list.key(key_value);

        return widget_state.list_out(out);
    }

    // tab / shift+tab: the next / previous menu tab, from every level; a tab with a tab group
    // (threads: repos + views, merge: states): ←→ move in the group; the other tabs: ←→
    // switch them too; ctrl+←→: the filter's word jumps
    let group = widget_state.grouped();

    match key_value.code {
        // esc or the menu key again: closed
        KeyCode::Esc => return Action::Close,
        _ if crate::infra::config::menu_key().matches(&key_value) => return Action::Close,
        // tab / shift+tab: the next / previous region (the tab bar, a tab group, the list),
        // as on every screen; ←→ on the tab bar: the tabs
        KeyCode::Tab | KeyCode::BackTab if !ctrl => {
            widget_state.cycle(key_value.code == KeyCode::BackTab);
            return Action::None;
        }
        _ if widget_state.tab == Tab::Threads => return widget_state.threads_key(key_value),
        KeyCode::Right | KeyCode::Left if !ctrl && !group && widget_state.level == Level::Body => {
            widget_state.switch(if key_value.code == KeyCode::Right { 1 } else { -1 });
            return Action::None;
        }
        _ if widget_state.level != Level::Body || key_value.code == KeyCode::Up => {
            if let Some(agent) = widget_state.level_key(key_value) {
                return agent;
            }
        }
        _ => {}
    }

    match widget_state.tab {
        Tab::Quick => widget_state.quick_key(key_value),
        Tab::Threads => Action::None,
        Tab::Search => widget_state.search_key(key_value),
        Tab::Merge => match widget_state.prs.key(key_value) {
            crate::screens::merge::pr_list::Pick::Open(error) => Action::Pr(error),
            _ => Action::None,
        },
        Tab::Tokens => {
            match key_value.code {
                KeyCode::Up | KeyCode::Char('k') => {
                    widget_state.tokens.dscroll = widget_state.tokens.dscroll.saturating_sub(1);
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    widget_state.tokens.dscroll = widget_state.tokens.dscroll.saturating_add(1);
                }
                KeyCode::PageUp => widget_state.tokens.dscroll = widget_state.tokens.dscroll.saturating_sub(10),
                KeyCode::PageDown => widget_state.tokens.dscroll = widget_state.tokens.dscroll.saturating_add(10),
                KeyCode::Char('r') => widget_state.tokens.detail_at = None,
                _ => {}
            }

            Action::None
        }
    }
}
