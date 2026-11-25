use super::{Act, Duration, Instant, Op, PUSH_RUNS, View, expected, pr_state, run_op};

impl View {
    /// Run git step OP on a thread; its end re-reads git.
    pub(crate) fn start_op(&mut self, op: Op) -> Act {
        start_op_view(self, op)
    }

    /// A push runs: its PUSHING group's wave moves (the view draws at the wave's pace).
    pub(crate) fn pushing(&self) -> bool {
        !self.pushing.is_empty() || self.push_by.is_some()
    }

    /// The push lock of the checkout: another process (an agent, shell mode) takes it → its
    /// commits (the unpushed ones now) into PUSHING; it ends → a read, then they leave. True:
    /// the rows changed.
    pub(crate) fn poll_push_lock(&mut self) -> bool {
        poll_push_lock_view(self)
    }

    /// What the last git step did, once (the thread view toasts it).
    pub(crate) fn take_note(&mut self) -> Option<String> {
        self.operation_state.note.take()
    }

    /// A message for the thread, once (the CI fix; the thread view sends it).
    pub(crate) fn take_send(&mut self) -> Option<String> {
        (!self.operation_state.outbox.is_empty()).then(|| self.operation_state.outbox.remove(0))
    }

    /// The PR read (30 s) and the CI fix message: finished ones apply. True: something changed.
    pub(super) fn poll_ship(&mut self) -> bool {
        poll_ship_view(self)
    }

    pub(super) fn poll_op(&mut self) -> bool {
        poll_op_view(self)
    }
}

fn start_op_view(view: &mut View, op: Op) -> Act {
    if view.pr_src.is_some() {
        return Act::Toast("a pull request's diff: read only (the merge tab's prompt tells the agents)".into());
    }

    if let Some(commit) = &view.commit_src {
        return Act::Toast(format!("commit {}: read only", commit.short));
    }

    if view.all {
        return Act::Toast("all changes: 0 git steps (esc: the uncommitted changes)".into());
    }

    if view.pushing() && op.pushes() {
        return Act::Toast(PUSH_RUNS.into());
    }

    if view.operation_state.op.is_some() {
        return Act::Toast("git: 1 step at a time".into());
    }

    // a push: its commits leave UNPUSHED for PUSHING until it ends
    let unpushed = view
        .changes
        .commits
        .list
        .iter()
        .filter(|commit| !commit.pushed)
        .map(|commit| commit.oid.clone());

    view.pushing = match &op {
        Op::Push | Op::OpenPr => unpushed.collect(),
        Op::PushTo(oid) => unpushed.skip_while(|unpushed_oid| unpushed_oid != oid).collect(),
        _ => Vec::new(),
    };

    view.pushing_at = (!view.pushing.is_empty()).then(Instant::now);
    let (tx, rx) = crate::infra::bus::channel();
    let dir = view.dir.clone();
    let label = op.label();

    // the card shows the result at once; the git read after the step confirms it
    if let Some(line) = expected(&op, &view.changes, crate::domain::landed::of(&view.id)) {
        crate::domain::landed::expect(&view.id, line);
    }

    std::thread::spawn(move || {
        let _ = tx.send(run_op(&dir, &op));
    });

    view.operation_state.op = Some(rx);
    view.layout.built = None;

    Act::Toast(format!("git … {label}"))
}

fn poll_push_lock_view(view: &mut View) -> bool {
    let Some(git_dir) = view.changes.git_dir.as_deref() else {
        return false;
    };

    let other = crate::infra::pushlock::held_in(git_dir).filter(|lock| lock.pid != std::process::id());

    match (other, view.push_by.is_some()) {
        (Some(lock), false) => {
            view.push_by = Some(lock.by);
            view.push_hold = false;
            view.pushing = view
                .changes
                .commits
                .list
                .iter()
                .filter(|commit| !commit.pushed)
                .map(|commit| commit.oid.clone())
                .collect();
            view.pushing_at = Some(Instant::now());

            // 0 unpushed known yet: the label shows the push all the same
            true
        }
        (None, true) => {
            view.push_by = None;
            view.push_hold = true;

            // a read on its way started before the push ended: a new one
            view.loading = None;
            view.refresh(true);
            true
        }
        _ => false,
    }
}

fn poll_ship_view(view: &mut View) -> bool {
    let mut changed = false;

    if let Some(rx) = &view.pull_request.pr_read
        && let Ok(pr) = rx.try_recv()
    {
        view.pull_request.pr_read = None;
        changed = view.pull_request.pr != pr;
        view.pull_request.pr = pr;
    }

    let on_branch = !cfg!(test)
        && view.pr_src.is_none()
        && view.commit_src.is_none()
        && view.changes.base != "HEAD"
        && !view.changes.base.is_empty();

    if on_branch
        && view.pull_request.pr_read.is_none()
        && view
            .pull_request
            .pr_at
            .is_none_or(|read_at| read_at.elapsed() > Duration::from_secs(30))
    {
        view.pull_request.pr_at = Some(Instant::now());
        let (tx, rx) = crate::infra::bus::channel();
        let dir = view.dir.clone();

        std::thread::spawn(move || {
            let _ = tx.send(pr_state(&dir));
        });

        view.pull_request.pr_read = Some(rx);
    }

    if let Some(rx) = &view.operation_state.ci
        && let Ok(result) = rx.try_recv()
    {
        view.operation_state.ci = None;

        match result {
            Ok(message) => view.operation_state.outbox.push(message),
            Err(error) => view.operation_state.note = Some(format!("CI: {error}")),
        }

        changed = true;
    }

    changed
}

fn poll_op_view(view: &mut View) -> bool {
    let Some(rx) = &view.operation_state.op else {
        return false;
    };

    let Ok(result) = rx.try_recv() else { return false };

    view.operation_state.op = None;

    // the push ended: its commits pushed (the read after it agrees), else back to UNPUSHED
    let gone = std::mem::take(&mut view.pushing);

    view.pushing_at = None;

    if result.is_ok() {
        for commit in view
            .changes
            .commits
            .list
            .iter_mut()
            .filter(|commit| gone.contains(&commit.oid))
        {
            commit.pushed = true;
        }
    }

    let prefill = view.operation_state.prefill.take();

    if result.is_ok()
        && let Some(message) = prefill
        && view.scm.msg.text().trim().is_empty()
    {
        view.scm.msg.set_text(&message);
    }

    view.operation_state.note = Some(match result {
        Ok(done) => format!("git: {done}"),
        Err(error) => format!("git: {error}"),
    });

    // the expected state of `start_op`: confirmed or corrected by a read
    crate::domain::landed::refresh(&view.id);
    view.refresh(true);
    view.sync_stash(true);

    true
}
