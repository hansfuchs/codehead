use super::{Instant, Value, View, chat, store};
#[cfg(not(test))]
use super::{Msg, client};

impl View {
    /// New events of the log. True: some came.
    pub(super) fn read(&mut self) -> bool {
        let evs = self.tail.read();

        if evs.is_empty() {
            return false;
        }

        if let Some(early) = self.early.as_mut() {
            early.extend(evs.iter().cloned());
        }

        let asks = self.thread.asks.first().map(|agent| agent.id.clone());
        let mut worktree = false;

        for error in &evs {
            read_store(self, &mut worktree, error);
        }

        if worktree {
            self.worktree_landed();
        }

        self.place_flow_ask();

        if self.thread.asks.first().map(|agent| agent.id.clone()) != asks {
            self.sync_ask();
        }

        self.prompt.history = self.thread.sent();
        self.stream.dirty = true;

        true
    }

    /// The thread's work dir changed (the host made its worktree after the view opened): the
    /// shell, files, paths and changes views move there.
    pub(super) fn worktree_landed(&mut self) {
        let Some(dir) = store::load(&self.id)
            .map(|thread| thread.work_dir())
            .filter(|directory| directory.is_dir())
        else {
            return;
        };

        if dir == self.cwd {
            return;
        }

        // the process dir: shell runs, relative paths (tests: 1 process, 0 change)
        if !cfg!(test) {
            let _ = std::env::set_current_dir(&dir);
        }

        self.panes.files = crate::features::files::View::new(&self.id, &dir);
        self.prompt.paths = crate::ui::paths::Paths::new(&dir);

        if self.shell_state.shell_cwd == self.cwd {
            self.shell_state.shell_cwd = dir.clone();
        }

        self.cwd = dir;
        self.checkout_state.checkouts.clear();
        self.load_checkouts();
    }

    /// A new thread's host never came (its task not logged): the task waits in the prompt.
    pub(super) fn kickoff_failed(&mut self, task: &str) {
        let _ = store::update(&self.id, |thread| thread.kickoff = false);

        if self.prompt.input.text().trim().is_empty() {
            self.prompt.input.set_text(task);
            self.prompt.input.move_end();
            self.save_prompt();
        }

        self.toast("the thread's host did not start · the task waits in the prompt · ctrl+enter: send");
        self.stream.dirty = true;
    }

    /// The log's head is in: the stream again from the 1st event (head, then the tail so far).
    pub(super) fn take_head(&mut self, head: &[store::Event]) {
        let Some(early) = self.early.take() else { return };
        let mut thread = chat::Thread::default();

        for error in head.iter().chain(&early) {
            thread.push(error);
        }

        let asks = self.thread.asks.first().map(|agent| agent.id.clone());
        let shift = thread.items.len().saturating_sub(self.thread.items.len());

        self.thread = thread;

        // the head in front: every item index moved, the search's texts again
        self.find.reset(shift);
        self.place_flow_ask();

        if self.thread.asks.first().map(|agent| agent.id.clone()) != asks {
            self.sync_ask();
        }

        self.prompt.history = self.thread.sent();
        self.stream.dirty = true;
    }

    pub(super) fn toast(&mut self, message: impl Into<String>) {
        self.toast = Some((message.into(), Instant::now()));
    }

    /// Command C to the host, off the UI thread; the reply comes back as `Msg::Done(WHAT)`.
    pub(super) fn host(&self, what: &'static str, character: Value) {
        #[cfg(test)]
        {
            let _ = what;

            self.ops.borrow_mut().push(character);
        }

        #[cfg(not(test))]
        let (id, tx) = (self.id.clone(), self.tx.clone());

        #[cfg(not(test))]
        std::thread::spawn(move || {
            let _ = tx.send(Msg::Done(what, client::call(&id, &character)));
        });
    }
}

fn read_store(view: &mut View, worktree: &mut bool, error: &store::Event) {
    view.thread.push(error);

    // the host made the thread's worktree: the view moves there
    if let store::Kind::ToolResult { id, ok: true, .. } = &error.kind
        && id == "worktree"
        && error.agent.as_deref() == Some("setup")
    {
        *worktree = true;
    }

    // the logged text (or the turn's end) replaces the live chunks of its agent
    if matches!(
        error.kind,
        store::Kind::Text { .. } | store::Kind::Tool { .. } | store::Kind::TurnEnd { .. }
    ) && let Some(agent) = &error.agent
    {
        view.stream.live.retain(|(count, _)| count != agent);
    }

    if let store::Kind::Prompt { from, text, .. } = &error.kind
        && from == "owner"
    {
        view.prompt
            .pending
            .retain(|(thread, _, _)| !text.starts_with(thread.as_str()) && !thread.starts_with(text.as_str()));
    }

    if let (store::Kind::TurnEnd { .. }, Some((_, old, _))) = (&error.kind, &view.prompt.queued)
        && error.agent.as_deref() == Some(old.as_str())
        && let Some((new, old, text)) = view.prompt.queued.take()
    {
        view.run_replace(new, old, &(text));
    }

    if matches!(error.kind, store::Kind::Join | store::Kind::Leave) {
        view.agents.members = store::load(&view.id).map(|thread| thread.agents).unwrap_or_default();
        view.prompt.mention.replaceable = view.agents.members.clone();
        view.sync_order();
        view.sync_flowbars();
    }
}
