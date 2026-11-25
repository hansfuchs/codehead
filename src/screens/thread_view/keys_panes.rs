use super::{KeyCode, KeyEvent, KeyModifiers, Pane, View};

impl View {
    pub(super) fn key_pane_switch(&mut self, key: KeyEvent) -> Option<bool> {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);

        if alt && !ctrl && key.code == KeyCode::Char('\\') {
            toggle_split(self);
            return Some(false);
        }

        let pane = match key.code {
            KeyCode::F(number @ 1..=4) if key.modifiers.is_empty() => Some(usize::from(number) - 1),
            KeyCode::Char(character @ '1'..='4') if alt != ctrl => Some(usize::from(character as u8 - b'1')),
            _ => None,
        };

        if let Some(pane) = pane.and_then(|index| Pane::ALL.get(index)) {
            self.set_pane(*pane);
            return Some(false);
        }

        if self.panes.pane == Pane::Files
            && !self.panes.prompt_focus
            && self.panes.files.typing()
            && !(ctrl && key.code == KeyCode::Char('c'))
        {
            let action = self.panes.files.key(key);

            self.files_act(action);
            return Some(false);
        }

        None
    }

    pub(super) fn key_pane_focus(&mut self, key: KeyEvent) -> Option<bool> {
        if self.panes.pane == Pane::Chat || self.panes.prompt_focus {
            return None;
        }

        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

        if self.panes.pane == Pane::Changes
            && self.checkout_state.repo_focus
            && !(ctrl && key.code == KeyCode::Char('c'))
        {
            key_repo(self, key);
            return Some(false);
        }

        let esc_here = key.code == KeyCode::Esc
            && ((self.panes.pane == Pane::Changes && self.panes.changes.takes_esc())
                || (self.panes.pane == Pane::Merge && self.merge_state.merge.takes_esc()));

        if (key.code == KeyCode::Esc && !esc_here) || (ctrl && key.code == KeyCode::Char('c')) {
            return None;
        }

        key_focused_pane(self, key);

        Some(false)
    }
}

fn checkout_step(key: KeyEvent) -> isize {
    if matches!(key.code, KeyCode::Right | KeyCode::Char('l')) {
        1
    } else {
        -1
    }
}

fn toggle_split(view: &mut View) {
    view.layout.split = !view.layout.split;
    view.stream.dirty = true;
    view.toast(if view.layout.split {
        "changes: the chat on the right (≥ 160 columns)"
    } else {
        "changes: full width"
    });
}

fn key_repo(view: &mut View, key: KeyEvent) {
    let modified = key.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT);

    match key.code {
        KeyCode::Left | KeyCode::Right | KeyCode::Char('h' | 'l') if !modified => {
            view.step_checkout(checkout_step(key));
        }
        KeyCode::Down | KeyCode::Enter | KeyCode::Esc => view.checkout_state.repo_focus = false,
        _ => {}
    }

    view.stream.dirty = true;
}

fn key_focused_pane(view: &mut View, key: KeyEvent) {
    if view.panes.pane == Pane::Merge {
        let action = view.merge_state.merge.key(key);

        view.merge_act(action);
        return;
    }

    if view.panes.pane == Pane::Changes
        && key.code == KeyCode::Up
        && key.modifiers.is_empty()
        && view.checkout_state.checkouts.len() > 1
        && view.panes.changes.at_top()
    {
        view.checkout_state.repo_focus = true;
        view.stream.dirty = true;
        return;
    }

    if view.panes.pane == Pane::Files {
        let action = view.panes.files.key(key);

        view.files_act(action);
    } else {
        let action = view.panes.changes.key(key);

        view.changes_act(action);
    }
}
