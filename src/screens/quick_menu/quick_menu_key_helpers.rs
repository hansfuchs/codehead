use super::{Action, Home, KeyCode, KeyEvent, Level, Menu, Tab, thread_list};

impl Menu {
    pub(super) fn search_key(&mut self, key: KeyEvent) -> Action {
        let count = self.search_state.hits.len();

        match key.code {
            KeyCode::Up => self.search_state.scursor = self.search_state.scursor.saturating_sub(1),
            KeyCode::Down if count > 0 => self.search_state.scursor = (self.search_state.scursor + 1).min(count - 1),
            KeyCode::PageUp => self.search_state.scursor = self.search_state.scursor.saturating_sub(5),
            KeyCode::PageDown if count > 0 => {
                self.search_state.scursor = (self.search_state.scursor + 5).min(count - 1);
            }
            KeyCode::Enter => return self.jump(self.search_state.scursor),
            _ => {
                self.search_state.search.key(key);
                self.refresh_hits();
            }
        }

        Action::None
    }

    /// Keys on the tab bar / the tab group. Some: the key is done.
    pub(super) fn level_key(&mut self, key: KeyEvent) -> Option<Action> {
        dispatch_region_key(self, key)
    }

    /// Keys of the threads tab: the tab bar's (Bar), else the thread list's.
    pub(super) fn threads_key(&mut self, key: KeyEvent) -> Action {
        dispatch_threads_key(self, key)
    }

    pub(super) fn quick_key(&mut self, key: KeyEvent) -> Action {
        let count = self.items.len();

        match key.code {
            KeyCode::Up | KeyCode::Char('k') => self.qcursor = self.qcursor.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') => self.qcursor = (self.qcursor + 1).min(count - 1),
            KeyCode::Char('n') => return Action::Home(Home::New),
            KeyCode::Char('h') => return Action::Back,
            KeyCode::Char('f') => {
                self.tab = Tab::Search;
                self.level = Level::Body;
            }
            KeyCode::Enter => return self.run_item(self.qcursor),
            _ => {}
        }

        Action::None
    }
}

fn dispatch_region_key(menu: &mut Menu, key: KeyEvent) -> Option<Action> {
    let up = key.code == KeyCode::Up;

    match menu.level {
        Level::Body if up && menu.at_top() => {
            menu.level = if menu.grouped() { Level::Group } else { Level::Bar };

            Some(Action::None)
        }
        Level::Body => None,
        Level::Bar => dispatch_bar_key(menu, key),
        Level::Group => {
            match key.code {
                KeyCode::Up => menu.level = Level::Bar,
                KeyCode::Down | KeyCode::Enter => menu.level = Level::Body,
                KeyCode::Left | KeyCode::Right => {
                    menu.prs.key(key);
                }
                _ => {
                    menu.level = Level::Body;
                    return None;
                }
            }

            Some(Action::None)
        }
    }
}

fn dispatch_bar_key(menu: &mut Menu, key: KeyEvent) -> Option<Action> {
    match key.code {
        KeyCode::Left | KeyCode::Right => {
            menu.switch(if key.code == KeyCode::Right { 1 } else { -1 });
        }
        KeyCode::Down | KeyCode::Enter => menu.level = if menu.grouped() { Level::Group } else { Level::Body },
        KeyCode::Up => {}
        // a letter: the list (its filter) takes it
        _ => {
            menu.level = Level::Body;
            return None;
        }
    }

    Some(Action::None)
}

fn dispatch_threads_key(menu: &mut Menu, key: KeyEvent) -> Action {
    if menu.level == Level::Bar {
        return threads_bar_key(menu, key);
    }

    let out = menu.list.key(key);

    menu.list_out(out)
}

fn threads_bar_key(menu: &mut Menu, key: KeyEvent) -> Action {
    match key.code {
        KeyCode::Left | KeyCode::Right => {
            menu.switch(if key.code == KeyCode::Right { 1 } else { -1 });
        }
        KeyCode::Up => {}
        KeyCode::Down | KeyCode::Enter => {
            menu.level = Level::Body;
            menu.list.focus = thread_list::Focus::Repos;
        }
        // a letter: the list's filter takes it
        _ => {
            menu.level = Level::Body;
            menu.list.focus = thread_list::Focus::List;
            let out = menu.list.key(key);

            return menu.list_out(out);
        }
    }

    Action::None
}
