use super::{Act, Focus, KeyCode, KeyEvent, KeyModifiers, Mode, Out, ThreadList};

impl ThreadList {
    pub(super) fn list_key(&mut self, key: KeyEvent) -> Out {
        list_key_thread_list(self, key)
    }
}

fn list_key_keycode_up_keycode_char(view_state: &mut ThreadList) {
    if view_state.cursor == 0 {
        view_state.focus = if view_state.has_stats() {
            Focus::Status
        } else {
            Focus::Views
        };
    } else {
        view_state.cursor -= 1;
    }
}

fn list_key_keycode_char(count: usize, view_state: &mut ThreadList) {
    if let Some(id) = view_state.current().map(|character| character.id.clone()) {
        if !view_state.sel.remove(&id) {
            view_state.sel.insert(id);
        }

        if view_state.cursor + 1 < count {
            view_state.cursor += 1;
        }
    }
}

fn list_key_body(key: KeyEvent, view_state: &mut ThreadList) -> Out {
    match key.code {
        KeyCode::Esc if !view_state.filter.text().is_empty() => {
            view_state.filter.set_text("");
            Out::None
        }
        _ => Out::Pass,
    }
}

fn list_key_keycode_down_keycode_char(count: usize, view_state: &mut ThreadList) {
    if view_state.cursor + 1 < count {
        view_state.cursor += 1;
    }
}

fn list_key_branch(view_state: &mut ThreadList) {
    if !view_state.acts().is_empty() {
        view_state.job_state.actions = Some(0);
    }
}

fn list_key_body_state(key: KeyEvent, view_state: &mut ThreadList) -> Out {
    view_state.step_repo(if key.code == KeyCode::Right { 1 } else { -1 });
    Out::None
}

fn list_key_thread_list(state: &mut ThreadList, key: KeyEvent) -> Out {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let count = state.visible().len();
    let cards = state.view.cards();

    match key.code {
        KeyCode::Left | KeyCode::Right if !ctrl => {
            return list_key_body_state(key, state);
        }
        KeyCode::Char('f') if ctrl && state.mode == Mode::Manage => {
            state.filtering = true;
            return Out::None;
        }
        _ => {}
    }

    // the host's views: the host moves its rows (↑ on its top row: `focus_views`)
    if !cards {
        return list_key_body(key, state);
    }

    match key.code {
        KeyCode::Up | KeyCode::Char('k') if key.code == KeyCode::Up || state.mode == Mode::Manage => {
            list_key_keycode_up_keycode_char(state);
        }
        KeyCode::Down | KeyCode::Char('j') if key.code == KeyCode::Down || state.mode == Mode::Manage => {
            list_key_keycode_down_keycode_char(count, state);
        }
        KeyCode::PageUp => state.cursor = state.cursor.saturating_sub(5),
        KeyCode::PageDown if count > 0 => state.cursor = (state.cursor + 5).min(count - 1),
        KeyCode::Enter => return state.run(Act::Open),
        _ if crate::ui::context_menu::is_key(&key) => {
            list_key_branch(state);
        }
        _ if state.mode == Mode::Switch => state.type_filter(key),
        KeyCode::Char(' ') => {
            list_key_keycode_char(count, state);
        }
        KeyCode::Char('a') if !ctrl => return state.run(Act::Archive),
        KeyCode::Char('u') if !ctrl => return state.run(Act::Unarchive),
        KeyCode::Char('e') if !ctrl => return state.run(Act::Edit),
        KeyCode::Char('d') if !ctrl => return state.run(Act::Delete),
        KeyCode::Esc if !state.filter.text().is_empty() => {
            state.filter.set_text("");
            state.cursor = 0;
        }
        KeyCode::Esc if !state.sel.is_empty() => state.sel.clear(),
        _ => return Out::Pass,
    }

    Out::None
}
