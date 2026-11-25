use super::{Act, KeyCode, KeyEvent, KeyModifiers, Op, Outcome, PRow, View, patch};

impl View {
    /// The review's cursor on file I's head (the filter clears when it hides the file).
    pub(in crate::features::changes) fn jump_file(&mut self, i: usize) {
        jump_file_view(self, i);
    }

    /// The review's cursor on comment CI (its box opens).
    pub(in crate::features::changes) fn jump_comment(&mut self, ci: usize) {
        let Some(path) = self.comment_path(ci) else { return };

        let Some(i) = self.changes.files.iter().position(|file| file.path == path) else {
            return;
        };

        if self.folded(&self.changes.files[i]) && !self.toggled.remove(&path) {
            self.toggled.insert(path);
        }

        self.layout.built = None;
        self.jump_file(i);

        if let Some(row) = self.layout.rows.iter().position(|result| result.comment == Some(ci)) {
            self.at = row;
            self.top = row.saturating_sub(3);
        }
    }

    /// s: the staged section's rows unstage, the others stage.
    pub(super) fn panel_stage(&mut self) -> Act {
        let ps = targets(self);

        if ps.is_empty() {
            return Act::Toast("0 files · space: pick files".into());
        }

        let unstage = matches!(self.scm.rows.get(self.scm.at), Some(PRow::File(_, true)));

        self.scm.picked.clear();
        self.start_op(if unstage {
            Op::UnstageFiles(ps)
        } else {
            Op::StageFiles(ps)
        })
    }

    /// p: whole files into the stash.
    pub(super) fn panel_stash(&mut self) -> Act {
        panel_stash_view(self)
    }

    /// Restore stash entry ID: its agent gets the patch.
    pub(in crate::features::changes) fn restore(&mut self, id: &str) -> Act {
        self.stash_state.stash = crate::features::changes::stash::read(&self.id);

        let Some(error_value) = self.stash_state.stash.iter_mut().find(|error| error.id == id) else {
            return Act::None;
        };

        error_value.restoring = true;
        let msg = crate::features::changes::stash::restore_ask(error_value);
        let who = error_value.agent.clone().unwrap_or_else(|| "the lead".into());

        crate::features::changes::stash::write(&self.id, &self.stash_state.stash);
        self.stash_state.stash_at = crate::features::changes::stash::stamp(&self.id);
        self.operation_state.outbox.push(msg);

        Act::Toast(format!("restore → {who}"))
    }

    /// m: the commit message takes the keys (the panel opens).
    pub(in crate::features::changes) fn focus_message(&mut self) {
        self.scm.shown = true;
        self.scm.focus = true;
        self.scm.msg_focus = true;
    }

    /// A key for the commit message.
    pub(super) fn msg_key(&mut self, key_value: KeyEvent) -> Act {
        msg_key_view(self, key_value)
    }
}

/// The panel's picked files, else the cursor's file.
fn targets(view: &View) -> Vec<String> {
    let known: Vec<String> = view
        .changes
        .files
        .iter()
        .map(|file| file.path.clone())
        .filter(|path_value| view.scm.picked.contains(path_value))
        .collect();

    if !known.is_empty() {
        return known;
    }

    match view.scm.rows.get(view.scm.at) {
        Some(PRow::File(i, _)) => view
            .changes
            .files
            .get(*i)
            .map(|file_value| vec![file_value.path.clone()])
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

fn commit(view: &mut View) -> Act {
    let msg = view.scm.msg.text().trim().to_string();

    if msg.is_empty() {
        return Act::Toast("0 message · ctrl+g: draft one".into());
    }

    if !view.has_staged() {
        return Act::Toast("0 staged files · s: stage".into());
    }

    view.scm.msg.set_text("");
    view.scm.msg_focus = false;

    // the keys back to the review: P (push), O (PR) work at once
    view.scm.focus = false;
    view.scm.drafting = None;

    view.start_op(Op::Commit(msg))
}

fn draft(view: &mut View) -> Act {
    if view.scm.drafting.is_some() {
        return Act::None;
    }

    if !view.has_staged() {
        return Act::Toast("0 staged files: 0 draft".into());
    }

    let (tx, rx) = crate::infra::bus::channel();
    let dir = view.dir.clone();
    let model = crate::infra::config::rank_model().unwrap_or_else(|| "haiku".into());

    std::thread::spawn(move || {
        let _ = tx.send(crate::features::changes::draft_message(&dir, &model));
    });

    view.scm.drafting = Some(rx);

    Act::Toast("drafting the commit message…".into())
}

/// The keys from the commit message to list row AT (clamped).
fn leave_message(at: usize, view: &mut View) {
    view.scm.msg_focus = false;
    view.sync_rows();
    view.scm.at = at.min(view.scm.rows.len().saturating_sub(1));
}

fn jump_file_view(view: &mut View, i: usize) {
    // a commit in the review's place: the changes come back
    view.peek = None;

    if !view.layout.rows.iter().any(|result| result.head && result.file == i) && !view.filter_state.filter.is_empty() {
        view.filter_state.filter.clear();
        view.layout.built = None;
    }

    view.rebuild_here();

    if let Some(height) = view
        .layout
        .rows
        .iter()
        .position(|result_value| result_value.head && result_value.file == i)
    {
        view.at = height;
        view.top = height.saturating_sub(1);
    }
}

fn panel_stash_view(view: &mut View) -> Act {
    let ps = targets(view);

    if ps.is_empty() {
        return Act::Toast("0 files · space: pick files".into());
    }

    view.scm.picked.clear();

    let items = ps
        .into_iter()
        .map(|path| {
            (
                path,
                String::new(),
                patch::Pick {
                    all: true,
                    ..Default::default()
                },
            )
        })
        .collect();

    view.stash_op(items)
}

fn msg_key_view(view: &mut View, key_value: KeyEvent) -> Act {
    let send = key_value
        .modifiers
        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER);

    match key_value.code {
        KeyCode::Esc => view.scm.msg_focus = false,
        KeyCode::Enter if send => return commit(view),
        // ↓ on the last row: the list's 1st row; page down: its last row
        KeyCode::Down if !send && view.scm.msg.on_last_row() => leave_message(0, view),
        KeyCode::PageDown => leave_message(usize::MAX, view),
        KeyCode::Char('g') if key_value.modifiers.contains(KeyModifiers::CONTROL) => return draft(view),
        _ => {
            if !matches!(view.scm.msg.key(key_value), Outcome::Handled) && key_value.code == KeyCode::Enter {
                view.scm.msg.newline();
            }
        }
    }

    Act::None
}
