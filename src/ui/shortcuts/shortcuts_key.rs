use super::{Focus, KeyCode, KeyEvent, KeyModifiers, Pick, Tab, TextArea, all, valid_name};

impl Pick {
    /// Key for the dropdown. true: used (the input must not see it).
    pub(crate) fn key(&mut self, key_value: KeyEvent, ta: &mut TextArea) -> bool {
        pick_key(self, key_value, ta)
    }
}

impl Tab {
    /// A key. False: ↑ on the 1st row (the sub-tab bar takes the keys).
    pub(crate) fn key(&mut self, key_value: KeyEvent) -> bool {
        match self.focus {
            Focus::List => key_list(self, key_value),
            Focus::Name | Focus::Text if key_value.code == KeyCode::Esc => key_escape(self),
            Focus::Name => key_name(self, key_value),
            Focus::Text => key_text(self, key_value),
        }
    }
}

fn pick_key(widget_state: &mut Pick, key_value: KeyEvent, ta: &mut TextArea) -> bool {
    let Some(output) = widget_state.open(ta) else {
        return false;
    };

    let plain = key_value.modifiers.is_empty() || key_value.modifiers == KeyModifiers::SHIFT;
    let count = output.matches.len();

    match key_value.code {
        KeyCode::Up | KeyCode::Down if plain => {
            widget_state.cursor = if key_value.code == KeyCode::Up {
                (widget_state.cursor + count - 1) % count
            } else {
                (widget_state.cursor + 1) % count
            };

            widget_state.hover = None;
            let name = output.list[output.matches[widget_state.cursor]].name.clone();

            widget_state
                .preview
                .show(ta, output.at, &format!("#{}", output.query), &format!("#{name}"));
        }
        KeyCode::Tab | KeyCode::Enter if key_value.modifiers.is_empty() => {
            widget_state.pick(ta, &output, widget_state.cursor);
        }
        KeyCode::Esc if key_value.modifiers.is_empty() => widget_state.closed = Some(output.at),
        KeyCode::Char(character)
            if plain
                && (character.is_alphanumeric() || character == '_' || character == '-')
                && widget_state.preview.revert(ta) =>
        {
            return false;
        }
        _ => return false,
    }

    true
}

fn key_list(tab: &mut Tab, key_value: KeyEvent) -> bool {
    let ctrl = key_value.modifiers.contains(KeyModifiers::CONTROL);
    let count = tab.list.len() + 1;

    if !matches!(key_value.code, KeyCode::Char('d') | KeyCode::Delete) {
        tab.armed = false;
    }

    match key_value.code {
        KeyCode::Up | KeyCode::Char('k') if tab.at == 0 => return false,
        KeyCode::Up | KeyCode::Char('k') => {
            tab.at -= 1;
            tab.load();
        }
        KeyCode::Down | KeyCode::Char('j') => {
            tab.at = (tab.at + 1).min(count - 1);
            tab.load();
        }
        KeyCode::Enter | KeyCode::Right | KeyCode::Char('l') if tab.at == tab.list.len() => tab.add(),
        KeyCode::Enter | KeyCode::Right | KeyCode::Char('l') => {
            tab.text.move_end();
            tab.focus = Focus::Text;
        }
        KeyCode::Char('n') if ctrl || key_value.modifiers.is_empty() => tab.add(),
        // R: rename (as in the flows tab; r reads again on every screen)
        KeyCode::Char('R') if tab.at < tab.list.len() => {
            tab.name.move_end();
            tab.focus = Focus::Name;
        }
        KeyCode::Char('d') | KeyCode::Delete if tab.at < tab.list.len() => {
            if tab.armed {
                tab.armed = false;
                tab.delete();
            } else {
                tab.armed = true;
            }
        }
        _ => {}
    }

    true
}

fn key_escape(tab: &mut Tab) -> bool {
    let dirty = tab.name.edited() || tab.text.edited();

    if !crate::ui::textarea::esc_drops(&mut tab.esc_at, dirty) {
        tab.error = Some("esc again: drop the edits · ctrl+s: save".into());
        return true;
    }

    tab.error = None;

    // a new shortcut never saved: it goes
    if tab.list.get(tab.at).is_some_and(|text_value| {
        text_value.text.is_empty() && !all().iter().any(|agent| agent.name == text_value.name)
    }) {
        tab.list.remove(tab.at);
        tab.at = tab.at.min(tab.list.len());
    }

    tab.load();
    tab.focus = Focus::List;

    true
}

fn key_name(tab: &mut Tab, key_value: KeyEvent) -> bool {
    match key_value.code {
        KeyCode::Enter | KeyCode::Down => {
            let name = tab.name.text().trim().to_string();

            if !valid_name(&name) {
                tab.error = Some("name: letters, digits, _ or - only".into());
            } else if tab
                .list
                .iter()
                .enumerate()
                .any(|(i, text_2)| i != tab.at && text_2.name == name)
            {
                tab.error = Some(format!("#{name} exists already"));
            } else {
                tab.error = None;
                tab.text.move_end();
                tab.focus = Focus::Text;
            }
        }
        _ => {
            tab.name.key(key_value);
        }
    }

    true
}

fn key_text(tab: &mut Tab, key_value: KeyEvent) -> bool {
    let ctrl = key_value.modifiers.contains(KeyModifiers::CONTROL);

    match key_value.code {
        // ctrl+s / ctrl+enter: saved, back to the list
        KeyCode::Char('s') if ctrl => {
            if tab.store() {
                tab.focus = Focus::List;
            }
        }
        KeyCode::Enter
            if key_value
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER) =>
        {
            if tab.store() {
                tab.focus = Focus::List;
            }
        }
        KeyCode::Up if tab.text.on_first_row() => tab.focus = Focus::Name,
        _ => {
            if !matches!(tab.text.key(key_value), crate::ui::textarea::Outcome::Handled)
                && key_value.code == KeyCode::Enter
            {
                tab.text.newline();
            }
        }
    }

    true
}
