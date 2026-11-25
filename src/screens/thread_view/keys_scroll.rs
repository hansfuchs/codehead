use super::{KeyCode, KeyEvent, KeyModifiers, View};

impl View {
    pub(super) fn key_scroll(&mut self, key: KeyEvent) -> Option<bool> {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

        match key.code {
            KeyCode::Enter
                if key.modifiers.is_empty() && self.shell_mode() && !self.prompt.input.text().contains('\n') =>
            {
                self.submit();
                return Some(false);
            }
            KeyCode::Enter
                if ctrl || key.modifiers.contains(KeyModifiers::ALT) || key.modifiers.contains(KeyModifiers::SUPER) =>
            {
                self.submit();
                return Some(false);
            }
            // ctrl+s: save only (the thread view keeps the prompt by itself), never send
            KeyCode::Char('s') if ctrl => {
                self.toast("send: ctrl+enter (⌘enter, alt+enter) · the prompt stays as a draft");
                return Some(false);
            }
            KeyCode::PageUp => {
                self.scroll(self.layout.stream_rect.height.saturating_sub(2) as isize);
                return Some(false);
            }
            KeyCode::PageDown => {
                self.scroll(-(self.layout.stream_rect.height.saturating_sub(2) as isize));
                return Some(false);
            }
            KeyCode::Up | KeyCode::Down if key.modifiers.contains(KeyModifiers::SHIFT) => {
                return Some(key_scroll_body(self, key));
            }

            _ => {}
        }

        None
    }
}

fn key_scroll_body(view: &mut View, key: KeyEvent) -> bool {
    view.scroll(if key.code == KeyCode::Up { 3 } else { -3 });
    false
}
