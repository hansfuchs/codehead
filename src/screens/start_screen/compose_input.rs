use super::{Act, App, Field, Input, KeyCode, KeyEvent, KeyModifiers};

impl App {
    /// The task's fields top to bottom: drafts row, prompt, title, where + its boxes (a
    /// git repo), agents, notes.
    pub(super) fn fields(&self) -> Vec<Field> {
        let mut fields = vec![Field::Drafts];

        // the repo row: always (the picker finds a repo also when codehead runs outside of one)
        fields.extend([Field::Prompt, Field::Title, Field::Repo]);

        if self.cfg.repo.is_some() {
            fields.push(Field::Projects);
        }

        if !self.repository.wheres.is_empty() {
            fields.extend([Field::Where, Field::Opts]);
        }

        fields.extend([Field::Agents, Field::Notes]);

        fields
    }

    /// ↑↓ to the field D steps away: above the top one the tabs row, below the last 0 move.
    pub(super) fn go_field(&mut self, delta: isize) {
        let fields = self.fields();
        let at = fields.iter().position(|x| *x == self.compose.field).unwrap_or(0) as isize + delta;

        if at < 0 {
            self.tabs_focus = true;
        } else if let Some(&x) = fields.get(at as usize) {
            enter_field(self, delta, x);
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

fn step_pick_cursor(app: &mut App, dx: isize, dy: isize) -> bool {
    if let Some(i) = App::grid_step(&app.compose.pick_places, app.compose.pick_cursor, dx, dy) {
        app.compose.pick_cursor = i;
        true
    } else {
        false
    }
}

fn enter_field(app: &mut App, delta: isize, x: Field) {
    app.compose.field = x;

    // the opts boxes from above: the 1st, from below: the last
    if x == Field::Opts {
        app.repository.opt_i = usize::from(delta <= 0);
    }
}

fn compose_key_input(app: &mut App, key: KeyEvent) {
    let input = if app.compose.field == Field::Title {
        &mut app.compose.title
    } else {
        &mut app.compose.prompt
    };

    let _: Input = input.key(key);
}

fn compose_shortcut(app: &mut App, key: KeyEvent) -> bool {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    let plain = !key.modifiers.contains(KeyModifiers::SHIFT);

    match key.code {
        KeyCode::Esc => (),
        // cmd+enter (SUPER) needs a terminal that reports it (kitty keyboard protocol)
        KeyCode::Enter if ctrl || alt || key.modifiers.contains(KeyModifiers::SUPER) => app.start(),
        KeyCode::Char('s') if ctrl => app.run(Act::SaveDraft),
        // ctrl+v / ⌘v in the prompt, like Claude Code: the clipboard image, else its text
        KeyCode::Char('v' | 'V')
            if app.compose.field == Field::Prompt && (ctrl || key.modifiers.contains(KeyModifiers::SUPER)) =>
        {
            let pasted = app.compose.paster.clipboard();

            app.paste_prompt(pasted);
        }
        // the prompt (top field): ↑ on its 1st row / ↓ on its last row leave it
        KeyCode::Up if app.compose.field == Field::Prompt && plain && app.compose.prompt.on_first_row() => {
            app.go_field(-1);
        }
        KeyCode::Down if app.compose.field == Field::Prompt && plain && app.compose.prompt.on_last_row() => {
            app.go_field(1);
        }
        KeyCode::Enter | KeyCode::Down if app.compose.field == Field::Title => app.go_field(1),
        KeyCode::Up if app.compose.field == Field::Title => app.go_field(-1),
        _ => return false,
    }

    true
}

fn compose_drafts_key(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Enter | KeyCode::Char(' ') if app.draft_btn() == 0 => app.open_drafts(),
        KeyCode::Enter | KeyCode::Char(' ') => app.save_draft(),
        KeyCode::Left | KeyCode::Char('h') if !app.draft_selection.drafts.is_empty() => {
            app.draft_selection.draft_btn = 0;
        }
        KeyCode::Right | KeyCode::Char('l') => app.draft_selection.draft_btn = 1,
        KeyCode::Down => app.go_field(1),
        KeyCode::Up => app.go_field(-1),
        _ => {}
    }
}

fn compose_repo_key(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Enter | KeyCode::Char(' ') => app.open_repo_pick(),
        KeyCode::Up => app.go_field(-1),
        KeyCode::Down => app.go_field(1),
        _ => {}
    }
}

fn compose_projects_key(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Char(' ') | KeyCode::Enter => app.open_projects(),
        KeyCode::Up => app.go_field(-1),
        KeyCode::Down => app.go_field(1),
        _ => {}
    }
}

fn compose_where_key(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Enter | KeyCode::Char(' ') => app.open_where(),
        KeyCode::Up => app.go_field(-1),
        KeyCode::Down => app.go_field(1),
        _ => {}
    }
}

fn compose_opts_key(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Char(' ') => app.toggle_opt(app.repository.opt_i),
        KeyCode::Up | KeyCode::Char('k') if app.repository.opt_i > 0 => app.repository.opt_i -= 1,
        KeyCode::Down | KeyCode::Char('j') if app.repository.opt_i == 0 => app.repository.opt_i = 1,
        KeyCode::Up => app.go_field(-1),
        KeyCode::Enter | KeyCode::Down => app.go_field(1),
        _ => {}
    }
}

fn compose_notes_key(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Enter | KeyCode::Char(' ') => app.open_notes(),
        KeyCode::Up => app.go_field(-1),
        KeyCode::Char('x') | KeyCode::Backspace | KeyCode::Delete => {
            app.note_list.selected.clear();
            app.compose.carry.clear();
        }
        _ => {}
    }
}

fn compose_agents_key(app: &mut App, key: KeyEvent) {
    let step = |app: &mut App, dx, dy| step_pick_cursor(app, dx, dy);

    match key.code {
        KeyCode::Left | KeyCode::Char('h') => {
            step(app, -1, 0);
        }
        KeyCode::Right | KeyCode::Char('l') => {
            step(app, 1, 0);
        }
        KeyCode::Char(' ') => app.toggle_pick(),
        KeyCode::Up | KeyCode::Char('k') => {
            step_up_or_leave(app, step);
        }
        KeyCode::Down | KeyCode::Char('j') => {
            if !step(app, 0, 1) {
                app.go_field(1);
            }
        }
        KeyCode::Enter => app.go_field(1),
        _ => {}
    }
}

fn step_up_or_leave(app: &mut App, step: impl Fn(&mut App, isize, isize) -> bool) {
    if !step(app, 0, -1) {
        app.go_field(-1);
    }
}
