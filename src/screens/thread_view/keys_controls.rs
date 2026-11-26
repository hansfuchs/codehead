use super::{KeyCode, KeyEvent, KeyModifiers, Pasted, View};

impl View {
    pub(super) fn key_controls(&mut self, key: KeyEvent) -> Option<bool> {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let command = key.modifiers.contains(KeyModifiers::SUPER);

        match key.code {
            KeyCode::Char('t') if ctrl => {
                self.toggle_thoughts();
                return Some(false);
            }
            KeyCode::Char('o') if ctrl || command => {
                self.cost_box.toggle();
                return Some(false);
            }
            // a prompt with text: ctrl+e = its line end (the text area)
            KeyCode::Char('e') if ctrl && self.prompt.input.is_empty() => {
                self.stream.all_runs = !self.stream.all_runs;
                self.stream.open.clear();
                self.stream.dirty = true;
                return Some(false);
            }
            KeyCode::Char('v') if ctrl => {
                return Some(paste_clipboard(self));
            }

            _ => {}
        }

        None
    }
}

fn paste_clipboard(view: &mut View) -> bool {
    let pasted = view.prompt.paster.clipboard();

    match pasted {
        Pasted::Text(text) => view.prompt.input.paste(&text),
        Pasted::File { path, marker } => {
            view.prompt.input.insert(&marker);
            view.prompt.attachments.push((marker, path));
        }
        Pasted::Nothing => view.toast("clipboard: 0 image, 0 text"),
    }

    false
}
