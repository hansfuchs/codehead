use super::{Instant, KeyCode, KeyEvent, Outcome, View};

type KeyHandler = fn(&mut View, KeyEvent) -> Option<bool>;

const HANDLERS: [KeyHandler; 15] = [
    View::key_global,
    View::key_focus,
    View::key_search,
    View::key_flow,
    View::key_subthreads,
    View::key_activity,
    View::key_pane_switch,
    View::key_pane_focus,
    View::key_ask,
    View::key_prompt_dropdown,
    View::key_escape,
    View::key_copy,
    View::key_controls,
    View::key_scroll,
    View::key_history,
];

impl View {
    pub(super) fn key(&mut self, key: KeyEvent) -> bool {
        if matches!(key.code, KeyCode::Modifier(_)) {
            return false;
        }

        self.last_key = Some(Instant::now());
        self.merge_state.merge.touch();

        if self.menu.is_none() && self.stream.flash.take().is_some() {
            self.stream.dirty = true;
        }

        HANDLERS
            .iter()
            .find_map(|handler| handler(self, key))
            .unwrap_or_else(|| key_input(self, key))
    }
}

fn key_input(view: &mut View, key: KeyEvent) -> bool {
    if key.code == KeyCode::Enter && view.prompt.input.is_empty() {
        return false;
    }

    if let Outcome::Handled = view.prompt.input.key(key) {
        view.prompt.hist_at = None;
        reset_shell_history(view, key);
    } else if key.code == KeyCode::Enter {
        view.prompt.input.newline();
    }

    view.prompt.hist.input_changed(&view.prompt.input.text());
    false
}

fn reset_shell_history(view: &mut View, key: KeyEvent) {
    if !matches!(key.code, KeyCode::Up | KeyCode::Down) {
        view.shell_state.shell_at = None;
    }
}
