use super::{Act, Body, Comment, FileDiff, Row, View, lkey, worktree_lines, write_marks};

impl View {
    /// Where the IDE opens: the 1st selected line (in the cursor's file, else the 1st file with
    /// a selection), else the cursor's line; a removed line: the worktree line it sat before.
    pub(super) fn open_spot(&self) -> Option<(String, u32)> {
        open_spot_view(self)
    }

    pub(super) fn file_at(&self) -> Option<&FileDiff> {
        self.layout
            .rows
            .get(self.at)
            .and_then(|row| self.changes.files.get(row.file))
    }

    pub(super) fn jump(&mut self, down: bool, pick: impl Fn(&Row) -> bool) {
        let count = self.layout.rows.len();

        let found = if down {
            (self.at + 1..count).find(|&i| pick(&self.layout.rows[i]))
        } else {
            (0..self.at).rev().find(|&i| pick(&self.layout.rows[i]))
        };

        if let Some(i) = found {
            self.at = i;
        }
    }

    pub(super) fn page(&self) -> usize {
        usize::from(self.layout.area.height.saturating_sub(2)).max(1)
    }

    /// Fold (ON) or open the box under the cursor; the cursor goes to its head.
    pub(super) fn fold(&mut self, on: bool) {
        let Some(file) = self.file_at() else { return };

        if self.folded(file) == on {
            self.rebuild_on_file();
            return;
        }

        let path = file.path.clone();

        if !self.toggled.remove(&path) {
            self.toggled.insert(path);
        }

        self.rebuild_on_file();
    }

    /// The keys to the panel / the review (the panel shows).
    pub(crate) fn toggle_focus(&mut self) {
        self.scm.shown = true;
        self.scm.focus = !self.scm.focus;
        self.scm.msg_focus = false;
    }

    /// The side panel shown / hidden (the context menu); hidden: the keys go to the review.
    pub(crate) fn toggle_panel(&mut self) {
        self.scm.shown = !self.scm.shown;
        self.scm.focus &= self.scm.shown;
        self.scm.msg_focus &= self.scm.shown;
    }

    /// The panel shows (tab walks to it).
    pub(crate) fn panel_shown(&self) -> bool {
        self.scm.shown
    }

    /// The keys to the panel (ON; it shows) or to the review.
    pub(crate) fn focus_panel(&mut self, on: bool) {
        if on {
            self.scm.shown = true;
        }

        self.scm.focus = on;
        self.scm.msg_focus = false;
    }

    /// The keys where FROM (another checkout's changes) had them: the panel, its commit message.
    pub(crate) fn focus_like(&mut self, view: &View) {
        if view.panel_focus() {
            self.focus_panel(true);
            self.scm.msg_focus = view.scm.msg_focus;
        }
    }

    /// `d`: the 1st press arms the delete of row AT (PANEL: of the panel); true: the 2nd press.
    pub(crate) fn del_armed(&mut self, panel: bool, at: usize) -> bool {
        if self.del_prev == Some((panel, at)) {
            return true;
        }

        self.del = Some((panel, at));
        false
    }

    /// The source control panel has the keys.
    pub(crate) fn panel_focus(&self) -> bool {
        self.scm.shown && self.scm.focus
    }

    /// The rows change (a comment): the cursor stays on its row number.
    pub(super) fn rebuild_here(&mut self) {
        let width = self.layout.built.map_or(
            if self.layout.built_w > 0 {
                self.layout.built_w
            } else {
                self.layout.area.width.saturating_sub(1)
            },
            |built| built.0,
        );
        let at = self.at;

        self.layout.built = None;
        self.build(width);
        self.at = at.min(self.layout.rows.len().saturating_sub(1));
    }

    /// Comment CI folded / unfolded: the cursor on its head row.
    pub(super) fn rebuild_on_comment(&mut self, ci: usize) {
        self.rebuild_here();

        if let Some(row) = self.layout.rows.iter().position(|row| row.comment == Some(ci)) {
            self.at = row;
        }
    }

    /// Enter / a click on comment row I: a folded comment unfolds (its whole thread), an unfolded
    /// one names the keys (a click: edits the row's text).
    pub(super) fn open_comment_row(&mut self, i: usize, click: bool) -> Act {
        open_comment_row_view(self, i, click)
    }

    /// The rows change (fold, layout): the cursor stays on its file's header.
    pub(super) fn rebuild_on_file(&mut self) {
        rebuild_on_file_view(self);
    }

    /// The finding under the cursor into the comment batch (ctrl+enter sends it).
    pub(super) fn take_finding(&mut self) -> Act {
        take_finding_view(self)
    }

    /// The classifier again (setting `classify`).
    pub(super) fn classify_again(&mut self) -> Act {
        if self.classifier.classifier.is_none() {
            return Act::Toast("classify: off (setting classify)".into());
        }

        self.apply_rank(true);
        self.layout.built = None;
        Act::Toast("classifying again…".into())
    }

    /// r: the box under the cursor reviewed / not; reviewed: the cursor goes to the next box.
    pub(super) fn toggle_reviewed(&mut self) -> Act {
        toggle_reviewed_view(self)
    }

    /// The row index under screen row Y of the rows' area (pinned rows included).
    pub(super) fn row_at_y(&self, y: u16) -> Option<usize> {
        self.layout
            .shown_rows
            .get(usize::from(y.checked_sub(self.layout.area.y)?))
            .copied()
    }
}

fn open_spot_view(view: &View) -> Option<(String, u32)> {
    let cur = view.file_at();

    let sel_file = cur
        .filter(|file| {
            view.selection_state
                .sel
                .get(&file.path)
                .is_some_and(|keys| !keys.is_empty())
        })
        .or_else(|| {
            let mut seen = std::collections::HashSet::new();

            view.layout
                .rows
                .iter()
                .filter(|row| seen.insert(row.file))
                .filter_map(|row| view.changes.files.get(row.file))
                .find(|file| {
                    view.selection_state
                        .sel
                        .get(&file.path)
                        .is_some_and(|keys| !keys.is_empty())
                })
        });

    if let Some(file) = sel_file {
        let keys = &view.selection_state.sel[&file.path];
        let first = file
            .hunks
            .iter()
            .flat_map(|hunk| worktree_lines(hunk).into_iter().zip(&hunk.lines))
            .filter(|(_, line)| lkey(line).is_some_and(|key| keys.contains(&key)))
            .map(|(worktree_line, _)| worktree_line)
            .min();

        return Some((file.path.clone(), first.unwrap_or_else(|| file.first_line()).max(1)));
    }

    let (file, row) = (cur?, view.layout.rows.get(view.at)?);

    let line = match row.body {
        Body::Diff(hunk, line) => worktree_lines(&file.hunks[hunk]).get(line).copied(),
        Body::Side(hunk, removed, added) => added
            .or(removed)
            .and_then(|line| worktree_lines(&file.hunks[hunk]).get(line).copied()),
        _ => row.at,
    };

    Some((file.path.clone(), line.unwrap_or_else(|| file.first_line()).max(1)))
}

fn open_comment_row_view(view: &mut View, i: usize, click: bool) -> Act {
    let Some((ci, reply)) = view.row_comment(i) else {
        return Act::None;
    };

    let Some(comment) = view.comment_state.comments.get(ci) else {
        return Act::None;
    };

    if view.comment_shut(comment) {
        let id = comment.id.clone();

        view.flip_comment(&id);
        view.rebuild_on_comment(ci);
        return Act::None;
    }

    if click {
        view.edit_comment(ci, reply);
        return Act::None;
    }

    Act::Toast("c: reply · e: edit · C: fold · d: resolve · ctrl+⏎: send the batch".into())
}

fn rebuild_on_file_view(view: &mut View) {
    let file = view.layout.rows.get(view.at).map(|row| row.file);
    let width = view.layout.built.map_or(
        if view.layout.built_w > 0 {
            view.layout.built_w
        } else {
            view.layout.area.width.saturating_sub(1)
        },
        |built| built.0,
    );

    view.layout.built = None;
    view.build(width);

    if let Some(i) = file
        && let Some(row) = view.layout.rows.iter().position(|row| row.head && row.file == i)
    {
        view.at = row;
    }
}

fn take_finding_view(view: &mut View) -> Act {
    if let Some(fi) = view.layout.rows.get(view.at).and_then(|row| row.finding)
        && let Some(finding) = view
            .reviewer
            .review
            .as_ref()
            .and_then(|review| review.findings.get(fi))
            .cloned()
    {
        view.drop_finding(fi);
        let level = crate::domain::changes::rank::Level::parse(&finding.severity).unwrap_or_default();

        view.comment_state.comments.push(Comment {
            path: finding.file.clone(),
            line: finding.line,
            text: crate::features::changes::review::as_comment(&finding),
            level,
            ..Default::default()
        });
        view.save_comments();
        view.rebuild_here();
        return Act::Toast(format!(
            "finding → comment batch · {} · ctrl+enter: send",
            view.comment_state.comments.len()
        ));
    }

    Act::None
}

fn toggle_reviewed_view(view: &mut View) -> Act {
    let Some(file) = view.file_at().cloned() else {
        return Act::None;
    };

    let on = !view.is_reviewed(&file);

    if on {
        view.marks.insert(file.path.clone(), file.hash);
    } else {
        view.marks.remove(&file.path);
    }

    view.toggled.remove(&file.path);
    write_marks(&view.marks_id, &view.marks);
    view.rebuild_on_file();

    if on {
        view.jump(true, |row| row.head);
    }

    let (done, all) = view.progress();

    Act::Toast(format!(
        "{} · {done}/{all} reviewed",
        if on { "reviewed" } else { "not reviewed" }
    ))
}
