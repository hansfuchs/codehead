use super::{FlowsTab, Focus, KeyCode, KeyEvent, KeyModifiers, Out};

impl FlowsTab {
    pub(crate) fn key(&mut self, key_value: KeyEvent) -> Out {
        key_flows_tab(self, key_value)
    }

    fn key_body_state(&mut self) {
        self.key_body();
    }

    fn key_body(&mut self) {
        self.filter.clear();
        self.filtering = false;
    }
}

fn key_flows_tab(state: &mut FlowsTab, key_value: KeyEvent) -> Out {
    let ctrl = key_value.modifiers.contains(KeyModifiers::CONTROL);

    if !matches!(key_value.code, KeyCode::Char('d')) || ctrl {
        state.armed = false;
    }

    if !(ctrl && key_value.code == KeyCode::Char('d')) {
        state.reset_armed = false;
    }

    if state.ask.is_some() {
        state.ask_key(key_value);
        return state.out();
    }

    if state.picker.is_some() {
        state.picker_key(key_value);
        return state.out();
    }

    if let Some(thread) = state.target {
        state.target_key(key_value, thread);
        return state.out();
    }

    if state.filtering {
        match key_value.code {
            KeyCode::Esc => {
                state.key_body_state();
            }
            KeyCode::Enter | KeyCode::Down => state.filtering = false,
            KeyCode::Backspace | KeyCode::Char('u' | 'w' | 'h')
                if crate::ui::textarea::delete_key(&mut state.filter, &key_value) => {}
            KeyCode::Char(character) if !ctrl => state.filter.push(character),
            _ => {}
        }

        state.at = 0;
        state.load_flow();
        return Out::None;
    }

    match state.focus {
        Focus::List => return state.list_key(key_value),
        Focus::Rail => state.rail_key(key_value),
        Focus::Editor => state.editor_key(key_value),
    }

    state.out()
}
