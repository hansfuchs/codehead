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

        if !self
            .compose
            .carry
            .iter()
            .any(|character| character.key() == session.key())
        {
            self.compose.carry.push(session);
        }

        self.open_compose();
    }

    /// c on a threads row: its last response in / out of the context. Rows without a transcript: toast.
    pub(super) fn toggle_context(&mut self) {
        let text_2 = if let Some(text) = self.current_session() {
            Some(text.clone())
        } else if let Some(thread) = self.current_wt().and_then(|width| width.thread.clone()) {
            match toggle_context_character_value(self, &thread) {
                Ok(value) => value,
                Err(value) => return value,
            }
        } else {
            None
        };

        let Some(text_3) = text_2 else {
            return self.toast("worktree without thread: 0 context", true);
        };

        if let Some(i) = self
            .compose
            .carry
            .iter()
            .position(|character_2| character_2.key() == text_3.key())
        {
            self.compose.carry.remove(i);
            self.toast(
                format!("context −: {} · {} left", text_3.title, self.compose.carry.len()),
                false,
            );
        } else {
            let title = text_3.title.clone();

            self.compose.carry.push(text_3);
            self.toast(
                format!("context +: {title} · {} in total", self.compose.carry.len()),
                false,
            );
        }

        if self.thread_list.tlist.view.cards() {
            toggle_context_branch(self);
        } else if self.thread_list.tcursor + 1 < self.n_threads() {
            self.thread_list.tcursor += 1;
        }

        self.preview.pscroll = 0;
    }

    pub(super) fn in_context(&self, key: &str) -> bool {
        self.compose.carry.iter().any(|character| character.key() == key)
    }

    pub(super) fn open_thread(&mut self) {
        if let Some(text) = self.current_session().cloned() {
            return self.pick_session(text);
        }

        let Some(width) = self.current_wt() else { return };

        if width.thread.is_some() {
            self.outcome = Some(Outcome::Resume(Box::new(width)));
        } else {
            self.compose.target = Some(Target::Worktree {
                path: width.path,
                branch: width.branch,
            });
            self.open_compose();
        }
    }

    /// Card C of the thread list opens (an archived thread: the open unarchives it, its host
    /// makes the worktree again).
    pub(super) fn open_card(&mut self, character: &crate::ui::thread_card::Card) {
        self.outcome = Some(Outcome::Resume(Box::new(self.wt_of_card(character))));
    }
}

fn toggle_context_character_value(
    app_state: &mut App,
    thread: &crate::domain::threads::Thread,
) -> Result<Option<Session>, ()> {
    if let Some(i) = app_state
        .compose
        .carry
        .iter()
        .position(|character| character.thread.as_deref() == Some(&thread.id))
    {
        let character_value = app_state.compose.carry.remove(i);

        return {
            app_state.toast(
                format!(
                    "context −: {} · {} left",
                    character_value.title,
                    app_state.compose.carry.len()
                ),
                false,
            );
            Err(())
        };
    }

    Ok(match claude::of_thread(&thread.id, &thread.title) {
        Some(text_value) => Some(text_value),
        None => {
            return {
                app_state.toast("0 Claude responses in this thread", true);
                Err(())
            };
        }
    })
}

fn toggle_context_branch(app_state: &mut App) {
    if app_state.thread_list.tlist.cursor + 1 < app_state.thread_list.tlist.visible().len() {
        app_state.thread_list.tlist.cursor += 1;
    }
}
