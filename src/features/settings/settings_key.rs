use super::{Action, Form, KeyCode, KeyEvent, SUBS, Settings, Sub};

impl Form {
    pub(crate) fn key(&mut self, key: KeyEvent) -> Action {
        let (before, was_editing) = (super::field(self), self.editing);
        let action = super::key_helpers::key_inner(key, self);

        // an alias box unlocked: its name + target for an Esc
        if was_editing.is_none()
            && let Some(i) = self.editing
        {
            let at = self.plain() + super::alias_of(i, self) * 2;

            self.pair_entry = Some((at, self.fields[at].text(), self.fields[at + 1].text()));
        }

        if super::field(self) != before {
            self.focus_input();
        }

        action
    }
}

impl Settings {
    pub(crate) fn key(&mut self, key: KeyEvent) -> Action {
        key_settings(self, key)
    }
}

fn key_settings(settings: &mut Settings, key: KeyEvent) -> Action {
    if settings.bar {
        let at = SUBS.iter().position(|text| text.0 == settings.sub).unwrap_or(0);

        match key.code {
            KeyCode::Left | KeyCode::Char('h') => settings.switch(SUBS[(at + SUBS.len() - 1) % SUBS.len()].0),
            KeyCode::Right | KeyCode::Char('l') => settings.switch(SUBS[(at + 1) % SUBS.len()].0),
            KeyCode::Up => return Action::Header,
            KeyCode::Down | KeyCode::Enter | KeyCode::Char('j') | KeyCode::Esc => {
                settings.bar = false;
                settings.focus_input();
            }
            _ => {}
        }

        return Action::None;
    }

    let up = match settings.sub {
        Sub::Codehead => matches!(settings.form.key(key), Action::Header),
        Sub::Harnesses => matches!(settings.harness.key(key), Action::Header),
        Sub::Mcp => !settings.mcp.key(key),
        Sub::Shortcuts => !settings.shortcuts.key(key),
    };

    if up {
        settings.bar = true;
    }

    Action::None
}
