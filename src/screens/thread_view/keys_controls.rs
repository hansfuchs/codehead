use super::{KeyCode, KeyEvent, KeyModifiers, Pasted, View};

impl View {
    pub(super) fn key_controls(&mut self, key: KeyEvent) -> Option<bool> {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

        match key.code {
            KeyCode::Char('t') if ctrl => {
                self.toggle_thoughts();
                return Some(false);
            }
            KeyCode::Char('o') if ctrl => {
                self.stream.all_runs = !self.stream.all_runs;
                self.stream.open.clear();
                self.stream.dirty = true;
                return Some(false);
            }
            KeyCode::Char('v') if ctrl => {
                return Some(key_controls_path(self));
            }

            _ => {}
        }

        None
    }
}

fn key_controls_path(view: &mut View) -> bool {
    let path = view.prompt.paster.clipboard();

    match path {
        Pasted::Text(thread) => view.prompt.input.paste(&thread),
        Pasted::File { path, marker } => {
            view.prompt.input.insert(&marker);
            view.prompt.attachments.push((marker, path));
        }
        Pasted::Nothing => view.toast("clipboard: 0 image, 0 text"),
    }

    false
}
