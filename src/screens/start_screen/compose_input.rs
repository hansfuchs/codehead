use super::{Act, App, Field, Input, KeyCode, KeyEvent, KeyModifiers};

impl App {
    /// The task's fields top to bottom: drafts row, prompt, title, where + its boxes (a
    /// git repo), agents, notes.
    pub(super) fn fields(&self) -> Vec<Field> {
        let mut value = vec![Field::Drafts];

        // the repo row: always (the picker finds a repo also when codehead runs outside of one)
        value.extend([Field::Prompt, Field::Title, Field::Repo]);

        if self.cfg.repo.is_some() {
            value.push(Field::Projects);
        }

        if !self.repository.wheres.is_empty() {
            value.extend([Field::Where, Field::Opts]);
        }

        value.extend([Field::Agents, Field::Notes]);

        value
    }

    /// ↑↓ to the field D steps away: above the top one the tabs row, below the last 0 move.
    pub(super) fn go_field(&mut self, directory: isize) {
        let file = self.fields();
        let at = file.iter().position(|x| *x == self.compose.field).unwrap_or(0) as isize + directory;

        if at < 0 {
            self.tabs_focus = true;
        } else if let Some(&x) = file.get(at as usize) {
            go_field_body(self, directory, x);
        }
    }

    pub(super) fn compose_key(&mut self, key: KeyEvent) {
        if compose_shortcut(self, key) {
            return;
        }

        match self.compose.field {
            Field::Drafts => compose_drafts_key(self, key),
            Field::Repo => compose_repo_key(self, key),
            Field::Projects => compose_projects_key(self, key),
            Field::Where => compose_where_key(self, key),
            Field::Opts => compose_opts_key(self, key),
            Field::Notes => compose_notes_key(self, key),
            Field::Agents => compose_agents_key(self, key),
            _ => {
                compose_key_input(self, key);
            }
        }
    }
}

fn compose_agents_key_branch(agent: &mut App, dx: isize, dy: isize) -> bool {
    if let Some(i) = App::grid_step(&agent.compose.pick_places, agent.compose.pick_cursor, dx, dy) {
        agent.compose.pick_cursor = i;
        true
    } else {
        false
    }
}

fn go_field_body(app_state: &mut App, directory: isize, x: Field) {
    app_state.compose.field = x;

    // the opts boxes from above: the 1st, from below: the last
    if x == Field::Opts {
        app_state.repository.opt_i = usize::from(directory <= 0);
    }
}

fn compose_key_input(app_state: &mut App, key: KeyEvent) {
    let input = if app_state.compose.field == Field::Title {
        &mut app_state.compose.title
    } else {
        &mut app_state.compose.prompt
    };

    let _: Input = input.key(key);
}

fn compose_shortcut(app_state: &mut App, key: KeyEvent) -> bool {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    let plain = !key.modifiers.contains(KeyModifiers::SHIFT);

    match key.code {
        KeyCode::Esc => (),
        // cmd+enter (SUPER) needs a terminal that reports it (kitty keyboard protocol)
        KeyCode::Enter if ctrl || alt || key.modifiers.contains(KeyModifiers::SUPER) => app_state.start(),
        KeyCode::Char('s') if ctrl => app_state.run(Act::SaveDraft),
        // ctrl+v / ⌘v in the prompt, like Claude Code: the clipboard image, else its text
        KeyCode::Char('v' | 'V')
            if app_state.compose.field == Field::Prompt && (ctrl || key.modifiers.contains(KeyModifiers::SUPER)) =>
        {
            let path = app_state.compose.paster.clipboard();

            app_state.paste_prompt(path);
        }
        // the prompt (top field): ↑ on its 1st row / ↓ on its last row leave it
        KeyCode::Up if app_state.compose.field == Field::Prompt && plain && app_state.compose.prompt.on_first_row() => {
            app_state.go_field(-1);
        }
        KeyCode::Down
            if app_state.compose.field == Field::Prompt && plain && app_state.compose.prompt.on_last_row() =>
        {
            app_state.go_field(1);
        }
        KeyCode::Enter | KeyCode::Down if app_state.compose.field == Field::Title => app_state.go_field(1),
        KeyCode::Up if app_state.compose.field == Field::Title => app_state.go_field(-1),
        _ => return false,
    }

    true
}

fn compose_drafts_key(app_state: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Enter | KeyCode::Char(' ') if app_state.draft_btn() == 0 => app_state.open_drafts(),
        KeyCode::Enter | KeyCode::Char(' ') => app_state.save_draft(),
        KeyCode::Left | KeyCode::Char('h') if !app_state.draft_selection.drafts.is_empty() => {
            app_state.draft_selection.draft_btn = 0;
        }
        KeyCode::Right | KeyCode::Char('l') => app_state.draft_selection.draft_btn = 1,
        KeyCode::Down => app_state.go_field(1),
        KeyCode::Up => app_state.go_field(-1),
        _ => {}
    }
}

fn compose_repo_key(app_state: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Enter | KeyCode::Char(' ') => app_state.open_repo_pick(),
        KeyCode::Up => app_state.go_field(-1),
        KeyCode::Down => app_state.go_field(1),
        _ => {}
    }
}

fn compose_projects_key(app_state: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Char(' ') | KeyCode::Enter => app_state.open_projects(),
        KeyCode::Up => app_state.go_field(-1),
        KeyCode::Down => app_state.go_field(1),
        _ => {}
    }
}

fn compose_where_key(app_state: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Enter | KeyCode::Char(' ') => app_state.open_where(),
        KeyCode::Up => app_state.go_field(-1),
        KeyCode::Down => app_state.go_field(1),
        _ => {}
    }
}

fn compose_opts_key(app_state: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Char(' ') => app_state.toggle_opt(app_state.repository.opt_i),
        KeyCode::Up | KeyCode::Char('k') if app_state.repository.opt_i > 0 => app_state.repository.opt_i -= 1,
        KeyCode::Down | KeyCode::Char('j') if app_state.repository.opt_i == 0 => app_state.repository.opt_i = 1,
        KeyCode::Up => app_state.go_field(-1),
        KeyCode::Enter | KeyCode::Down => app_state.go_field(1),
        _ => {}
    }
}

fn compose_notes_key(app_state: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Enter | KeyCode::Char(' ') => app_state.open_notes(),
        KeyCode::Up => app_state.go_field(-1),
        KeyCode::Char('x') | KeyCode::Backspace | KeyCode::Delete => {
            app_state.note_list.selected.clear();
            app_state.compose.carry.clear();
        }
        _ => {}
    }
}

fn compose_agents_key(app_state: &mut App, key: KeyEvent) {
    let step = |agent: &mut App, dx, dy| compose_agents_key_branch(agent, dx, dy);

    match key.code {
        KeyCode::Left | KeyCode::Char('h') => {
            step(app_state, -1, 0);
        }
        KeyCode::Right | KeyCode::Char('l') => {
            step(app_state, 1, 0);
        }
        KeyCode::Char(' ') => app_state.toggle_pick(),
        KeyCode::Up | KeyCode::Char('k') => {
            compose_agents_key_branch_state(app_state, step);
        }
        KeyCode::Down | KeyCode::Char('j') => {
            if !step(app_state, 0, 1) {
                app_state.go_field(1);
            }
        }
        KeyCode::Enter => app_state.go_field(1),
        _ => {}
    }
}

fn compose_agents_key_branch_state(app_state: &mut App, step: impl Fn(&mut App, isize, isize) -> bool) {
    if !step(app_state, 0, -1) {
        app_state.go_field(-1);
    }
}
