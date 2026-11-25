use super::{App, Field, Outcome, Screen, Session, Tab, Target, claude};

impl App {
    pub(super) fn open_compose(&mut self) {
        self.compose.screen = Screen::Compose;
        self.tabs_focus = false;
        self.compose.field = Field::Prompt;
    }

    /// Enter on the task's notes field: the notes picker.
    pub(super) fn open_notes(&mut self) {
        self.compose.screen = Screen::Pick;
        self.tab = Tab::Notes;
        self.tabs_focus = false;
        self.preview.pscroll = 0;
    }

    /// Claude Code session → task: its dir, its title, its last response as context.
    pub(super) fn pick_session(&mut self, session: Session) {
        if self.compose.title.is_empty() {
            self.compose.title.set_text(&session.title);
        }

        self.compose.target = Some(Target::Dir(session.cwd.clone()));
        self.toast(format!("context: last response of {}", session.title), false);

        if !self.compose.carry.iter().any(|carried| carried.key() == session.key()) {
            self.compose.carry.push(session);
        }

        self.open_compose();
    }

    /// c on a threads row: its last response in / out of the context. Rows without a transcript: toast.
    pub(super) fn toggle_context(&mut self) {
        let session = if let Some(session) = self.current_session() {
            Some(session.clone())
        } else if let Some(thread) = self.current_wt().and_then(|worktree| worktree.thread.clone()) {
            match thread_context(self, &thread) {
                Ok(value) => value,
                Err(value) => return value,
            }
        } else {
            None
        };

        let Some(session) = session else {
            return self.toast("worktree without thread: 0 context", true);
        };

        if let Some(i) = self
            .compose
            .carry
            .iter()
            .position(|carried| carried.key() == session.key())
        {
            self.compose.carry.remove(i);
            self.toast(
                format!("context −: {} · {} left", session.title, self.compose.carry.len()),
                false,
            );
        } else {
            let title = session.title.clone();

            self.compose.carry.push(session);
            self.toast(
                format!("context +: {title} · {} in total", self.compose.carry.len()),
                false,
            );
        }

        if self.thread_list.tlist.view.cards() {
            next_card(self);
        } else if self.thread_list.tcursor + 1 < self.n_threads() {
            self.thread_list.tcursor += 1;
        }

        self.preview.pscroll = 0;
    }

    pub(super) fn in_context(&self, key: &str) -> bool {
        self.compose.carry.iter().any(|carried| carried.key() == key)
    }

    pub(super) fn open_thread(&mut self) {
        if let Some(session) = self.current_session().cloned() {
            return self.pick_session(session);
        }

        let Some(worktree) = self.current_wt() else { return };

        if worktree.thread.is_some() {
            self.outcome = Some(Outcome::Resume(Box::new(worktree)));
        } else {
            self.compose.target = Some(Target::Worktree {
                path: worktree.path,
                branch: worktree.branch,
            });
            self.open_compose();
        }
    }

    /// Card C of the thread list opens (an archived thread: the open unarchives it, its host
    /// makes the worktree again).
    pub(super) fn open_card(&mut self, card: &crate::ui::thread_card::Card) {
        self.outcome = Some(Outcome::Resume(Box::new(self.wt_of_card(card))));
    }
}

fn thread_context(app: &mut App, thread: &crate::domain::threads::Thread) -> Result<Option<Session>, ()> {
    if let Some(i) = app
        .compose
        .carry
        .iter()
        .position(|carried| carried.thread.as_deref() == Some(&thread.id))
    {
        let removed = app.compose.carry.remove(i);

        return {
            app.toast(
                format!("context −: {} · {} left", removed.title, app.compose.carry.len()),
                false,
            );
            Err(())
        };
    }

    Ok(match claude::of_thread(&thread.id, &thread.title) {
        Some(session) => Some(session),
        None => {
            return {
                app.toast("0 Claude responses in this thread", true);
                Err(())
            };
        }
    })
}

fn next_card(app: &mut App) {
    if app.thread_list.tlist.cursor + 1 < app.thread_list.tlist.visible().len() {
        app.thread_list.tlist.cursor += 1;
    }
}
