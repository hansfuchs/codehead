use super::{KeyCode, KeyEvent, KeyModifiers, View};

impl View {
    pub(super) fn key_history(&mut self, key: KeyEvent) -> Option<bool> {
        let empty = self.prompt.input.is_empty();

        match key.code {
            KeyCode::Up if self.shell_mode() && self.prompt.hist_at.is_none() && self.prompt.input.on_first_row() => {
                self.shell_history_step(true);
                return Some(false);
            }
            KeyCode::Down if self.shell_mode() && self.prompt.hist_at.is_none() && self.prompt.input.on_last_row() => {
                return Some(key_history_branch(self));
            }
            // `!` on an empty prompt: shell mode, a space after the `!`
            KeyCode::Char('!') if empty && !key.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) => {
                self.prompt.input.insert("! ");
                return Some(false);
            }
            // 1 backspace on the bare `! ` (or `!`): shell mode off, the prompt empty
            KeyCode::Backspace
                if matches!(self.prompt.input.text().as_str(), "! " | "!")
                    && !key
                        .modifiers
                        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER) =>
            {
                self.prompt.input.set_text("");
                self.shell_state.shell_at = None;
                return Some(false);
            }
            KeyCode::Up if self.prompt.input.on_first_row() && (empty || self.prompt.hist_at.is_some()) => {
                self.history_step(true);
                return Some(false);
            }
            KeyCode::Down if self.prompt.hist_at.is_some() && self.prompt.input.on_last_row() => {
                self.history_step(false);
                return Some(false);
            }

            _ => {}
        }

        None
    }
}

fn key_history_branch(view: &mut View) -> bool {
    if !view.shell_history_step(false) {
        view.edit_cwd();
    }

    false
}
