use super::{Act, Focus, KeyCode, KeyEvent, KeyModifiers, Mode, Out, ThreadList};

impl ThreadList {
    pub(super) fn list_key(&mut self, key: KeyEvent) -> Out {
        list_key_thread_list(self, key)
    }
}

fn cursor_up_or_tabs(thread_list: &mut ThreadList) {
    if thread_list.cursor == 0 {
        thread_list.focus = if thread_list.has_stats() {
            Focus::Status
        } else {
            Focus::Views
        };
    } else {
        thread_list.cursor -= 1;
    }
}

fn toggle_selected(count: usize, thread_list: &mut ThreadList) {
    if let Some(id) = thread_list.current().map(|card| card.id.clone()) {
        if !thread_list.sel.remove(&id) {
            thread_list.sel.insert(id);
        }

        if thread_list.cursor + 1 < count {
            thread_list.cursor += 1;
        }
    }
}

fn host_view_key(key: KeyEvent, thread_list: &mut ThreadList) -> Out {
    match key.code {
        KeyCode::Esc if !thread_list.filter.text().is_empty() => {
            thread_list.filter.set_text("");
            Out::None
        }
        _ => Out::Pass,
    }
}

fn cursor_down(count: usize, thread_list: &mut ThreadList) {
    if thread_list.cursor + 1 < count {
        thread_list.cursor += 1;
    }
}

fn open_actions(thread_list: &mut ThreadList) {
    if !thread_list.acts().is_empty() {
        thread_list.job_state.actions = Some(0);
    }
}

fn step_repo_key(key: KeyEvent, thread_list: &mut ThreadList) -> Out {
    thread_list.step_repo(if key.code == KeyCode::Right { 1 } else { -1 });
    Out::None
}

fn list_key_thread_list(state: &mut ThreadList, key: KeyEvent) -> Out {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let count = state.visible().len();
    let cards = state.view.cards();

    match key.code {
        KeyCode::Left | KeyCode::Right if !ctrl => {
            return step_repo_key(key, state);
        }
        KeyCode::Char('f') if ctrl && state.mode == Mode::Manage => {
            state.filtering = true;
            return Out::None;
        }
        _ => {}
    }

    // the host's views: the host moves its rows (↑ on its top row: `focus_views`)
    if !cards {
        return host_view_key(key, state);
    }

    match key.code {
        KeyCode::Up | KeyCode::Char('k') if key.code == KeyCode::Up || state.mode == Mode::Manage => {
            cursor_up_or_tabs(state);
        }
        KeyCode::Down | KeyCode::Char('j') if key.code == KeyCode::Down || state.mode == Mode::Manage => {
            cursor_down(count, state);
        }
        KeyCode::PageUp => state.cursor = state.cursor.saturating_sub(5),
        KeyCode::PageDown if count > 0 => state.cursor = (state.cursor + 5).min(count - 1),
        KeyCode::Enter => return state.run(Act::Open),
        _ if crate::ui::context_menu::is_key(&key) => {
            open_actions(state);
        }
        _ if state.mode == Mode::Switch => state.type_filter(key),
        KeyCode::Char(' ') => {
            toggle_selected(count, state);
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
