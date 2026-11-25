use super::{Agent, App, Duration, Field, Instant, Outcome, Row, Screen, Session, Use, Worktree, fuzzy, worktree_of};

impl App {
    pub(super) fn current_agent(&self) -> Option<&Agent> {
        self.agent_list.agents.get(self.agent_list.acursor)
    }

    pub(super) fn uses(&self, agent: &Agent) -> Vec<Use> {
        self.agent_list
            .usage
            .as_ref()
            .and_then(|usage| usage.get(&agent.name))
            .cloned()
            .unwrap_or_default()
    }

    pub(super) fn current(&self) -> Option<&Row> {
        self.note_list.rows.get(self.note_list.cursor)
    }

    pub(super) fn n_worktrees(&self) -> usize {
        self.thread_list.worktrees.as_ref().map_or(0, Vec::len)
    }

    /// Rows of the threads tab's own views (worktrees without thread, Claude Code sessions) after
    /// the repo tab and the filter: indices into worktrees ++ sessions, best match first. The
    /// card views: 0 (the thread list has them).
    pub(super) fn tvisible(&self) -> Vec<usize> {
        use crate::features::thread_list::View;
        let worktrees = self.thread_list.worktrees.as_deref().unwrap_or(&[]);
        let nw = worktrees.len();

        let items: Vec<usize> = match self.thread_list.tlist.view {
            View::Worktrees => (0..nw).filter(|&i| worktrees[i].thread.is_none()).collect(),
            View::Claude => (nw..nw + self.thread_list.sessions.len()).collect(),
            View::Active | View::Archived => return Vec::new(),
        };

        let query = self.thread_list.tlist.query().trim().to_lowercase();

        let name = |i: usize| -> String { visible_label(self, worktrees, nw, i) };

        let mut scored: Vec<(i64, usize)> = items
            .into_iter()
            .filter(|&i| self.thread_list.tlist.keeps(self.trepo_of(i)))
            .filter_map(|i| match_score(&query, name, i))
            .collect();

        scored.sort_unstable();

        scored.into_iter().map(|x| x.1).collect()
    }

    pub(super) fn n_threads(&self) -> usize {
        self.tvisible().len()
    }

    /// The row of a card: the full read of its thread (preview: changes, agents, task), else
    /// the card's data.
    pub(super) fn wt_of_card(&self, card: &crate::ui::thread_card::Card) -> Worktree {
        self.thread_list
            .worktrees
            .iter()
            .flatten()
            .find(|worktree| worktree.thread.as_ref().is_some_and(|thread| thread.id == card.id))
            .cloned()
            .unwrap_or_else(|| worktree_of(card))
    }

    /// The thread / worktree under the cursor (cards: the thread list's).
    pub(super) fn current_wt(&self) -> Option<Worktree> {
        if self.thread_list.tlist.view.cards() {
            return self.thread_list.tlist.current().map(|card| self.wt_of_card(card));
        }

        let i = *self.tvisible().get(self.thread_list.tcursor)?;

        self.thread_list.worktrees.as_ref()?.get(i).cloned()
    }

    pub(super) fn current_session(&self) -> Option<&Session> {
        let i = *self.tvisible().get(self.thread_list.tcursor)?;

        i.checked_sub(self.n_worktrees())
            .and_then(|j| self.thread_list.sessions.get(j))
    }

    pub(super) fn toggle_select(&mut self) {
        let Some(row) = self.current() else { return };
        let path = row.path().to_string();
        let inside = self.note_list.notes.inside(&path);

        if row.is_dir() {
            toggle_dir_selection(self, inside);
        } else if let Some(i) = self.note_list.selected.iter().position(|note| *note == path) {
            self.note_list.selected.remove(i);
        } else {
            self.note_list.selected.push(path);
        }
    }

    /// ctrl+c, everywhere: a selection in the focused input → copied. Else quit; with unsent text
    /// (title or prompt) only on a 2nd ctrl+c within 1s.
    pub(super) fn ctrl_c(&mut self) {
        let sel = match (self.compose.screen, self.compose.field) {
            (Screen::Compose, Field::Title) => self.compose.title.selected_text(),
            (Screen::Compose, Field::Prompt) => self.compose.prompt.selected_text(),
            _ => None,
        };

        if let Some(text) = sel {
            crate::infra::clip::copy(&text);
            return self.toast(format!("copied {} chars", text.chars().count()), false);
        }

        // as in the thread view: the field's text → cleared (ctrl+z brings it back)
        let field = match (self.compose.screen, self.compose.field) {
            (Screen::Compose, Field::Title) => Some(&mut self.compose.title),
            (Screen::Compose, Field::Prompt) => Some(&mut self.compose.prompt),
            _ => None,
        };

        if let Some(ta) = field
            && !ta.text().is_empty()
        {
            ta.load_text("");
            return self.toast("cleared · ctrl+z: back", false);
        }

        // then: quit on a 2nd ctrl+c within 1 s (the task stays as a draft)
        if self
            .overlays
            .last_ctrl_c
            .is_some_and(|last| last.elapsed() < Duration::from_secs(1))
        {
            self.outcome = Some(Outcome::Quit);
            return;
        }

        self.overlays.last_ctrl_c = Some(Instant::now());
        self.toast("ctrl+c again: quit", false);
    }

    pub(super) fn toast(&mut self, msg: impl Into<String>, err: bool) {
        self.overlays.toast = Some((msg.into(), Instant::now(), err));
    }

    /// Enter on the notes list: only the space-selected notes are the context (0 allowed).
    /// Copy mode (-c) with 0 selected: copies the row under the cursor.
    pub(super) fn confirm_notes(&mut self) {
        if self.cfg.copy_mode {
            confirm_notes_row(self);
            return;
        }

        self.compose.screen = Screen::Compose;
        self.compose.field = Field::Notes;
    }
}

fn match_score(query: &str, name: impl Fn(usize) -> String, i: usize) -> Option<(i64, usize)> {
    if query.is_empty() {
        Some((0, i))
    } else {
        fuzzy(query, &name(i)).map(|score| (-score, i))
    }
}

fn visible_label(app: &App, worktrees: &[Worktree], nw: usize, i: usize) -> String {
    if i < nw {
        worktrees[i].label()
    } else {
        let session = &app.thread_list.sessions[i - nw];

        format!("{} {}", session.title, session.place())
    }
}

fn toggle_dir_selection(app: &mut App, inside: Vec<String>) {
    if inside.iter().all(|note| app.note_list.selected.contains(note)) {
        app.note_list.selected.retain(|note| !inside.contains(note));
    } else {
        select_missing(app, inside);
    }
}

fn select_missing(app: &mut App, inside: Vec<String>) {
    for note in inside {
        if !app.note_list.selected.contains(&note) {
            app.note_list.selected.push(note);
        }
    }
}

fn confirm_notes_row(app: &mut App) {
    if app.note_list.selected.is_empty()
        && let Some(row) = app.current()
    {
        app.note_list.selected = app.note_list.notes.inside(row.path());
    }

    app.outcome = Some(Outcome::Copy(
        app.note_list
            .selected
            .iter()
            .map(|path| app.note_list.notes.abs(path))
            .collect(),
    ));
}
