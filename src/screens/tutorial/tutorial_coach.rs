use super::{Coach, Event, Kind, MARKER, Member, Path, REPO_EVERY, SEEDED, Task, make_quests, mock_merged};

impl Coach {
    pub(crate) fn new(crew: &[Member]) -> Coach {
        let by = |result: &str| {
            crew.iter()
                .find(|message| message.role == result)
                .map(|message_value| message_value.name.clone())
        };

        let reviewer = by("reviewer");

        let crew_tasks = if reviewer.is_some() {
            vec![Task::Mention, Task::Watch]
        } else {
            vec![Task::Mention]
        };

        let quests = make_quests(crew_tasks);

        Coach {
            quests,
            done: Vec::new(),
            lead: crew.first().map(|message_2| message_2.name.clone()).unwrap_or_default(),
            builder: by("builder").unwrap_or_default(),
            reviewer,
            owner_msgs: 0,
            fixed: false,
            repo_at: None,
        }
    }
    pub(super) fn order(&self) -> Vec<Task> {
        self.quests
            .iter()
            .flat_map(|query| query.tasks.iter().copied())
            .collect()
    }
    /// The next task (None: all done).
    pub(crate) fn current(&self) -> Option<Task> {
        self.order().into_iter().find(|thread| !self.done.contains(thread))
    }
    pub(crate) fn is_done(&self, thread: Task) -> bool {
        self.done.contains(&thread)
    }
    /// The quest of the next task (None: all done).
    pub(crate) fn quest(&self) -> Option<usize> {
        let cur = self.current()?;

        self.quests.iter().position(|query| query.tasks.contains(&cur))
    }
    /// Every task of the basics done.
    pub(crate) fn basics_done(&self) -> bool {
        self.quests
            .iter()
            .filter(|query| !query.advanced)
            .flat_map(|query_value| &query_value.tasks)
            .all(|thread| self.done.contains(thread))
    }
    /// The owner may leave as done: the basics done (the advanced quests are optional).
    pub(crate) fn finished(&self) -> bool {
        self.basics_done()
    }
    /// The basics done, 0 advanced task yet: the stop offer shows.
    pub(crate) fn at_stop(&self) -> bool {
        self.basics_done()
            && !self
                .quests
                .iter()
                .filter(|query| query.advanced)
                .flat_map(|query_value| &query_value.tasks)
                .any(|thread| self.done.contains(thread))
    }
    /// Task T done, and every task before it (a skipped one never blocks). true: a change.
    pub(crate) fn reach(&mut self, thread: Task) -> bool {
        reach_task(self, thread)
    }
    /// What to do, the keys.
    pub(crate) fn text(&self, thread: Task) -> (String, &'static str) {
        let (lead, bytes) = (&self.lead, &self.builder);
        let side = self.reviewer.as_deref().unwrap_or(lead);

        match (thread, &self.reviewer) {
            (Task::Send, _) => (format!("send the task to {lead}. it holds a style guide"), "ctrl+enter"),
            (Task::Attach, _) => (
                format!("click {MARKER} in your message: the files view shows it"),
                "click",
            ),
            (Task::Mention, Some(result)) => (
                format!("back to the chat. call {result} in with a shortcut: @{result} #review"),
                "F1 · @ · # · ctrl+enter",
            ),
            (Task::Mention, None) => (
                format!("back to the chat. ask for more with a shortcut: @{lead} #polish"),
                "F1 · # · ctrl+enter",
            ),
            (Task::Watch, Some(result_value)) => (format!("watch {result_value} and {bytes} work it out"), "wait"),
            (Task::Watch, None) => (String::new(), ""),
            (Task::Changes, _) => ("see what changed: the changes view".into(), "F2"),
            (Task::Ide, _) => (
                "open a line of hello.md in your IDE (here: a toast only)".into(),
                "j k · e",
            ),
            (Task::Stage, _) => (
                format!("stage hello.md only: README.md is the work of \"{SEEDED}\""),
                "← · j k · space · s",
            ),
            (Task::Commit, _) => ("write a commit message, commit".into(), "m · type · ctrl+enter"),
            (Task::Sub, _) => (
                format!("a side check before the PR: >sub @{side} does README.md need a change?"),
                "F1 · >sub · ctrl+enter",
            ),
            (Task::SubDone, _) => ("wait for the sub-thread's result in the chat".into(), "wait"),
            (Task::AskPr, _) => (format!("ask for a pull request: @{lead} open a PR"), "ctrl+enter"),
            (Task::MergeView, _) => ("see the PR in the merge view".into(), "F3"),
            (Task::Merged, _) => ("merge it: the merge stop, enter enter".into(), "j k · enter enter"),
            (Task::Menu, _) => ("open the menu".into(), "ctrl+p"),
            (Task::Switch, _) => (format!("open the thread \"{SEEDED}\""), "↑↓ · enter"),
            (Task::Menu2, _) => ("open the menu again".into(), "ctrl+p"),
            (Task::New, _) => ("start a new thread".into(), "n"),
            (Task::Flow, _) => ("pick the flow \"chore\" on an agent card".into(), "f on a card · enter"),
            (Task::Start, _) => ("start the thread".into(), "ctrl+enter"),
            (Task::FlowRun, _) => ("watch the flow bar: 3 steps".into(), "wait"),
            (Task::Leave, _) => ("done. leave the tutorial".into(), "esc esc"),
        }
    }
    /// New events of a thread. true: a change.
    pub(crate) fn see(&mut self, events: &[Event]) -> bool {
        observe_events(self, events)
    }
    /// T done (+ the tasks before it) once AFTER is done: an advanced task done out of order.
    pub(super) fn reach_from(&mut self, thread: Task, after: Task) -> bool {
        if self.is_done(after) { self.reach(thread) } else { false }
    }
    /// The view's state. true: a change.
    pub(crate) fn see_view(&mut self, path: &crate::screens::thread_view::Peek) -> bool {
        let mut moved = false;

        match path.pane {
            crate::screens::thread_view::Pane::Files => moved |= self.tick(Task::Attach),
            crate::screens::thread_view::Pane::Changes => moved |= self.tick(Task::Changes),
            crate::screens::thread_view::Pane::Merge => moved |= self.tick(Task::MergeView),
            crate::screens::thread_view::Pane::Chat => {}
        }

        if path.ide {
            moved |= self.tick(Task::Ide);
        }

        if path.menu {
            moved |= self.tick(Task::Menu) || self.tick(Task::Menu2);
        }

        moved
    }
    /// The repo's git state, while a task waits on it (max. 1 read per `REPO_EVERY`). true: a
    /// change.
    pub(crate) fn see_repo(&mut self, repo: &Path) -> bool {
        let Some(thread_value) = self
            .current()
            .filter(|thread| matches!(thread, Task::Ide | Task::Stage | Task::Commit | Task::Merged))
        else {
            return false;
        };

        if self.repo_at.is_some_and(|at| at.elapsed() < REPO_EVERY) {
            return false;
        }

        self.repo_at = Some(std::time::Instant::now());

        let out = |args: &[&str]| {
            std::process::Command::new("git")
                .arg("-C")
                .arg(repo)
                .args(args)
                .stdin(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .output()
                .ok()
                .map(|output_value| String::from_utf8_lossy(&output_value.stdout).into_owned())
                .unwrap_or_default()
        };

        if thread_value == Task::Merged {
            return mock_merged(repo) && self.tick(Task::Merged);
        }

        // a commit: every task of the quest before it counts as done
        if out(&["rev-list", "--count", "HEAD"]).trim().parse::<u32>().unwrap_or(0) > 1 {
            return self.reach(Task::Commit);
        }

        if thread_value == Task::Stage
            && out(&["diff", "--cached", "--name-only"])
                .lines()
                .any(|line| line.trim() == "hello.md")
        {
            return self.tick(Task::Stage);
        }

        false
    }
}

fn reach_task(widget_state: &mut Coach, thread: Task) -> bool {
    if widget_state.done.contains(&thread) {
        return false;
    }

    for x in widget_state.order() {
        if !widget_state.done.contains(&x) {
            widget_state.done.push(x);
        }

        if x == thread {
            break;
        }
    }

    true
}

fn observe_events(widget_state: &mut Coach, events: &[Event]) -> bool {
    let mut moved = false;

    for error in events {
        observe_event(widget_state, &mut moved, error);
    }

    moved
}

fn observe_event(widget_state: &mut Coach, moved: &mut bool, error: &Event) {
    match &error.kind {
        Kind::Prompt { from, to, text } if from == "owner" => {
            observe_prompt(widget_state, moved, to, text);
        }
        Kind::Prompt { from, to, .. }
            if *from == widget_state.builder
                && widget_state
                    .reviewer
                    .as_ref()
                    .is_some_and(|result_value| to.contains(result_value)) =>
        {
            widget_state.fixed = true;
        }
        Kind::TurnEnd { .. } if widget_state.fixed && error.agent.is_some() && error.agent == widget_state.reviewer => {
            *moved |= widget_state.tick(Task::Watch);
        }
        Kind::SubThread { state, .. } if state == "running" => {
            *moved |= widget_state.reach_from(Task::Sub, Task::Commit);
        }
        Kind::SubThread { state, .. } if state == "done" || state == "failed" => {
            *moved |= widget_state.tick(Task::SubDone);
        }
        Kind::Text { text } if error.agent.is_some() && text.contains("/pull/") => {
            *moved |= widget_state.reach_from(Task::AskPr, Task::Commit);
        }
        Kind::Flow { op, .. } if op == "finished" => *moved |= widget_state.tick(Task::FlowRun),
        _ => {}
    }
}

fn observe_prompt(widget_state: &mut Coach, moved: &mut bool, to: &[String], text: &str) {
    widget_state.owner_msgs += 1;
    let to_reviewer = widget_state
        .reviewer
        .as_ref()
        .is_some_and(|result| to.contains(result) || text.contains(&format!("@{result}")));

    *moved |= widget_state.tick(Task::Send);

    if to_reviewer || (widget_state.reviewer.is_none() && widget_state.owner_msgs >= 2) {
        // the files view skipped: the mention counts all the same
        if widget_state.current() == Some(Task::Attach) {
            *moved |= widget_state.reach(Task::Attach);
        }

        *moved |= widget_state.tick(Task::Mention);
    }
}
