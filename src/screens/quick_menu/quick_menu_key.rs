use super::{Action, KeyCode, KeyEvent, Level, Menu, Tab};

impl Menu {
    pub(crate) fn key(&mut self, key: KeyEvent) -> Action {
        dispatch_key(self, key)
    }
}

fn dispatch_key(menu: &mut Menu, key: KeyEvent) -> Action {
    menu.poll();
    let ctrl = key.modifiers.contains(ratatui::crossterm::event::KeyModifiers::CONTROL);

    // the thread list's form / question / dropdown: every key is its own (esc, tab too)
    if menu.tab == Tab::Threads && menu.level != Level::Bar && menu.list.modal() {
        let out = menu.list.key(key);

        return menu.list_out(out);
    }

    // tab / shift+tab: the next / previous menu tab, from every level; a tab with a tab group
    // (threads: repos + views, merge: states): ←→ move in the group; the other tabs: ←→
    // switch them too; ctrl+←→: the filter's word jumps
    let group = menu.grouped();

    match key.code {
        // esc or the menu key again: closed
        KeyCode::Esc => return Action::Close,
        _ if crate::infra::config::menu_key().matches(&key) => return Action::Close,
        // tab / shift+tab: the next / previous region (the tab bar, a tab group, the list),
        // as on every screen; ←→ on the tab bar: the tabs
        KeyCode::Tab | KeyCode::BackTab if !ctrl => {
            menu.cycle(key.code == KeyCode::BackTab);
            return Action::None;
        }
        _ if menu.tab == Tab::Threads => return menu.threads_key(key),
        KeyCode::Right | KeyCode::Left if !ctrl && !group && menu.level == Level::Body => {
            menu.switch(if key.code == KeyCode::Right { 1 } else { -1 });
            return Action::None;
        }
        _ if menu.level != Level::Body || key.code == KeyCode::Up => {
            if let Some(action) = menu.level_key(key) {
                return action;
            }
        }
        _ => {}
    }

    match menu.tab {
        Tab::Quick => menu.quick_key(key),
        Tab::Threads => Action::None,
        Tab::Search => menu.search_key(key),
        Tab::Merge => match menu.prs.key(key) {
            crate::screens::merge::pr_list::Pick::Open(entry) => Action::Pr(entry),
            _ => Action::None,
        },
    }
}
