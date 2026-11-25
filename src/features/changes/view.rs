use super::{Act, KeyCode, KeyEvent, Modal, View};
#[cfg(test)]
use super::{Changes, FileDiff, Instant, order};

impl View {
    /// Tests: FILES as the read changes (a git read on its way: dropped).
    #[cfg(test)]
    pub(crate) fn set_files(&mut self, mut files: Vec<FileDiff>) {
        order(&mut files);
        self.changes = Changes {
            base: "origin/main".into(),
            files,
            ..Default::default()
        };
        self.marks.clear();
        self.loading = None;
        self.loaded = Some(Instant::now());
        self.layout.built = None;
    }

    pub(crate) fn at_top(&self) -> bool {
        if self.modal_state.modal.is_some() || self.filter_state.filtering {
            return false;
        }

        if self.scm.focus {
            return panel_at_top(self);
        }

        if let Some(peek) = &self.peek {
            return peek.at_top();
        }

        self.at == 0
    }

    /// The mode the keys are in, for the footer's badge: the filter takes the letters, or lines
    /// are selected.
    pub(crate) fn mode(&self) -> Option<&'static str> {
        if self.filter_state.filtering {
            Some("filter")
        } else if !self.selection_state.sel.is_empty() {
            Some("select")
        } else {
            None
        }
    }

    /// The view has the letters: the filter, a modal, the commit message.
    pub(crate) fn typing(&self) -> bool {
        let peek = !self.scm.focus && self.peek.as_ref().is_some_and(|path| path.typing());

        peek || self.filter_state.filtering
            || self.modal_state.modal.is_some()
            || (self.scm.focus && self.scm.msg_focus)
    }

    /// Esc is the view's: it closes a modal, leaves the filter or the message, clears the
    /// filter, the selection or the panel's picks, closes the commit in the review's place.
    pub(crate) fn takes_esc(&self) -> bool {
        let picks = (self.scm.focus && (!self.scm.picked.is_empty() || !self.scm.commit_picks.is_empty()))
            || !self.scm.comment_picks.is_empty();
        let peek = !self.scm.focus && self.peek.is_some();

        self.typing()
            || !self.filter_state.filter.is_empty()
            || !self.selection_state.sel.is_empty()
            || picks
            || peek
            || self.scm.picking()
    }

    pub(crate) fn hints(&self) -> &'static str {
        hints_view(self)
    }

    /// The filter's text (the input along).
    pub(super) fn set_filter(&mut self, thread: &str) {
        self.filter_state.filter = thread.to_string();

        if self.filter_state.filter_ta.text() != thread {
            self.filter_state.filter_ta.set_text(thread);
        }
    }

    /// A key while the filter has the focus.
    pub(super) fn filter_key(&mut self, key: KeyEvent) -> Act {
        filter_key_view(self, key)
    }
}

const COMMENT_MODAL_HINT: &str = "ctrl+⏎: save · @agent: it gets the comment at once · esc: close (edited: esc esc) · alt+↑↓ / wheel: scroll · 0 text: delete ";

const CONFIRM_MODAL_HINT: &str = "y / enter: yes · n / esc: no ";

const FILTER_HINT: &str = "type to filter · enter: 1st hit · esc: clear ";

const BRANCH_HINT: &str = "all changes · j k: file · c: comment · C: comments · space: pick · v: reviewed / resolve · esc: back · ←: panel · ctrl+.: actions ";

const READ_ONLY_HINT: &str =
    "commit, read only · j k: file · enter: fold · ctrl+f: filter · w: side by side · esc: back · ←: panel ";

const COMMIT_HINT: &str =
    "j k · space: pick · enter: show · P: push (picked: up to) · [ ]: sections · ctrl+.: uncommit, ship ";

const COMMIT_FILE_HINT: &str = "j k · enter / →: the file in the review · ←: the commit · [ ]: sections ";

const PULL_REQUEST_PANEL_HINT: &str =
    "j k · enter on the commit: pick a commit · →: open the file · e: IDE · tab: next ";

const PULL_REQUEST_HINT: &str =
    "j k: file · space: select · c: comment · v: reviewed · ←: panel · e: IDE · tab: next · ctrl+.: actions ";

const MESSAGE_HINT: &str =
    "commit message · ctrl+⏎: commit the staged files · ctrl+g: draft by model · ↓ pgdn: list · esc: back ";

const PANEL_HINT: &str = "tab: next · ctrl+.: actions, ship ";

const COMMENT_HINT: &str = "comment · enter: unfold / edit · c: reply · v: resolve · C: fold · d d: delete · ctrl+⏎: send the batch · ctrl+.: actions ";

const REVIEW_HINT: &str =
    "j k: file · space: select · s: stage · c: comment · v: reviewed · e: IDE · tab: next · ctrl+.: actions ";

fn peek_hints(view: &View) -> &'static str {
    if view.filter_state.filtering {
        return FILTER_HINT;
    }

    if view.all {
        return BRANCH_HINT;
    }

    READ_ONLY_HINT
}

fn pull_request_hints(panel: bool, filter: bool) -> &'static str {
    if panel {
        return PULL_REQUEST_PANEL_HINT;
    }

    if filter {
        return FILTER_HINT;
    }

    PULL_REQUEST_HINT
}

fn ordinary_hints(view: &View) -> &'static str {
    if view.scm.focus && view.scm.msg_focus {
        return MESSAGE_HINT;
    }

    if view.scm.focus {
        return PANEL_HINT;
    }

    if view.filter_state.filtering {
        return FILTER_HINT;
    }

    if view.row_comment(view.at).is_some() {
        return COMMENT_HINT;
    }

    REVIEW_HINT
}

fn panel_at_top(view: &View) -> bool {
    if view.scm.pick.is_some() {
        return false;
    }

    if view.scm.msg_focus {
        return view.scm.msg.on_first_row();
    }

    view.scm.at == 0 && view.pr_src.is_some()
}

fn hints_view(view: &View) -> &'static str {
    match &view.modal_state.modal {
        Some(Modal::Comment { .. }) => return COMMENT_MODAL_HINT,
        Some(Modal::Confirm { .. }) => return CONFIRM_MODAL_HINT,
        None => {}
    }

    if let Some(peek) = view.peek.as_ref().filter(|_| !view.scm.focus) {
        return peek_hints(peek);
    }

    if view.scm.focus && !view.scm.msg_focus && view.scm.on_commit() {
        return COMMIT_HINT;
    }

    if view.scm.focus && !view.scm.msg_focus && view.scm.on_commit_file() {
        return COMMIT_FILE_HINT;
    }

    if view.pr_src.is_some() {
        return pull_request_hints(view.scm.focus, view.filter_state.filtering);
    }

    ordinary_hints(view)
}

fn filter_key_view(view: &mut View, key: KeyEvent) -> Act {
    match key.code {
        KeyCode::Esc => {
            view.filter_state.filtering = false;
            view.set_filter("");
        }
        KeyCode::Enter => {
            view.filter_state.filtering = false;
            view.rebuild_here();

            if let Some(height) = view.layout.rows.iter().position(|result| result.head) {
                view.at = height;
                view.top = 0;
            }

            return Act::None;
        }
        // the input: cursor, words, selection, undo (a text input as the others)
        _ => {
            if !matches!(
                view.filter_state.filter_ta.key(key),
                crate::ui::textarea::Outcome::Handled
            ) {
                return Act::None;
            }

            let thread = view.filter_state.filter_ta.text();

            if thread == view.filter_state.filter {
                return Act::None;
            }

            view.filter_state.filter = thread;
        }
    }

    view.layout.built = None;
    view.at = 0;
    view.top = 0;
    Act::None
}
