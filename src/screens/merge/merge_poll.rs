use super::{
    ASSOC, FAST_FOR, HashMap, Instant, Merge, Op, POLL, POLL_CHECKS, POLL_FAST, PrRef, Res, SLEEP, State, Sub, Tab,
    Verdict, gh,
};

impl Merge {
    /// Background results in, due reads out (VISIBLE: the tab shows). True: a redraw is due.
    pub(crate) fn poll(&mut self, visible: bool) -> bool {
        poll_merge(self, visible)
    }
}

fn poll_merge(merge: &mut Merge, visible: bool) -> bool {
    let mut dirty = false;

    while let Ok(result) = merge.rx.try_recv() {
        dirty = true;

        apply_result(merge, result);
    }

    if let Some(view) = merge.tab().and_then(|tab| {
        if tab.sub == Sub::Changes {
            tab.changes.as_mut()
        } else {
            None
        }
    }) && view.poll()
    {
        dirty = true;
    }

    if !visible || merge.active.elapsed() > SLEEP || cfg!(test) {
        return dirty;
    }

    // the thread's PR list
    if merge.thread
        && !merge.assoc_busy
        && merge.sources.is_some()
        && merge.assoc_at.is_none_or(|read_at| read_at.elapsed() > ASSOC)
    {
        merge.read_assoc();
    }

    // the PR in view
    read_visible_request(merge, dirty)
}

fn apply_associated_requests(merge: &mut Merge, result: Result<Vec<PrRef>, String>) {
    merge.assoc_busy = false;

    match result {
        Ok(list) => {
            crate::screens::merge::pr_list::remember(&merge.thread_id, &list);

            if list != merge.prs {
                merge.version += 1;
            }

            let keep = merge.current().cloned();

            merge.prs = list;
            merge.cur = keep
                .and_then(|kept| merge.prs.iter().position(|pr| *pr == kept))
                .unwrap_or(0);
            merge.assoc_error = None;
        }
        Err(error) => merge.assoc_error = Some(error),
    }
}

fn apply_operation_result(labels: &HashMap<String, Vec<gh::Label>>, pr: &PrRef, op: &Op, tab: &mut Tab) -> String {
    let queue = tab.pr.as_ref().is_some_and(|pull_request| pull_request.queue);

    let note = match (&op, queue) {
        (Op::Merge(_, false), true) => "added to the merge queue".into(),
        (Op::Merge(_, true), true) => "auto-merge on: joins the merge queue when ready".into(),
        _ => op.done().to_string(),
    };

    // the merge state at once on screen; the read confirms
    if let Some(pull_request) = tab.pr.as_mut() {
        match &op {
            Op::Merge(method, true) => pull_request.auto_merge = Some(method.clone()),
            Op::Merge(_, false) if queue => pull_request.queued = Some(gh::Queued::default()),
            Op::Merge(_, false) => pull_request.state = State::Merged,
            Op::DisableAuto => pull_request.auto_merge = None,
            Op::Dequeue(_) => pull_request.queued = None,
            _ => {}
        }
    }

    if let (Some(pull_request), Op::AddLabel(name) | Op::RemoveLabel(name)) = (tab.pr.as_mut(), &op) {
        // at once on screen; the read confirms
        if matches!(op, Op::AddLabel(_)) {
            let color = labels
                .get(&pr.repo)
                .and_then(|repo_labels| repo_labels.iter().find(|x| &x.name == name))
                .map(|x| x.color.clone())
                .unwrap_or_default();

            pull_request.labels.push(gh::Label {
                name: name.clone(),
                color,
            });
        } else {
            pull_request.labels.retain(|x| &x.name != name);
        }
    }

    note
}

fn read_visible_request(merge: &mut Merge, dirty: bool) -> bool {
    if let Some(pr) = merge.current().cloned() {
        let tab = merge.tabs.entry(pr.clone()).or_default();

        let every = if tab.reads.fast_until.is_some_and(|until| Instant::now() < until) {
            POLL_FAST
        } else if tab.pr.as_ref().is_some_and(|pull_request| {
            pull_request
                .checks
                .iter()
                .any(|check| check.verdict == Verdict::Pending)
        }) {
            POLL_CHECKS
        } else {
            POLL
        };

        // a merged / closed PR: rarely changes
        let every = if tab
            .pr
            .as_ref()
            .is_some_and(|pull_request| matches!(pull_request.state, State::Merged | State::Closed))
        {
            every * 6
        } else {
            every
        };

        if !tab.reads.reading && tab.reads.read_at.is_none_or(|read_at| read_at.elapsed() > every) {
            merge.read(pr);
        }
    }

    dirty
}

fn apply_result(merge: &mut Merge, result: Res) {
    match result {
        Res::Pr(pr, read) => {
            apply_pull_request(merge, pr, read);
        }
        Res::Prs(prs) => {
            apply_associated_requests(merge, prs);
        }
        Res::Op(pr, op, done) => {
            let tab = merge.tabs.entry(pr.clone()).or_default();

            tab.input.busy = None;
            tab.reads.fast_until = Some(Instant::now() + FAST_FOR);

            match done {
                Ok(()) => {
                    merge.note = Some(apply_operation_result(&merge.labels, &pr, &op, tab));
                    merge.read(pr);
                }
                Err(error) => merge.note = Some(format!("gh: {error}")),
            }
        }
        Res::Labels(repo, labels) => match labels {
            Ok(labels) => {
                merge.labels.insert(repo, labels);
            }
            Err(error) => merge.note = Some(format!("labels: {error}")),
        },
    }
}

fn apply_pull_request(merge: &mut Merge, pr: PrRef, read: Box<Result<gh::PullRequest, String>>) {
    let tab = merge.tabs.entry(pr).or_default();

    tab.reads.reading = false;

    match *read {
        Ok(pull_request) => {
            if let Some(view) = tab.changes.as_mut() {
                view.set_pr(&pull_request);
            }

            if tab.pr.as_ref() != Some(&pull_request) {
                merge.version += 1;
            }

            tab.pr = Some(pull_request);
            tab.error = None;
        }
        Err(error) => tab.error = Some(error),
    }
}
