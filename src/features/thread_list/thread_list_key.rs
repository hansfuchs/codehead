use super::{Act, Focus, Form, FormOut, KeyCode, KeyEvent, KeyModifiers, Mode, Out, ThreadList};

impl Act {
    pub(super) fn key(self) -> &'static str {
        match self {
            Act::Open => "enter",
            Act::Archive => "a",
            Act::Unarchive => "u",
            Act::Edit => "e",
            Act::Delete => "d",
        }
    }
}

impl ThreadList {
    pub(crate) fn key(&mut self, key_value: KeyEvent) -> Out {
        key_thread_list(self, key_value)
    }
}

impl Form {
    pub(super) fn key(&mut self, key_value: KeyEvent) -> FormOut {
        key_form(self, key_value)
    }
}

fn finish_key(key_value: KeyEvent, ctrl: bool, view_state: &mut ThreadList) -> Out {
    match view_state.focus {
        Focus::Repos => key_focus_repos(key_value, ctrl, view_state),
        Focus::Projects => key_focus_projects(key_value, ctrl, view_state),
        Focus::Views => finish_key_body(key_value, ctrl, view_state),
        Focus::Status => finish_key_body_state(key_value, ctrl, view_state),
        Focus::List => view_state.list_key(key_value),
    }
}

fn finish_key_body_state(key_value: KeyEvent, ctrl: bool, view_state: &mut ThreadList) -> Out {
    match key_value.code {
        KeyCode::Up => view_state.focus = Focus::Views,
        KeyCode::Down | KeyCode::Enter => view_state.focus = Focus::List,
        KeyCode::Left | KeyCode::Right => {
            view_state.step_stat(if key_value.code == KeyCode::Right { 1 } else { -1 });
        }
        KeyCode::Char(_) if view_state.mode == Mode::Switch && !ctrl => {
            view_state.focus = Focus::List;
            view_state.type_filter(key_value);
        }
        _ => return Out::Pass,
    }

    Out::None
}

fn finish_key_body(key_value: KeyEvent, ctrl: bool, view_state: &mut ThreadList) -> Out {
    match key_value.code {
        KeyCode::Up => view_state.focus = Focus::Projects,
        KeyCode::Down if view_state.has_stats() => view_state.focus = Focus::Status,
        KeyCode::Down | KeyCode::Enter => view_state.focus = Focus::List,
        KeyCode::Left | KeyCode::Right => {
            view_state.step_view(if key_value.code == KeyCode::Right { 1 } else { -1 });
        }
        KeyCode::Char(_) if view_state.mode == Mode::Switch && !ctrl => {
            view_state.focus = Focus::List;
            view_state.type_filter(key_value);
        }
        _ => return Out::Pass,
    }

    Out::None
}

fn key_focus_projects(key_value: KeyEvent, ctrl: bool, view_state: &mut ThreadList) -> Out {
    match key_value.code {
        KeyCode::Up => view_state.focus = Focus::Repos,
        KeyCode::Down => view_state.focus = Focus::Views,
        KeyCode::Enter => view_state.focus = Focus::List,
        KeyCode::Left | KeyCode::Right => view_state.step_proj(if key_value.code == KeyCode::Right { 1 } else { -1 }),
        KeyCode::Char(_) if view_state.mode == Mode::Switch && !ctrl => {
            view_state.focus = Focus::List;
            view_state.type_filter(key_value);
        }
        _ => return Out::Pass,
    }

    Out::None
}

fn key_focus_repos(key_value: KeyEvent, ctrl: bool, view_state: &mut ThreadList) -> Out {
    match key_value.code {
        KeyCode::Up => return Out::Up,
        KeyCode::Down => view_state.focus = Focus::Projects,
        KeyCode::Enter => view_state.focus = Focus::List,
        KeyCode::Left | KeyCode::Right => view_state.step_repo(if key_value.code == KeyCode::Right { 1 } else { -1 }),
        KeyCode::Char(_) if view_state.mode == Mode::Switch && !ctrl => {
            view_state.focus = Focus::List;
            view_state.type_filter(key_value);
        }
        _ => return Out::Pass,
    }

    Out::None
}

fn key_keycode_char_keycode_enter(view_state: &mut Form) {
    let i = view_state.row - 2;

    view_state.projs[i].1 = !view_state.projs[i].1;

    if i == 0 {
        // auto: the agents' pick from now on; off: the current picks stay
        key_keycode_char_keycode_enter_branch(view_state);
    } else {
        view_state.projs[0].1 = false;
    }
}

fn key_keycode_char_keycode_enter_branch(view_state: &mut Form) {
    if view_state.projs[0].1 {
        for path in view_state.projs.iter_mut().skip(1) {
            path.1 = false;
        }
    }
}

fn key_directory(key_value: KeyEvent, view_state: &mut Form) {
    let directory: isize = if key_value.code == KeyCode::Right { 1 } else { -1 };

    view_state.repo_i = (view_state.repo_i as isize + directory).rem_euclid(view_state.repos.len() as isize) as usize;
}

fn key_thread_list(state: &mut ThreadList, key_value: KeyEvent) -> Out {
    state.poll(true);
    let ctrl = key_value.modifiers.contains(KeyModifiers::CONTROL);

    if let Some(form) = state.form.as_mut() {
        match form.key(key_value) {
            FormOut::Stay => {}
            FormOut::Close => state.form = None,
            FormOut::Save(id, error) => {
                state.form = None;
                state.edit(id, error);
            }
        }

        return Out::None;
    }

    if state.job_state.pending.is_some() {
        return match key_value.code {
            KeyCode::Enter | KeyCode::Char('y') => state.confirm(),
            _ => {
                state.job_state.pending = None;
                Out::None
            }
        };
    }

    if let Some(cur) = state.job_state.actions {
        let acts = state.acts();

        match key_value.code {
            KeyCode::Esc => state.job_state.actions = None,
            _ if crate::ui::context_menu::is_key(&key_value) => state.job_state.actions = None,
            KeyCode::Up => state.job_state.actions = Some(cur.saturating_sub(1)),
            KeyCode::Down => state.job_state.actions = Some((cur + 1).min(acts.len().saturating_sub(1))),
            KeyCode::Enter => return acts.get(cur).map_or(Out::None, |&agent| state.run(agent)),
            KeyCode::Char(character) => {
                return acts
                    .iter()
                    .find(|action| action.key() == character.to_string())
                    .copied()
                    .map_or(Out::None, |action| state.run(action));
            }
            _ => {}
        }

        return Out::None;
    }

    if state.filtering {
        match key_value.code {
            KeyCode::Esc | KeyCode::Enter => state.filtering = false,
            KeyCode::Up => state.cursor = state.cursor.saturating_sub(1),
            KeyCode::Down => state.cursor = (state.cursor + 1).min(state.count().saturating_sub(1)),
            _ => state.type_filter(key_value),
        }

        return Out::None;
    }

    finish_key(key_value, ctrl, state)
}

fn key_agent(view_state: &mut Form) {
    let i = view_state.row - 2 - view_state.projs.len();

    match view_state.links.get_mut(i) {
        Some(line) => line.1 = !line.1,
        None => {
            if let Some(agent) = view_state.agents.get_mut(i - view_state.links.len()) {
                agent.1 = !agent.1;
            }
        }
    }
}

fn key_form(state: &mut Form, key_value: KeyEvent) -> FormOut {
    let ctrl = key_value.modifiers.contains(KeyModifiers::CONTROL) || key_value.modifiers.contains(KeyModifiers::SUPER);
    let count = state.n_rows();

    match key_value.code {
        KeyCode::Esc => return FormOut::Close,
        KeyCode::Char('s') if ctrl => return state.save(),
        KeyCode::Enter if ctrl || state.row == 0 => return state.save(),
        KeyCode::Up | KeyCode::BackTab => state.row = state.row.saturating_sub(1),
        KeyCode::Down | KeyCode::Tab => state.row = (state.row + 1).min(count - 1),
        _ if state.row == 0 => {
            state.title.key(key_value);
        }
        KeyCode::Left | KeyCode::Right if state.row == 1 => {
            key_directory(key_value, state);
        }
        KeyCode::Char(' ') | KeyCode::Enter if state.row >= 2 && state.row < 2 + state.projs.len() => {
            key_keycode_char_keycode_enter(state);
        }
        KeyCode::Char(' ') | KeyCode::Enter if state.row >= 2 => {
            key_agent(state);
        }
        _ => {}
    }

    FormOut::Stay
}
