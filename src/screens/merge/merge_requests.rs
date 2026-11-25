use super::{ARM, Act, Duration, Instant, MAX_PER_MIN, MIN_MANUAL, Merge, Op, PrRef, Res, cache_write, gh};

impl Merge {
    /// A gh call may go now (max. `MAX_PER_MIN` per minute).
    pub(super) fn budget(&mut self) -> bool {
        self.calls.retain(|call| call.elapsed() < Duration::from_secs(60));

        if self.calls.len() >= MAX_PER_MIN {
            return false;
        }

        self.calls.push(Instant::now());
        true
    }

    pub(super) fn spawn(&self, job: impl FnOnce() -> Res + Send + 'static) {
        let tx = self.tx.clone();

        if cfg!(test) {
            let _ = tx.send(job());

            return;
        }

        std::thread::spawn(move || {
            let _ = tx.send(job());
        });
    }

    pub(super) fn read(&mut self, pr: PrRef) {
        read_pull_request(self, pr);
    }

    /// A stop waits for its 2nd enter (its button says so): redraws until the arm ends.
    pub(crate) fn armed(&self) -> bool {
        self.tab_ref()
            .and_then(|tab| tab.input.armed)
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

        let tab = self.tabs.entry(pr.clone()).or_default();

        if tab.input.busy.is_some() {
            return Act::Toast("gh: 1 edit at a time".into());
        }

        tab.input.busy = Some(op.clone());
        let op = op.clone();

        self.spawn(move || {
            let result = gh::run(&pr, &op);

            Res::Op(pr, op, result)
        });

        Act::None
    }
}

fn read_pull_request(merge: &mut Merge, pr: PrRef) {
    if !merge.budget() {
        return;
    }

    let tab = merge.tabs.entry(pr.clone()).or_default();

    tab.reads.reading = true;
    tab.reads.read_at = Some(Instant::now());

    merge.spawn(move || {
        let result = gh::fetch(&pr);

        if let Ok(pull_request) = &result
            && !cfg!(test)
        {
            cache_write(pull_request);
        }

        Res::Pr(pr, Box::new(result))
    });
}

fn refresh_pull_requests(merge: &mut Merge) -> Act {
    if merge
        .manual_at
        .is_some_and(|manual_at| manual_at.elapsed() < MIN_MANUAL)
    {
        return Act::Toast("read: wait a moment".into());
    }

    merge.manual_at = Some(Instant::now());

    if merge.thread {
        merge.assoc_at = None;
        merge.read_assoc();
    }

    if let Some(pr) = merge.current().cloned() {
        merge.read(pr);

        if let Some(view) = merge.tab().and_then(|tab| tab.changes.as_mut()) {
            view.refresh(true);
        }
    }

    Act::Toast("merge: reading GitHub".into())
}
