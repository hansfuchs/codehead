use super::{Act, App, Field, Input, KeyCode, KeyEvent, Outcome, Path, Row, Screen, key_name};
use std::ops::ControlFlow;

impl App {
    pub(super) fn move_cursor(&mut self, delta: isize) {
        let count = self.note_list.rows.len();

        if count == 0 {
            return;
        }

        let cursor = (self.note_list.cursor as isize + delta).clamp(0, count as isize - 1) as usize;

        if cursor != self.note_list.cursor {
            self.note_list.cursor = cursor;
            self.preview.pscroll = 0;
        }
    }

    pub(super) fn fold(&mut self, open: bool) {
        let Some(row) = self.current().cloned() else { return };

        if let Row::Dir { path, collapsed, .. } = &row
            && fold_dir(self, open, path, *collapsed).is_break()
        {
            return;
        }

        if open || self.note_list.filter.is_some() {
            return;
        }

        // ← on a file / closed folder: fold the parent, cursor onto it
        let path = row.path().trim_end_matches('/');

        if let Some((parent, _)) = path.rsplit_once('/') {
            fold_parent(self, parent);
        }
    }

    pub(super) fn notes_key(&mut self, key: KeyEvent) {
        if self.note_list.filter_editing {
            notes_filter_key(self, key);
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
                run_note_command(self, key);
            }
        }
    }

    /// Repo of threads tab item I (worktrees ++ sessions).
    pub(super) fn trepo_of(&self, i: usize) -> Option<&Path> {
        let worktrees = self.thread_list.worktrees.as_deref().unwrap_or(&[]);

        match worktrees.get(i) {
            Some(worktree) => worktree.repo.as_deref(),
            None => self
                .thread_list
                .sessions
                .get(i - worktrees.len())
                .and_then(|session| session.repo.as_deref()),
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
            threads_view_changed(self, &before);
        }

        match out {
            Out::None | Out::Home => {}
            Out::Up => self.tabs_focus = true,
            Out::Open(card) => self.open_card(&card),
            Out::Pass => {
                threads_other_key(self, key);
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
                rows_up(self);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                rows_down(self, count);
            }
            KeyCode::Enter => self.open_thread(),
            _ => return self.thread_list.tlist.key(key),
        }

        Out::None
    }

    pub(super) fn agents_key(&mut self, key: KeyEvent) {
        if self.agent_list.afilter_editing {
            agents_filter_key(self, key);
            return;
        }

        let step = |app: &mut Self, i: Option<usize>| {
            select_agent(app, i);
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
                run_agent_command(self, key);
            }
        }
    }

    pub(super) fn toggle_pick(&mut self) {
        let Some(agent) = self.agent_list.agents.get(self.compose.pick_cursor) else {
            return;
        };

        let name = agent.name.clone();

        if let Some(i) = self.compose.picks.iter().position(|pick| *pick == name) {
            self.compose.picks.remove(i);
        } else {
            self.compose.picks.push(name);
        }
    }
}

fn select_agent(app: &mut App, i: Option<usize>) {
    if let Some(i) = i {
        app.agent_list.acursor = i;
        app.preview.pscroll = 0;
    }
}

fn notes_filter_key(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Esc => {
            app.note_list.filter = None;
            app.note_list.filter_editing = false;
            app.rebuild();
        }
        KeyCode::Enter => app.note_list.filter_editing = false,
        KeyCode::Up => app.move_cursor(-1),
        KeyCode::Down => app.move_cursor(1),
        // backspace on an empty filter: filter closed
        KeyCode::Backspace if app.note_list.filter_ta.is_empty() => {
            app.note_list.filter = None;
            app.note_list.filter_editing = false;
            app.note_list.cursor = 0;
            app.rebuild();
        }
        _ => {
            if matches!(app.note_list.filter_ta.key(key), Input::Handled) {
                app.sync_filter();
            }
        }
    }
}

fn fold_dir(app: &mut App, open: bool, path: &String, collapsed: bool) -> ControlFlow<()> {
    if open && collapsed {
        app.note_list.collapsed.remove(path);
        app.rebuild();
        return ControlFlow::Break(());
    }

    if open {
        app.move_cursor(1);
        return ControlFlow::Break(());
    }

    if !collapsed {
        app.note_list.collapsed.insert(path.clone());
        app.rebuild();
        return ControlFlow::Break(());
    }

    ControlFlow::Continue(())
}

fn threads_other_key(app: &mut App, key: KeyEvent) {
    if let KeyCode::Char('c') = key.code {
        app.toggle_context();
    } else {
        run_thread_command(app, key);
    }
}

fn run_thread_command(app: &mut App, key: KeyEvent) {
    let name = key_name(&key);

    if let Some(command) = app.commands().0.into_iter().find(|command| command.key == name) {
        app.run(command.act);
    }
}

fn rows_up(app: &mut App) {
    if app.thread_list.tcursor == 0 {
        app.thread_list.tlist.focus_views();
    } else {
        app.thread_list.tcursor -= 1;
        app.preview.pscroll = 0;
    }
}

fn agents_filter_key(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Esc | KeyCode::Enter => app.agent_list.afilter_editing = false,
        KeyCode::Up => {
            if let Some(i) = app.agent_step(0, -1) {
                app.agent_list.acursor = i;
            }
        }
        KeyCode::Down => {
            if let Some(i) = app.agent_step(0, 1) {
                app.agent_list.acursor = i;
            }
        }
        _ => {
            if matches!(app.agent_list.afilter_ta.key(key), Input::Handled) {
                app.sync_afilter();
            }
        }
    }

    app.preview.pscroll = 0;
}

fn fold_parent(app: &mut App, parent: &str) {
    let parent = format!("{parent}/");

    app.note_list.collapsed.insert(parent.clone());
    app.rebuild();

    if let Some(i) = app.note_list.rows.iter().position(|row| row.path() == parent) {
        app.note_list.cursor = i;
    }
}

fn run_note_command(app: &mut App, key: KeyEvent) {
    let name = key_name(&key);

    if let Some(command) = app.commands().0.into_iter().find(|command| command.key == name) {
        app.run(command.act);
    }
}

fn threads_view_changed(app: &mut App, before: &(usize, crate::features::thread_list::View, String)) {
    app.preview.pscroll = 0;

    if before.1 != app.thread_list.tlist.view || before.2 != app.thread_list.tlist.query() {
        app.thread_list.tcursor = 0;
    }
}

fn run_agent_command(app: &mut App, key: KeyEvent) {
    let name = key_name(&key);

    if let Some(command) = app.commands().0.into_iter().find(|command| command.key == name) {
        app.run(command.act);
    }
}

fn rows_down(app: &mut App, count: usize) {
    if app.thread_list.tcursor + 1 < count {
        app.thread_list.tcursor += 1;
        app.preview.pscroll = 0;
    }
}
