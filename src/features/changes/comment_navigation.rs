#[cfg(test)]
use super::Spot;
use super::{Act, Body, CLine, CState, Comment, Duration, HashSet, Instant, View, threads};

impl View {
    /// Comment ID (a reply note's jump): unfolded, the cursor on it, once its file is read.
    pub(crate) fn show_comment(&mut self, id: &str) {
        self.comment_state.want_comment = Some(id.to_string());
        self.scm.focus = false;
        self.sync_comments();
        self.show_wanted();
    }

    /// The wanted comment, its file read: unfolded, the cursor on it.
    pub(super) fn show_wanted(&mut self) {
        show_wanted_view(self);
    }

    /// Unfolded comments with unseen agent replies on screen 1 s: seen (their notification goes).
    pub(super) fn see_replies(&mut self) {
        see_replies_view(self);
    }

    /// Comment C shows 1 row (folded): `C` folds all, enter / `C` on it flips it.
    pub(super) fn comment_shut(&self, comment: &Comment) -> bool {
        self.comment_state.comments_folded != self.comment_state.comment_flip.contains(&comment.id)
    }

    /// Comment ID shows its whole thread.
    pub(super) fn unfold_comment(&mut self, id: &str) {
        let shut = self
            .comment_state
            .comments
            .iter()
            .find(|character| character.id == id)
            .is_some_and(|comment| self.comment_shut(comment));

        if shut && !self.comment_state.comment_flip.remove(id) {
            self.comment_state.comment_flip.insert(id.to_string());
        }

        self.layout.built = None;
    }

    /// Comment ID folded / unfolded (the other way than now).
    pub(super) fn flip_comment(&mut self, id: &str) {
        if !self.comment_state.comment_flip.remove(id) {
            self.comment_state.comment_flip.insert(id.to_string());
        }

        self.layout.built = None;
    }

    /// The rows of comment C in the file view: its head; unfolded: its text, its replies.
    pub(super) fn clines(&self, comment: &Comment) -> Vec<CLine> {
        let mut value = vec![CLine::Head];

        if self.comment_shut(comment) {
            return value;
        }

        value.extend((1..comment.text.lines().count()).map(CLine::Text));

        for (result, x) in comment.replies.iter().enumerate() {
            value.push(CLine::Reply(result));
            value.extend((1..x.text.lines().count()).map(|key| CLine::RText(result, key)));
        }

        value
    }

    /// The comment of row I and its reply (None: the comment's own rows).
    pub(super) fn row_comment(&self, i: usize) -> Option<(usize, Option<usize>)> {
        let row = self.layout.rows.get(i)?;

        let Body::Comment(ci, comment) = row.body else {
            return None;
        };

        let comment_value = self.comment_state.comments.get(ci)?;

        Some(match self.clines(comment_value).get(comment) {
            Some(CLine::Reply(x) | CLine::RText(x, _)) => (ci, Some(*x)),
            _ => (ci, None),
        })
    }

    /// The open comments.
    pub(crate) fn comment_count(&self) -> usize {
        self.comment_state
            .comments
            .iter()
            .filter(|character| character.open())
            .count()
    }

    /// The prompt took the context (picked comments): the picks clear, 1 prompt each.
    pub(crate) fn took_context(&mut self) {
        self.scm.comment_picks.clear();

        if let Some(path) = self.peek.as_mut() {
            path.scm.comment_picks.clear();
        }
    }

    /// Comment CI open → resolved by the owner, resolved → open.
    pub(super) fn toggle_resolved(&mut self, ci: usize) -> Act {
        toggle_resolved_view(self, ci)
    }

    /// Add a comment TEXT on SPOTS (1 per file); the selection it was on clears; the rows show
    /// it at once.
    #[cfg(test)]
    pub(crate) fn add_comment(&mut self, spots: &[Spot], text: &str) {
        self.add_comment_at(spots, text, crate::domain::changes::rank::Level::default());
    }
}

fn show_wanted_view(view: &mut View) {
    let Some(id) = view.comment_state.want_comment.clone() else {
        return;
    };

    let Some(ci) = view
        .comment_state
        .comments
        .iter()
        .position(|character| character.id == id)
    else {
        return;
    };

    if view
        .comment_path(ci)
        .is_none_or(|path| !view.changes.files.iter().any(|file| file.path == path))
    {
        return;
    }

    view.comment_state.want_comment = None;
    view.unfold_comment(&id);
    view.jump_comment(ci);
}

fn see_replies_view(view: &mut View) {
    let mut on: HashSet<String> = HashSet::new();

    for &row in &view.layout.shown_rows {
        if let Some(Body::Comment(ci, _)) = view.layout.rows.get(row).map(|result| result.body)
            && let Some(comment) = view.comment_state.comments.get(ci)
            && threads::unseen(comment) > 0
            && !view.comment_shut(comment)
        {
            on.insert(comment.id.clone());
        }
    }

    view.comment_state.seen_at.retain(|id, _| on.contains(id));
    let now = Instant::now();

    let due: Vec<String> = on
        .into_iter()
        .filter(|id| {
            now.duration_since(*view.comment_state.seen_at.entry(id.clone()).or_insert(now)) >= Duration::from_secs(1)
        })
        .collect();

    for id in due {
        view.comment_state.seen_at.remove(&id);
        view.write_through(|thread| threads::mark_seen(thread, &id));
    }
}

fn toggle_resolved_view(view: &mut View, ci: usize) -> Act {
    let Some(comment) = view.comment_state.comments.get_mut(ci) else {
        return Act::None;
    };

    let open = comment.open();

    comment.state = if open { CState::Resolved } else { CState::Open };
    comment.resolved_by.clear();
    comment.note.clear();

    if !open {
        comment.sent = false;
    }

    let id = comment.id.clone();

    view.save_comments();
    view.rebuild_here();

    Act::Toast(format!("{id} {}", if open { "resolved" } else { "open again" }))
}
