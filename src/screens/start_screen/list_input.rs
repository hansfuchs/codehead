use super::{Act, App, Field, Input, KeyCode, KeyEvent, Outcome, Path, Row, Screen, key_name};
use std::ops::ControlFlow;

impl App {
    pub(super) fn move_cursor(&mut self, delta: isize) {
        let count = self.note_list.rows.len();

        if count == 0 {
            return;
        }

        let character = (self.note_list.cursor as isize + delta).clamp(0, count as isize - 1) as usize;

        if character != self.note_list.cursor {
            self.note_list.cursor = character;
            self.preview.pscroll = 0;
        }
    }

    pub(super) fn fold(&mut self, open: bool) {
        let Some(row) = self.current().cloned() else { return };

        if let Row::Dir { path, collapsed, .. } = &row
            && fold_branch(self, open, path, *collapsed).is_break()
        {
            return;
        }

        if open || self.note_list.filter.is_some() {
            return;
        }

        // ← on a file / closed folder: fold the parent, cursor onto it
        let path_value = row.path().trim_end_matches('/');

        if let Some((parent, _)) = path_value.rsplit_once('/') {
            fold_parent(self, parent);
        }
    }

    pub(super) fn notes_key(&mut self, key: KeyEvent) {
        if self.note_list.filter_editing {
            notes_key_body(self, key);
            return;
        }

        match key.code {
            KeyCode::Up | KeyCode::Char('k') => self.move_cursor(-1),
            KeyCode::Down | KeyCode::Char('j') => self.move_cursor(1),
            KeyCode::Home | KeyCode::Char('g') => self.move_cursor(isize::MIN / 2),
            KeyCode::End | KeyCode::Char('G') => self.move_cursor(isize::MAX / 2),
            KeyCode::Right | KeyCode::Char('l') => self.fold(true),
            KeyCode::Left | KeyCode::Char('h') => self.fold(false),
            KeyCode::Char(' ') => {
                self.toggle_select();
                self.move_cursor(1);
            }
            KeyCode::Enter => self.confirm_notes(),
            KeyCode::Esc if self.note_list.filter.is_some() => {
                self.note_list.filter = None;
                self.rebuild();
            }
            KeyCode::Esc if self.cfg.copy_mode => self.outcome = Some(Outcome::Quit),
            // back to the task, selection kept
            KeyCode::Esc | KeyCode::Tab | KeyCode::BackTab => {
                self.compose.screen = Screen::Compose;
                self.compose.field = Field::Notes;
            }
            _ => {
                notes_key_name(self, key);
            }
        }
    }

    /// Repo of threads tab item I (worktrees ++ sessions).
    pub(super) fn trepo_of(&self, i: usize) -> Option<&Path> {
        let wts = self.thread_list.worktrees.as_deref().unwrap_or(&[]);

        match wts.get(i) {
            Some(width) => width.repo.as_deref(),
            None => self
                .thread_list
                .sessions
                .get(i - wts.len())
                .and_then(|text| text.repo.as_deref()),
        }
    }

    /// Keys of the threads tab: the thread list's (cards, its tabs, its filter, its actions),
    /// this screen's rows in the views worktrees / claude.
    pub(super) fn threads_key(&mut self, key: KeyEvent) {
        use crate::features::thread_list::{Focus, Out};
        let rows = !self.thread_list.tlist.view.cards()
            && self.thread_list.tlist.focus == Focus::List
            && !self.thread_list.tlist.modal();
        let before = (
            self.thread_list.tlist.cursor,
            self.thread_list.tlist.view,
            self.thread_list.tlist.query(),
        );

        let out = if rows {
            self.rows_key(key)
        } else {
            self.thread_list.tlist.key(key)
        };

        if before
            != (
                self.thread_list.tlist.cursor,
                self.thread_list.tlist.view,
                self.thread_list.tlist.query(),
            )
        {
            threads_key_body(self, &before);
        }

        match out {
            Out::None | Out::Home => {}
            Out::Up => self.tabs_focus = true,
            Out::Open(character) => self.open_card(&character),
            Out::Pass => {
                threads_key_name(self, key);
            }
        }
    }

    /// Keys on the rows of the views worktrees / claude (↑ on the top row: the view tabs; ←→,
    /// the filter: the thread list's).
    pub(super) fn rows_key(&mut self, key: KeyEvent) -> crate::features::thread_list::Out {
        use crate::features::thread_list::Out;
        let count = self.n_threads();

        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                rows_key_branch(self);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                rows_key_branch_state(self, count);
            }
            KeyCode::Enter => self.open_thread(),
            _ => return self.thread_list.tlist.key(key),
        }

        Out::None
    }

    pub(super) fn agents_key(&mut self, key: KeyEvent) {
        if self.agent_list.afilter_editing {
            agents_key_body(self, key);
            return;
        }

        let step = |agent: &mut Self, i: Option<usize>| {
            agents_key_branch(agent, i);
        };

        match key.code {
            KeyCode::Up | KeyCode::Char('k') => match self.agent_step(0, -1) {
                None => self.tabs_focus = true,
                i => step(self, i),
            },
            KeyCode::Down | KeyCode::Char('j') => step(self, self.agent_step(0, 1)),
            KeyCode::Left | KeyCode::Char('h') => step(self, self.agent_step(-1, 0)),
            KeyCode::Right | KeyCode::Char('l') => step(self, self.agent_step(1, 0)),
            KeyCode::Esc
                if self
                    .agent_list
                    .afilter
                    .as_deref()
                    .is_some_and(|query| !query.is_empty()) =>
            {
                self.agent_list.afilter = None;
                self.agent_list.afilter_ta.set_text("");
                self.agent_list.aoffset = 0;
            }
            KeyCode::Enter => self.run(Act::EditAgent),
            _ => {
                agents_key_name(self, key);
            }
        }
    }

    pub(super) fn toggle_pick(&mut self) {
        let Some(agent) = self.agent_list.agents.get(self.compose.pick_cursor) else {
            return;
        };

        let name = agent.name.clone();

        if let Some(i) = self.compose.picks.iter().position(|path| *path == name) {
            self.compose.picks.remove(i);
        } else {
            self.compose.picks.push(name);
        }
    }
}

fn agents_key_branch(agent: &mut App, i: Option<usize>) {
    if let Some(i) = i {
        agent.agent_list.acursor = i;
        agent.preview.pscroll = 0;
    }
}

fn notes_key_body(app_state: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Esc => {
            app_state.note_list.filter = None;
            app_state.note_list.filter_editing = false;
            app_state.rebuild();
        }
        KeyCode::Enter => app_state.note_list.filter_editing = false,
        KeyCode::Up => app_state.move_cursor(-1),
        KeyCode::Down => app_state.move_cursor(1),
        // backspace on an empty filter: filter closed
        KeyCode::Backspace if app_state.note_list.filter_ta.is_empty() => {
            app_state.note_list.filter = None;
            app_state.note_list.filter_editing = false;
            app_state.note_list.cursor = 0;
            app_state.rebuild();
        }
        _ => {
            if matches!(app_state.note_list.filter_ta.key(key), Input::Handled) {
                app_state.sync_filter();
            }
        }
    }
}

fn fold_branch(app_state: &mut App, open: bool, path: &String, collapsed: bool) -> ControlFlow<()> {
    if open && collapsed {
        app_state.note_list.collapsed.remove(path);
        app_state.rebuild();
        return ControlFlow::Break(());
    }

    if open {
        app_state.move_cursor(1);
        return ControlFlow::Break(());
    }

    if !collapsed {
        app_state.note_list.collapsed.insert(path.clone());
        app_state.rebuild();
        return ControlFlow::Break(());
    }

    ControlFlow::Continue(())
}

fn threads_key_name(app_state: &mut App, key: KeyEvent) {
    if let KeyCode::Char('c') = key.code {
        app_state.toggle_context();
    } else {
        threads_key_name_name(app_state, key);
    }
}

fn threads_key_name_name(app_state: &mut App, key: KeyEvent) {
    let name = key_name(&key);

    if let Some(character_2) = app_state
        .commands()
        .0
        .into_iter()
        .find(|character_value| character_value.key == name)
    {
        app_state.run(character_2.act);
    }
}

fn rows_key_branch(app_state: &mut App) {
    if app_state.thread_list.tcursor == 0 {
        app_state.thread_list.tlist.focus_views();
    } else {
        app_state.thread_list.tcursor -= 1;
        app_state.preview.pscroll = 0;
    }
}

fn agents_key_body(app_state: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Esc | KeyCode::Enter => app_state.agent_list.afilter_editing = false,
        KeyCode::Up => {
            if let Some(i) = app_state.agent_step(0, -1) {
                app_state.agent_list.acursor = i;
            }
        }
        KeyCode::Down => {
            if let Some(i) = app_state.agent_step(0, 1) {
                app_state.agent_list.acursor = i;
            }
        }
        _ => {
            if matches!(app_state.agent_list.afilter_ta.key(key), Input::Handled) {
                app_state.sync_afilter();
            }
        }
    }

    app_state.preview.pscroll = 0;
}

fn fold_parent(app_state: &mut App, parent: &str) {
    let parent = format!("{parent}/");

    app_state.note_list.collapsed.insert(parent.clone());
    app_state.rebuild();

    if let Some(i) = app_state
        .note_list
        .rows
        .iter()
        .position(|result| result.path() == parent)
    {
        app_state.note_list.cursor = i;
    }
}

fn notes_key_name(app_state: &mut App, key: KeyEvent) {
    let name = key_name(&key);

    if let Some(character_value) = app_state
        .commands()
        .0
        .into_iter()
        .find(|character| character.key == name)
    {
        app_state.run(character_value.act);
    }
}

fn threads_key_body(app_state: &mut App, before: &(usize, crate::features::thread_list::View, String)) {
    app_state.preview.pscroll = 0;

    if before.1 != app_state.thread_list.tlist.view || before.2 != app_state.thread_list.tlist.query() {
        app_state.thread_list.tcursor = 0;
    }
}

fn agents_key_name(app_state: &mut App, key: KeyEvent) {
    let name = key_name(&key);

    if let Some(character_value) = app_state
        .commands()
        .0
        .into_iter()
        .find(|character| character.key == name)
    {
        app_state.run(character_value.act);
    }
}

fn rows_key_branch_state(app_state: &mut App, count: usize) {
    if app_state.thread_list.tcursor + 1 < count {
        app_state.thread_list.tcursor += 1;
        app_state.preview.pscroll = 0;
    }
}
