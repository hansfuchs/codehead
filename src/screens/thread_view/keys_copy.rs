use super::{Duration, Exit, Instant, KeyCode, KeyEvent, KeyModifiers, View};

impl View {
    pub(super) fn key_copy(&mut self, key: KeyEvent) -> Option<bool> {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let empty = self.prompt.input.is_empty();

        match key.code {
            KeyCode::Char('c') if ctrl && self.selection.sel.is_some() => {
                copy_selected_stream(self);

                self.selection.sel = None;
                return Some(false);
            }
            // ctrl+c: the prompt's selection → copy; text → clear it (ctrl+z brings it back); an
            // empty prompt: quit on a 2nd ctrl+c within 1 s
            KeyCode::Char('c') if ctrl => {
                return Some(key_copy_thread(self, empty));
            }

            _ => {}
        }

        None
    }
}

fn key_copy_thread(view: &mut View, empty: bool) -> bool {
    if let Some(thread) = view.prompt.input.selected_text() {
        view.copy(&thread);
        return false;
    }

    if !empty {
        view.prompt.input.load_text("");
        view.toast("prompt cleared · ctrl+z: back");
        return false;
    }

    if view
        .last_ctrl_c
        .is_some_and(|thread| thread.elapsed() < Duration::from_secs(1))
    {
        view.exit = Exit::Quit;
        return true;
    }

    view.last_ctrl_c = Some(Instant::now());
    view.toast("ctrl+c again: quit");

    false
}

fn copy_selected_stream(view: &mut View) {
    if let Some(thread) = view.selected() {
        view.copy(&thread);
    }
}
