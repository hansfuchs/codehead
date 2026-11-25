use super::{ARM, Act, Duration, Instant, MAX_PER_MIN, MIN_MANUAL, Merge, Op, PrRef, Res, cache_write, gh};

impl Merge {
    /// A gh call may go now (max. `MAX_PER_MIN` per minute).
    pub(super) fn budget(&mut self) -> bool {
        self.calls.retain(|thread| thread.elapsed() < Duration::from_secs(60));

        if self.calls.len() >= MAX_PER_MIN {
            return false;
        }

        self.calls.push(Instant::now());
        true
    }
    pub(super) fn spawn(&self, file: impl FnOnce() -> Res + Send + 'static) {
        let tx = self.tx.clone();

        if cfg!(test) {
            let _ = tx.send(file());

            return;
        }

        std::thread::spawn(move || {
            let _ = tx.send(file());
        });
    }
    pub(super) fn read(&mut self, pr: PrRef) {
        read_pull_request(self, pr);
    }
    /// A stop waits for its 2nd enter (its button says so): redraws until the arm ends.
    pub(crate) fn armed(&self) -> bool {
        self.tab_ref()
            .and_then(|thread| thread.input.armed)
            .is_some_and(|(_, at)| at.elapsed() < ARM + Duration::from_millis(500))
    }
    /// The owner is at work (a key anywhere in the view): 0 sleep.
    pub(crate) fn touch(&mut self) {
        self.active = Instant::now();
    }
    /// `r`: read again now (3 s apart).
    pub(super) fn manual(&mut self) -> Act {
        refresh_pull_requests(self)
    }
    pub(super) fn run(&mut self, op: &Op) -> Act {
        let Some(pr) = self.current().cloned() else {
            return Act::None;
        };

        let thread = self.tabs.entry(pr.clone()).or_default();

        if thread.input.busy.is_some() {
            return Act::Toast("gh: 1 edit at a time".into());
        }

        thread.input.busy = Some(op.clone());
        let output = op.clone();

        self.spawn(move || {
            let result = gh::run(&pr, &output);

            Res::Op(pr, output, result)
        });

        Act::None
    }
}

fn read_pull_request(widget_state: &mut Merge, pr: PrRef) {
    if !widget_state.budget() {
        return;
    }

    let thread = widget_state.tabs.entry(pr.clone()).or_default();

    thread.reads.reading = true;
    thread.reads.read_at = Some(Instant::now());

    widget_state.spawn(move || {
        let result = gh::fetch(&pr);

        if let Ok(path) = &result
            && !cfg!(test)
        {
            cache_write(path);
        }

        Res::Pr(pr, Box::new(result))
    });
}

fn refresh_pull_requests(widget_state: &mut Merge) -> Act {
    if widget_state
        .manual_at
        .is_some_and(|thread_value| thread_value.elapsed() < MIN_MANUAL)
    {
        return Act::Toast("read: wait a moment".into());
    }

    widget_state.manual_at = Some(Instant::now());

    if widget_state.thread {
        widget_state.assoc_at = None;
        widget_state.read_assoc();
    }

    if let Some(pr) = widget_state.current().cloned() {
        widget_state.read(pr);

        if let Some(character) = widget_state.tab().and_then(|thread_2| thread_2.changes.as_mut()) {
            character.refresh(true);
        }
    }

    Act::Toast("merge: reading GitHub".into())
}
