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

fn poll_merge(widget_state: &mut Merge, visible: bool) -> bool {
    let mut dirty = false;

    while let Ok(result) = widget_state.rx.try_recv() {
        dirty = true;

        apply_result(widget_state, result);
    }

    if let Some(changes_2) = widget_state.tab().and_then(|changes_value| {
        if changes_value.sub == Sub::Changes {
            changes_value.changes.as_mut()
        } else {
            None
        }
    }) && changes_2.poll()
    {
        dirty = true;
    }

    if !visible || widget_state.active.elapsed() > SLEEP || cfg!(test) {
        return dirty;
    }

    // the thread's PR list
    if widget_state.thread
        && !widget_state.assoc_busy
        && widget_state.sources.is_some()
        && widget_state.assoc_at.is_none_or(|thread_3| thread_3.elapsed() > ASSOC)
    {
        widget_state.read_assoc();
    }

    // the PR in view
    read_visible_request(widget_state, dirty)
}

fn apply_associated_requests(widget_state: &mut Merge, result_2: Result<Vec<PrRef>, String>) {
    widget_state.assoc_busy = false;

    match result_2 {
        Ok(list) => {
            crate::screens::merge::pr_list::remember(&widget_state.thread_id, &list);

            if list != widget_state.prs {
                widget_state.version += 1;
            }

            let keep = widget_state.current().cloned();

            widget_state.prs = list;
            widget_state.cur = keep
                .and_then(|key| widget_state.prs.iter().position(|path_value| *path_value == key))
                .unwrap_or(0);
            widget_state.assoc_error = None;
        }
        Err(error_2) => widget_state.assoc_error = Some(error_2),
    }
}

fn apply_operation_result(labels: &HashMap<String, Vec<gh::Label>>, pr: &PrRef, op: &Op, thread_2: &mut Tab) -> String {
    let queue = thread_2.pr.as_ref().is_some_and(|path_2| path_2.queue);

    let note = match (&op, queue) {
        (Op::Merge(_, false), true) => "added to the merge queue".into(),
        (Op::Merge(_, true), true) => "auto-merge on: joins the merge queue when ready".into(),
        _ => op.done().to_string(),
    };

    // the merge state at once on screen; the read confirms
    if let Some(path_3) = thread_2.pr.as_mut() {
        match &op {
            Op::Merge(message, true) => path_3.auto_merge = Some(message.clone()),
            Op::Merge(_, false) if queue => path_3.queued = Some(gh::Queued::default()),
            Op::Merge(_, false) => path_3.state = State::Merged,
            Op::DisableAuto => path_3.auto_merge = None,
            Op::Dequeue(_) => path_3.queued = None,
            _ => {}
        }
    }

    if let (Some(path_4), Op::AddLabel(line) | Op::RemoveLabel(line)) = (thread_2.pr.as_mut(), &op) {
        // at once on screen; the read confirms
        if matches!(op, Op::AddLabel(_)) {
            let color = labels
                .get(&pr.repo)
                .and_then(|ls| ls.iter().find(|x| &x.name == line))
                .map(|x| x.color.clone())
                .unwrap_or_default();

            path_4.labels.push(gh::Label {
                name: line.clone(),
                color,
            });
        } else {
            path_4.labels.retain(|x| &x.name != line);
        }
    }

    note
}

fn read_visible_request(widget_state: &mut Merge, dirty: bool) -> bool {
    if let Some(pr) = widget_state.current().cloned() {
        let thread_4 = widget_state.tabs.entry(pr.clone()).or_default();

        let every = if thread_4.reads.fast_until.is_some_and(|user| Instant::now() < user) {
            POLL_FAST
        } else if thread_4.pr.as_ref().is_some_and(|path_5| {
            path_5
                .checks
                .iter()
                .any(|character_value| character_value.verdict == Verdict::Pending)
        }) {
            POLL_CHECKS
        } else {
            POLL
        };

        // a merged / closed PR: rarely changes
        let every = if thread_4
            .pr
            .as_ref()
            .is_some_and(|path_6| matches!(path_6.state, State::Merged | State::Closed))
        {
            every * 6
        } else {
            every
        };

        if !thread_4.reads.reading && thread_4.reads.read_at.is_none_or(|result_5| result_5.elapsed() > every) {
            widget_state.read(pr);
        }
    }

    dirty
}

fn apply_result(widget_state: &mut Merge, result: Res) {
    match result {
        Res::Pr(pr, result_value) => {
            apply_pull_request(widget_state, pr, result_value);
        }
        Res::Prs(result_2) => {
            apply_associated_requests(widget_state, result_2);
        }
        Res::Op(pr, op, result_3) => {
            let thread_2 = widget_state.tabs.entry(pr.clone()).or_default();

            thread_2.input.busy = None;
            thread_2.reads.fast_until = Some(Instant::now() + FAST_FOR);

            match result_3 {
                Ok(()) => {
                    widget_state.note = Some(apply_operation_result(&widget_state.labels, &pr, &op, thread_2));
                    widget_state.read(pr);
                }
                Err(error_3) => widget_state.note = Some(format!("gh: {error_3}")),
            }
        }
        Res::Labels(repo, result_4) => match result_4 {
            Ok(line_value) => {
                widget_state.labels.insert(repo, line_value);
            }
            Err(error_4) => widget_state.note = Some(format!("labels: {error_4}")),
        },
    }
}

fn apply_pull_request(widget_state: &mut Merge, pr: PrRef, result_value: Box<Result<gh::PullRequest, String>>) {
    let thread_value = widget_state.tabs.entry(pr).or_default();

    thread_value.reads.reading = false;

    match *result_value {
        Ok(path) => {
            if let Some(character) = thread_value.changes.as_mut() {
                character.set_pr(&path);
            }

            if thread_value.pr.as_ref() != Some(&path) {
                widget_state.version += 1;
            }

            thread_value.pr = Some(path);
            thread_value.error = None;
        }
        Err(error_value) => thread_value.error = Some(error_value),
    }
}
