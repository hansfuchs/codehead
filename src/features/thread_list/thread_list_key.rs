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
    pub(crate) fn key(&mut self, key: KeyEvent) -> Out {
        key_thread_list(self, key)
    }
}

impl Form {
    pub(super) fn key(&mut self, key: KeyEvent) -> FormOut {
        key_form(self, key)
    }
}

fn focus_key(key: KeyEvent, ctrl: bool, thread_list: &mut ThreadList) -> Out {
    match thread_list.focus {
        Focus::Repos => repo_tabs_key(key, ctrl, thread_list),
        Focus::Projects => project_tabs_key(key, ctrl, thread_list),
        Focus::Views => view_tabs_key(key, ctrl, thread_list),
        Focus::Status => status_tabs_key(key, ctrl, thread_list),
        Focus::List => thread_list.list_key(key),
    }
}

fn status_tabs_key(key: KeyEvent, ctrl: bool, thread_list: &mut ThreadList) -> Out {
    match key.code {
        KeyCode::Up => thread_list.focus = Focus::Views,
        KeyCode::Down | KeyCode::Enter => thread_list.focus = Focus::List,
        KeyCode::Left | KeyCode::Right => {
            thread_list.step_stat(if key.code == KeyCode::Right { 1 } else { -1 });
        }
        KeyCode::Char(_) if thread_list.mode == Mode::Switch && !ctrl => {
            thread_list.focus = Focus::List;
            thread_list.type_filter(key);
        }
        _ => return Out::Pass,
    }

    Out::None
}

fn view_tabs_key(key: KeyEvent, ctrl: bool, thread_list: &mut ThreadList) -> Out {
    match key.code {
        KeyCode::Up => thread_list.focus = Focus::Projects,
        KeyCode::Down if thread_list.has_stats() => thread_list.focus = Focus::Status,
        KeyCode::Down | KeyCode::Enter => thread_list.focus = Focus::List,
        KeyCode::Left | KeyCode::Right => {
            thread_list.step_view(if key.code == KeyCode::Right { 1 } else { -1 });
        }
        KeyCode::Char(_) if thread_list.mode == Mode::Switch && !ctrl => {
            thread_list.focus = Focus::List;
            thread_list.type_filter(key);
        }
        _ => return Out::Pass,
    }

    Out::None
}

fn project_tabs_key(key: KeyEvent, ctrl: bool, thread_list: &mut ThreadList) -> Out {
    match key.code {
        KeyCode::Up => thread_list.focus = Focus::Repos,
        KeyCode::Down => thread_list.focus = Focus::Views,
        KeyCode::Enter => thread_list.focus = Focus::List,
        KeyCode::Left | KeyCode::Right => thread_list.step_proj(if key.code == KeyCode::Right { 1 } else { -1 }),
        KeyCode::Char(_) if thread_list.mode == Mode::Switch && !ctrl => {
            thread_list.focus = Focus::List;
            thread_list.type_filter(key);
        }
        _ => return Out::Pass,
    }

    Out::None
}

fn repo_tabs_key(key: KeyEvent, ctrl: bool, thread_list: &mut ThreadList) -> Out {
    match key.code {
        KeyCode::Up => return Out::Up,
        KeyCode::Down => thread_list.focus = Focus::Projects,
        KeyCode::Enter => thread_list.focus = Focus::List,
        KeyCode::Left | KeyCode::Right => thread_list.step_repo(if key.code == KeyCode::Right { 1 } else { -1 }),
        KeyCode::Char(_) if thread_list.mode == Mode::Switch && !ctrl => {
            thread_list.focus = Focus::List;
            thread_list.type_filter(key);
        }
        _ => return Out::Pass,
    }

    Out::None
}

fn toggle_project(form: &mut Form) {
    let i = form.row - 2;

    form.projs[i].1 = !form.projs[i].1;

    if i == 0 {
        // auto: the agents' pick from now on; off: the current picks stay
        clear_picks_on_auto(form);
    } else {
        form.projs[0].1 = false;
    }
}

fn clear_picks_on_auto(form: &mut Form) {
    if form.projs[0].1 {
        for project in form.projs.iter_mut().skip(1) {
            project.1 = false;
        }
    }
}

fn step_repo_pick(key: KeyEvent, form: &mut Form) {
    let delta: isize = if key.code == KeyCode::Right { 1 } else { -1 };

    form.repo_i = (form.repo_i as isize + delta).rem_euclid(form.repos.len() as isize) as usize;
}

fn key_thread_list(state: &mut ThreadList, key: KeyEvent) -> Out {
    state.poll(true);
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

    if let Some(form) = state.form.as_mut() {
        match form.key(key) {
            FormOut::Stay => {}
            FormOut::Close => state.form = None,
            FormOut::Save(id, edit) => {
                state.form = None;
                state.edit(id, edit);
            }
        }

        return Out::None;
    }

    if state.job_state.pending.is_some() {
        return match key.code {
            KeyCode::Enter | KeyCode::Char('y') => state.confirm(),
            _ => {
                state.job_state.pending = None;
                Out::None
            }
        };
    }

    if let Some(cur) = state.job_state.actions {
        let acts = state.acts();

        match key.code {
            KeyCode::Esc => state.job_state.actions = None,
            _ if crate::ui::context_menu::is_key(&key) => state.job_state.actions = None,
            KeyCode::Up => state.job_state.actions = Some(cur.saturating_sub(1)),
            KeyCode::Down => state.job_state.actions = Some((cur + 1).min(acts.len().saturating_sub(1))),
            KeyCode::Enter => return acts.get(cur).map_or(Out::None, |&act| state.run(act)),
            KeyCode::Char(letter) => {
                return acts
                    .iter()
                    .find(|action| action.key() == letter.to_string())
                    .copied()
                    .map_or(Out::None, |action| state.run(action));
            }
            _ => {}
        }

        return Out::None;
    }

    if state.filtering {
        match key.code {
            KeyCode::Esc | KeyCode::Enter => state.filtering = false,
            KeyCode::Up => state.cursor = state.cursor.saturating_sub(1),
            KeyCode::Down => state.cursor = (state.cursor + 1).min(state.count().saturating_sub(1)),
            _ => state.type_filter(key),
        }

        return Out::None;
    }

    focus_key(key, ctrl, state)
}

fn toggle_link_or_agent(form: &mut Form) {
    let i = form.row - 2 - form.projs.len();

    match form.links.get_mut(i) {
        Some(link) => link.1 = !link.1,
        None => {
            if let Some(agent) = form.agents.get_mut(i - form.links.len()) {
                agent.1 = !agent.1;
            }
        }
    }
}

fn key_form(state: &mut Form, key: KeyEvent) -> FormOut {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL) || key.modifiers.contains(KeyModifiers::SUPER);
    let rows = state.n_rows();

    match key.code {
        KeyCode::Esc => return FormOut::Close,
        KeyCode::Char('s') if ctrl => return state.save(),
        KeyCode::Enter if ctrl || state.row == 0 => return state.save(),
        KeyCode::Up | KeyCode::BackTab => state.row = state.row.saturating_sub(1),
        KeyCode::Down | KeyCode::Tab => state.row = (state.row + 1).min(rows - 1),
        _ if state.row == 0 => {
            state.title.key(key);
        }
        KeyCode::Left | KeyCode::Right if state.row == 1 => {
            step_repo_pick(key, state);
        }
        KeyCode::Char(' ') | KeyCode::Enter if state.row >= 2 && state.row < 2 + state.projs.len() => {
            toggle_project(state);
        }
        KeyCode::Char(' ') | KeyCode::Enter if state.row >= 2 => {
            toggle_link_or_agent(state);
        }
        _ => {}
    }

    FormOut::Stay
}
