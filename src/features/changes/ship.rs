use super::{
    Act, Modal, Op, PUSH_RUNS, Ship, ShipRow, Then, View, base_branch, ci_fix_message, git, pr_desc_task,
    pr_rebase_task, pr_task, ship_task, stash,
};

impl View {
    /// The ship section's rows: the steps valid now (a stopped rebase: only its steps), by section.
    pub(super) fn ship_rows(&self) -> Vec<ShipRow> {
        ship_rows_view(self)
    }

    /// Ship step I of the panel's context menu (its "ship" section) runs; grayed: the toast.
    pub(crate) fn ctx_step(&mut self, i: usize) -> Act {
        let rows = self.ship_rows();
        let Some(row) = rows.get(i) else { return Act::None };

        if row.off {
            return Act::Toast(PUSH_RUNS.into());
        }

        ship(row.step, self)
    }

    /// The question's answer YES: its step runs.
    pub(crate) fn confirm(&mut self, yes: bool) -> Act {
        confirm_view(self, yes)
    }
}

fn rebase_ship_rows(
    row: impl Fn(Ship, String, String, bool) -> ShipRow,
    changes: &super::Changes,
) -> Option<Vec<ShipRow>> {
    if let Some(files) = &changes.stopped {
        return Some(vec![
            row(
                Ship::Resolve,
                "resolve the conflicts".into(),
                format!("{} conflicts", files.len()),
                false,
            ),
            row(
                Ship::Continue,
                "continue the rebase".into(),
                "git rebase --continue".into(),
                true,
            ),
            row(
                Ship::Abort,
                "abort the rebase".into(),
                "the branch as before".into(),
                true,
            ),
        ]);
    }

    None
}

fn existing_pull_request_rows(
    row: impl Fn(Ship, String, String, bool) -> ShipRow,
    changes: &super::Changes,
    rows: &mut Vec<ShipRow>,
    pull_request: &super::Pr,
) {
    rows.push(row(
        Ship::PrRebase,
        format!("update PR on the newest {}", base_branch(&changes.base)),
        format!("{} behind · sub-thread", changes.behind),
        true,
    ));
    rows.push(row(
        Ship::PrDesc,
        "update PR description".into(),
        "sub-thread".into(),
        true,
    ));
    rows.push(row(
        Ship::Merge,
        format!("merge PR #{} when its checks pass", pull_request.number),
        format!(
            "✓{} ✗{} ●{}",
            pull_request.pass, pull_request.fail, pull_request.pending
        ),
        true,
    ));

    if pull_request.fail > 0 {
        rows.push(row(
            Ship::CiLog,
            "CI failed: the log to the agents".into(),
            format!("{} failed", pull_request.fail),
            false,
        ));
    }
}

fn new_pull_request_rows(row: impl Fn(Ship, String, String, bool) -> ShipRow, rows: &mut Vec<ShipRow>) {
    rows.push(row(
        Ship::AgentPr { draft: false },
        "create PR".into(),
        "sub-thread".into(),
        true,
    ));
    rows.push(row(
        Ship::AgentPr { draft: true },
        "create PR draft".into(),
        "sub-thread".into(),
        true,
    ));
    rows.push(row(
        Ship::OpenPr,
        "push + open the pull request".into(),
        "gh pr create --fill".into(),
        true,
    ));
}

fn ship_cilog(view: &mut View) -> Act {
    if view.operation_state.ci.is_some() {
        return Act::Toast("CI: reading the log…".into());
    }

    let (tx, rx) = crate::infra::bus::channel();
    let dir = view.dir.clone();

    std::thread::spawn(move || {
        let _ = tx.send(ci_fix_message(&dir));
    });

    view.operation_state.ci = Some(rx);

    Act::Toast("CI: reading the failed log for the lead…".into())
}

fn ship_full(view: &mut View) -> Act {
    let branch = match checkout_branch(view) {
        Ok(branch) => branch,
        Err(error) => return Act::Toast(error),
    };

    view.pull_request.pr_at = None;
    Act::Sub(ship_task(&branch, &view.changes, view.pull_request.pr.as_ref()))
}

fn ship_prrebase(view: &mut View) -> Act {
    let Some(pr) = view.pull_request.pr.clone() else {
        return Act::Toast("0 pull request on this branch".into());
    };

    // a rebase needs a clean tree: the owner's uncommitted work never gets touched
    if !view.changes.files.is_empty() {
        return Act::Toast(format!(
            "{} uncommitted files: commit or stash them first",
            view.changes.files.len()
        ));
    }

    let branch = match checkout_branch(view) {
        Ok(branch) => branch,
        Err(error) => return Act::Toast(error),
    };

    view.pull_request.pr_at = None;
    Act::Sub(pr_rebase_task(&branch, &view.changes.base, &pr))
}

fn ship_prdesc(view: &mut View) -> Act {
    let Some(pr) = view.pull_request.pr.clone() else {
        return Act::Toast("0 pull request on this branch".into());
    };

    let branch = match checkout_branch(view) {
        Ok(branch) => branch,
        Err(error) => return Act::Toast(error),
    };

    view.pull_request.pr_at = None;
    Act::Sub(pr_desc_task(&branch, &pr))
}

fn ship_agentpr(view: &mut View, draft: bool) -> Act {
    if view.changes.ahead == 0 {
        return Act::Toast(format!(
            "0 commits ahead of {}: commit first (uncommitted changes stay out of the PR)",
            view.changes.base
        ));
    }

    let branch = match checkout_branch(view) {
        Ok(branch) => branch,
        Err(error) => return Act::Toast(error),
    };

    view.pull_request.pr_at = None;
    Act::Sub(pr_task(&branch, &view.changes.base, draft))
}

fn ship_resolve(view: &mut View) -> Act {
    let conflicts = view.changes.stopped.clone().unwrap_or_default();

    Act::Send(format!(
        "The rebase on {} stopped with conflicts in: {}. Resolve each conflict (keep the intent of both sides), `git add` the files, then run `git -c core.editor=true rebase --continue`. Repeat until the rebase is done; report what you decided per conflict.",
        view.changes.base,
        conflicts.join(", ")
    ))
}

/// The checkout's branch (the agent steps name it). Err: a detached HEAD, 0 git.
fn checkout_branch(view: &View) -> Result<String, String> {
    #[cfg(test)]
    if let Some(branch) = &view.branch_override {
        return Ok(branch.clone());
    }

    let branch = git(&view.dir, &["rev-parse", "--abbrev-ref", "HEAD"])
        .map_err(|error| format!("branch: {error}"))?
        .trim()
        .to_string();

    if branch.is_empty() || branch == "HEAD" {
        return Err("detached HEAD: check out a branch first".into());
    }

    Ok(branch)
}

/// Step STEP of the ship section.
fn ship(step: Ship, view: &mut View) -> Act {
    match step {
        Ship::Push => view.start_op(Op::Push),
        Ship::PushTo => view.ask_push_to(),
        Ship::Update => view.start_op(Op::Update(view.changes.base.clone())),
        Ship::Continue => view.start_op(Op::Continue),
        Ship::Abort => view.start_op(Op::Abort),
        Ship::Resolve => ship_resolve(view),
        Ship::OpenPr => {
            view.pull_request.pr_at = None;
            view.start_op(Op::OpenPr)
        }
        Ship::Merge => {
            view.pull_request.pr_at = None;
            view.start_op(Op::Merge)
        }
        Ship::AgentPr { draft } => ship_agentpr(view, draft),
        Ship::PrDesc => ship_prdesc(view),
        Ship::PrRebase => ship_prrebase(view),
        Ship::Full => ship_full(view),
        Ship::CiLog => ship_cilog(view),
    }
}

fn confirm_view(view: &mut View, yes: bool) -> Act {
    let Some(Modal::Confirm { then, .. }) = view.modal_state.modal.take() else {
        return Act::None;
    };

    if !yes {
        return Act::None;
    }

    match then {
        Then::Uncommit(picks, head) => return view.start_uncommit(picks, head),
        Then::PushTo(oid) => {
            view.scm.commit_picks.clear();
            view.pull_request.pr_at = None;
            return view.start_op(Op::PushTo(oid));
        }
        _ => {}
    }

    view.stash_state.stash = stash::read(&view.id);
    let before = view.stash_state.stash.len();

    match then {
        Then::DropStash(id) => view.stash_state.stash.retain(|entry| entry.id != id),
        Then::ClearStash => view.stash_state.stash.clear(),
        Then::Uncommit(..) | Then::PushTo(_) => {}
    }

    stash::write(&view.id, &view.stash_state.stash);
    view.stash_state.stash_at = stash::stamp(&view.id);

    Act::Toast(format!(
        "stash: {} removed · {} left",
        before - view.stash_state.stash.len(),
        view.stash_state.stash.len()
    ))
}

fn ship_rows_view(view: &View) -> Vec<ShipRow> {
    let busy = view.pushing();

    let row = |step: Ship, label: String, state: String, asks| {
        // a push runs: every step that pushes, grayed
        let off = busy && step.pushes();
        let state = if off { "a push runs".into() } else { state };

        ShipRow {
            group: step.group(),
            agent: step.agent(),
            off,
            step,
            label,
            state,
            asks,
        }
    };

    let changes = &view.changes;

    if let Some(rows) = rebase_ship_rows(row, changes) {
        return rows;
    }

    let mut rows = vec![row(
        Ship::Full,
        "ship: commit, push, PR, review, CI green".into(),
        "sub-thread".into(),
        true,
    )];

    rows.push(row(
        Ship::Push,
        "push".into(),
        format!("{} ahead of {}", changes.ahead, changes.base),
        true,
    ));

    if let Some((top, count)) = view.push_pick() {
        rows.push(row(
            Ship::PushTo,
            format!("push up to {} (picked)", top.short),
            format!("{count} commit{}", if count == 1 { "" } else { "s" }),
            false,
        ));
    }

    if changes.base != "HEAD" {
        rows.push(row(
            Ship::Update,
            format!("fetch + rebase on {}", changes.base),
            format!("{} behind", changes.behind),
            true,
        ));
    }

    match &view.pull_request.pr {
        None => new_pull_request_rows(row, &mut rows),
        Some(pr) => existing_pull_request_rows(row, changes, &mut rows, pr),
    }

    rows
}
