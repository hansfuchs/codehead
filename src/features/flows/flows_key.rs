use super::{FlowsTab, Focus, KeyCode, KeyEvent, KeyModifiers, Out};

impl FlowsTab {
    pub(crate) fn key(&mut self, key: KeyEvent) -> Out {
        key_flows_tab(self, key)
    }

    fn clear_filter(&mut self) {
        self.filter.clear();
        self.filtering = false;
    }
}

fn key_flows_tab(state: &mut FlowsTab, key: KeyEvent) -> Out {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

    if !matches!(key.code, KeyCode::Char('d')) || ctrl {
        state.armed = false;
    }

    if !(ctrl && key.code == KeyCode::Char('d')) {
        state.reset_armed = false;
    }

    if state.ask.is_some() {
        state.ask_key(key);
        return state.out();
    }

    if state.picker.is_some() {
        state.picker_key(key);
        return state.out();
    }

    if let Some(target) = state.target {
        state.target_key(key, target);
        return state.out();
    }

    if state.filtering {
        match key.code {
            KeyCode::Esc => {
                state.clear_filter();
            }
            KeyCode::Enter | KeyCode::Down => state.filtering = false,
            KeyCode::Backspace | KeyCode::Char('u' | 'w' | 'h')
                if crate::ui::textarea::delete_key(&mut state.filter, &key) => {}
            KeyCode::Char(character) if !ctrl => state.filter.push(character),
            _ => {}
        }

        state.at = 0;
        state.load_flow();
        return Out::None;
    }

    match state.focus {
        Focus::List => return state.list_key(key),
        Focus::Rail => state.rail_key(key),
        Focus::Editor => state.editor_key(key),
    }

    state.out()
}
