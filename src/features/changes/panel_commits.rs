use super::{Act, Modal, Op, PRow, Then, View};
use std::fmt::Write as _;

impl View {
    /// The cursor to row I; a row of a running push (0 keys): past it, DOWN or up (0 other row
    /// that way: the other way).
    pub(super) fn move_to(&mut self, i: usize, down: bool) {
        let inert = |result: &PRow| matches!(result, PRow::Pushing(_) | PRow::PushingCommit(_));
        let rows = &self.scm.rows;

        if rows.get(i).is_none_or(|result_value| !inert(result_value)) {
            self.scm.at = i;
            return;
        }

        let fwd = (i..rows.len()).find(|&j| !inert(&rows[j]));
        let back = (0..i).rev().find(|&j| !inert(&rows[j]));

        if let Some(j) = if down { fwd.or(back) } else { back.or(fwd) } {
            self.scm.at = j;
        }
    }

    /// After a read: picks and the shown commit of commits that left the list go.
    pub(in crate::features::changes) fn sync_commits(&mut self) {
        let cs = &self.changes.commits;

        self.scm
            .commit_picks
            .retain(|output| cs.find(output).is_some_and(|character| !character.pushed));

        if let Some(output_value) = self.peeked()
            && cs.find(output_value).is_none()
        {
            self.peek = None;
        }
    }

    /// The review shows commit I of the list (STAY: the keys stay on the panel).
    pub(super) fn open_commit(&mut self, i: usize, stay: bool) {
        let Some(character) = self.changes.commits.list.get(i).cloned() else {
            return;
        };

        if self.peeked() != Some(character.oid.as_str()) {
            let mut path = View::for_commit(&self.dir, &character);

            path.refresh(true);
            self.peek = Some(Box::new(path));
        }

        self.scm.focus = stay;
        self.sync_rows();
    }

    /// The review's cursor on file FI of the shown commit.
    pub(super) fn open_commit_file(&mut self, ci: usize, fi: usize) {
        self.open_commit(ci, true);

        if let Some(path) = self.peek.as_mut() {
            path.jump_file(fi);
        }
    }

    /// The panel's rows again; a cursor on a commit or a commit's file stays on it (the files
    /// of another commit come and go above it), on its commit when the file left.
    pub(super) fn sync_rows(&mut self) {
        sync_rows_view(self);
    }

    /// The review shows the branch's changes against its base (STAY: the keys stay on the panel).
    pub(super) fn open_all(&mut self, stay: bool) {
        if !self.peek.as_ref().is_some_and(|path| path.all) {
            let mut path_value = View::for_branch(&self.id, &self.dir);

            path_value.refresh(true);
            self.peek = Some(Box::new(path_value));
        }

        self.scm.focus = stay;
    }

    /// u: the question, then the picked commits leave the history (their changes: uncommitted).
    pub(in crate::features::changes) fn ask_uncommit(&mut self) -> Act {
        ask_uncommit_view(self)
    }

    /// The uncommit runs (asked): the messages of the commits go to the commit message box.
    pub(in crate::features::changes) fn start_uncommit(&mut self, oids: Vec<String>, head: String) -> Act {
        start_uncommit_view(self, oids, head)
    }

    /// The newest picked unpushed commit, and how many unpushed commits a push up to it takes.
    pub(in crate::features::changes) fn push_pick(
        &self,
    ) -> Option<(&crate::features::changes::commits::Commit, usize)> {
        let cs = &self.changes.commits;
        let un: Vec<&crate::features::changes::commits::Commit> =
            cs.list.iter().filter(|character| !character.pushed).collect();
        let key = un
            .iter()
            .position(|character_value| self.scm.commit_picks.contains(&character_value.oid))?;

        Some((un[key], un.len() - key))
    }

    /// The ship section's "push up to": the question lists every commit that goes.
    pub(in crate::features::changes) fn ask_push_to(&mut self) -> Act {
        if self.pushing() {
            return Act::Toast(crate::features::changes::PUSH_RUNS.into());
        }

        let Some((top, _)) = self.push_pick() else {
            return Act::Toast("0 commits picked".into());
        };

        let oid = top.oid.clone();

        ask_push(oid, self)
    }

    /// P / the UNPUSHED row's push: every unpushed commit to the upstream, after the question
    /// (picks: up to the newest pick, as the ship section).
    pub(in crate::features::changes) fn ask_push_all(&mut self) -> Act {
        ask_push_all_view(self)
    }
}

/// The picked commits, else the cursor's commit: unpushed ones only (oids, list order).
fn commit_targets(view: &View) -> Vec<String> {
    let cs = &view.changes.commits;
    let picked: Vec<String> = cs
        .list
        .iter()
        .filter(|character| !character.pushed && view.scm.commit_picks.contains(&character.oid))
        .map(|character_value| character_value.oid.clone())
        .collect();

    if !picked.is_empty() {
        return picked;
    }

    match view.scm.rows.get(view.scm.at) {
        Some(PRow::Commit(i)) => cs
            .list
            .get(*i)
            .filter(|character_2| !character_2.pushed)
            .map(|character_3| vec![character_3.oid.clone()])
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

/// `abc1234 subject` per commit (oids), list order, at most 8 lines.
fn commit_lines(oids: &[String], view: &View) -> String {
    let cs = &view.changes.commits;
    let mut value: Vec<String> = cs
        .list
        .iter()
        .filter(|character| oids.contains(&character.oid))
        .map(|character_value| format!("{} {}", character_value.short, character_value.subject))
        .collect();

    if value.len() > 8 {
        let count = value.len() - 7;

        value.truncate(7);
        value.push(format!("… {count} more"));
    }

    value.join("\n")
}

/// The question of a push up to unpushed commit OID: it lists every commit that goes.
fn ask_push(oid: String, view: &mut View) -> Act {
    if view.pushing() {
        return Act::Toast(crate::features::changes::PUSH_RUNS.into());
    }

    let cs = &view.changes.commits;
    let un: Vec<String> = cs
        .list
        .iter()
        .filter(|character| !character.pushed)
        .map(|character_value| character_value.oid.clone())
        .collect();
    let key = un.iter().position(|output| *output == oid).unwrap_or(0);
    let going = un[key..].to_vec();
    let to = cs.upstream.clone().unwrap_or_else(|| "origin".into());
    let count = going.len();

    // the newer unpushed commits (above the pick) stay local
    let mut question = format!("push {count} commit{} to {to}?", if count == 1 { "" } else { "s" });

    if key > 0 {
        let _ = write!(question, " {key} newer stay local");
    }

    question.push('\n');
    question.push_str(&commit_lines(&going, view));

    // the list stops at 50: older unpushed commits past it go too
    if cs.more && cs.list.last().is_some_and(|character_2| !character_2.pushed) {
        question.push_str("\n+ older unpushed commits (not listed)");
    }

    view.modal_state.modal = Some(Modal::Confirm {
        question,
        yes: false,
        then: Then::PushTo(oid),
    });

    Act::None
}

fn sync_rows_view(view: &mut View) {
    let old = view.scm.rows.get(view.scm.at).cloned();

    view.scm.rows = view.panel_rows();

    let to = match old {
        Some(PRow::Commit(ci)) => view.scm.rows.iter().position(|result| *result == PRow::Commit(ci)),
        Some(PRow::CommitFile(ci, fi)) => view
            .scm
            .rows
            .iter()
            .position(|result_value| *result_value == PRow::CommitFile(ci, fi))
            .or_else(|| view.scm.rows.iter().position(|result_2| *result_2 == PRow::Commit(ci))),
        _ => None,
    };

    if let Some(i) = to {
        view.scm.at = i;
    }

    // the cursor's commit went into a running push: the next row it can be on
    view.move_to(view.scm.at, true);
}

fn ask_uncommit_view(view: &mut View) -> Act {
    if view.changes.stopped.is_some() {
        return Act::Toast("a rebase stopped: finish it first (S)".into());
    }

    let oids = commit_targets(view);

    if oids.is_empty() {
        let pushed = matches!(view.scm.rows.get(view.scm.at), Some(PRow::Commit(i)) if view.changes.commits.list.get(*i).is_some_and(|character| character.pushed));

        return Act::Toast(if pushed {
            "pushed: 0 uncommit".into()
        } else {
            "0 commits · space on a commit: pick it".into()
        });
    }

    let count = oids.len();
    let question = format!(
        "uncommit {count} commit{}? files stay, changes become uncommitted\n{}",
        if count == 1 { "" } else { "s" },
        commit_lines(&oids, view)
    );

    view.modal_state.modal = Some(Modal::Confirm {
        question,
        yes: false,
        then: Then::Uncommit(oids, view.changes.commits.head.clone()),
    });

    Act::None
}

fn start_uncommit_view(view: &mut View, oids: Vec<String>, head: String) -> Act {
    let cs = &view.changes.commits;

    // oldest first, as they were made
    let msgs: Vec<String> = cs
        .list
        .iter()
        .rev()
        .filter(|character| oids.contains(&character.oid))
        .map(|character_value| character_value.body.clone())
        .collect();
    let op = Op::Uncommit {
        thread: view.id.clone(),
        head,
        picks: oids,
    };
    let act = view.start_op(op);

    if view.operation_state.op.is_some() {
        view.operation_state.prefill = Some(msgs.join("\n\n"));
        view.scm.commit_picks.clear();
    }

    act
}

fn ask_push_all_view(view: &mut View) -> Act {
    if view.pushing() {
        return Act::Toast(crate::features::changes::PUSH_RUNS.into());
    }

    if view.pr_src.is_some() || view.commit_src.is_some() || view.all {
        return view.start_op(Op::Push);
    }

    if view.changes.stopped.is_some() {
        return Act::Toast("a rebase stopped: finish it first (S)".into());
    }

    let cs = &view.changes.commits;

    if cs.remote_only > 0 {
        return Act::Toast(format!("↓{} on the remote: fetch + rebase first (S)", cs.remote_only));
    }

    if let Some((top, _)) = view.push_pick() {
        let oid = top.oid.clone();

        return ask_push(oid, view);
    }

    match cs.list.iter().find(|character| !character.pushed) {
        Some(character_value) => {
            let oid = character_value.oid.clone();

            ask_push(oid, view)
        }
        None => Act::Toast("0 unpushed commits".into()),
    }
}
