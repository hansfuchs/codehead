use super::{
    ARCHIVE_FIRST, ARCHIVE_MS, Arc, BATCHES, Co, DAY_MS, Duration, FETCH_EVERY, HashMap, HashSet, IDLE_AT, In, Instant,
    LANDS, Land, Ordering, Path, PathBuf, READ_US, Receiver, RecvTimeoutError, Refs, Seen, Topic, Worker, bus,
    cache_path, canon, dirty_count, fetch, git, idle_kind, prs_of, refs, set, store, task_branch, thread, unset,
    with_prs,
};

impl Worker {
    pub(crate) fn new(threads_dir: PathBuf) -> Self {
        Worker {
            threads_dir,
            threads: HashMap::new(),
            cos: HashMap::new(),
            kept: HashMap::new(),
            seen: Seen::new(),
            fetched: HashMap::new(),
            fetching: Arc::default(),
            archive_at: Instant::now() + ARCHIVE_FIRST,
        }
    }
    /// Every thread once: open ones first (the cache is on screen meanwhile).
    pub(crate) fn init(&mut self) {
        let mut all: Vec<store::Thread> = store::all_in(&self.threads_dir)
            .into_iter()
            .filter(|thread| thread.state != "deleted")
            .collect();

        all.sort_by_key(|thread| thread.state == "archived");

        for thread in all {
            let id = thread.id.clone();

            self.threads.insert(id.clone(), thread);
            read_thread(&id, self);
            self.publish(&[id]);
        }
    }
    pub(crate) fn run(mut self, rx: &Receiver<In>) {
        self.init();
        Worker::save();

        loop {
            IDLE_AT.store(crate::engine::now_ms(), Ordering::Relaxed);
            let wait = self.archive_at.saturating_duration_since(Instant::now());

            let first = match rx.recv_timeout(wait) {
                Ok(message) => Some(message),
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => return,
            };

            IDLE_AT.store(0, Ordering::Relaxed);
            let batch: Vec<In> = first
                .into_iter()
                .chain(std::iter::from_fn(|| rx.try_recv().ok()))
                .collect();
            let at = Instant::now();
            let ids = self.apply(batch);

            READ_US.store(at.elapsed().as_micros() as u64, Ordering::Relaxed);

            publish_changed_threads(&ids, &mut self);

            archive_due_threads(&mut self);
        }
    }

    /// Read what BATCH names. The threads whose state may differ now.
    pub(crate) fn apply(&mut self, batch: Vec<In>) -> Vec<String> {
        let mut full: HashSet<String> = HashSet::new();
        let mut dirty: HashSet<String> = HashSet::new();
        let mut ids: HashSet<String> = HashSet::new();
        let mut gits: HashSet<PathBuf> = HashSet::new();

        collect_batch_changes(batch, &mut full, &mut dirty, &mut ids, &mut gits, self);

        if !full.is_empty() || !dirty.is_empty() {
            BATCHES.fetch_add(1, Ordering::Relaxed);
        }

        for dir in &full {
            read_co(dir, true, self);
        }

        read_batch_changes(&full, &dirty, ids, &gits, self)
    }

    /// Set the states of IDS; the UI wakes on a change. True: 1 changed.
    pub(crate) fn publish(&self, ids: &[String]) -> bool {
        let mut any = false;

        for id in ids {
            match land(id, self) {
                Some(state) => any |= set(id, state),
                None => any |= unset(id),
            }
        }

        if any {
            bus::changed();
        }

        any
    }
    pub(crate) fn save() {
        let lands = LANDS.lock().ok().and_then(|lands| lands.clone()).unwrap_or_default();

        if let Ok(bytes) = serde_json::to_vec(&lands) {
            let _ = crate::engine::write_atomic(&cache_path(), &bytes);
        }
    }
}

fn append_changed_thread(out: &mut Vec<String>, id: &String, hit: bool) {
    if hit && !out.contains(id) {
        out.push(id.clone());
    }
}

fn archive_due_threads(worker: &mut Worker) {
    if Instant::now() >= worker.archive_at {
        auto_archive(worker);
    }
}

fn publish_changed_threads(ids: &[String], worker: &mut Worker) {
    if worker.publish(ids) {
        Worker::save();
    }
}

fn refresh_thread_checkouts(full: &mut HashSet<String>, ids: &mut HashSet<String>, id: String, worker: &mut Worker) {
    if let Some(thread) = worker.threads.get(&id) {
        full.extend(dirs(thread));
    }

    ids.insert(id);
}

fn mark_work_directory_dirty(dirty: &mut HashSet<String>, top: &PathBuf, worker: &mut Worker) {
    dirty.extend(
        worker
            .cos
            .iter()
            .filter(|(_, co)| co.top == *top)
            .map(|(dir, _)| dir.clone()),
    );
}

fn mark_repo_checkouts_full(
    full: &mut HashSet<String>,
    gits: &mut HashSet<PathBuf>,
    common: PathBuf,
    part: &bus::Git,
    worker: &mut Worker,
) {
    full.extend(
        worker
            .cos
            .iter()
            .filter(|(_, co)| co.common == common && co.hit(part))
            .map(|(dir, _)| dir.clone()),
    );
    gits.insert(common);
}

fn rescan_thread_records(
    full: &mut HashSet<String>,
    ids: &mut HashSet<String>,
    gits: &mut HashSet<PathBuf>,
    root: PathBuf,
    worker: &mut Worker,
) {
    if root == canon(&worker.threads_dir) {
        // lost thread events: every thread again
        for thread in store::all_in(&worker.threads_dir)
            .into_iter()
            .filter(|thread| thread.state != "deleted")
        {
            ids.insert(thread.id.clone());
            worker.threads.insert(thread.id.clone(), thread);
        }
    }

    full.extend(
        worker
            .cos
            .iter()
            .filter(|(_, co)| co.top.starts_with(&root) || co.common.starts_with(&root))
            .map(|(dir, _)| dir.clone()),
    );
    gits.insert(root);
}

fn reload_thread_record(full: &mut HashSet<String>, ids: &mut HashSet<String>, id: String, worker: &mut Worker) {
    let new = store::load_in(&worker.threads_dir, &id).filter(|thread| thread.state != "deleted");
    let old = worker.threads.get(&id);

    let key = |thread: &store::Thread| {
        (
            thread.state.clone(),
            thread
                .all_checkouts()
                .into_iter()
                .map(|checkout| checkout.dir)
                .collect::<Vec<_>>(),
            thread.worktree_pending.clone(),
            thread.repo.clone(),
        )
    };

    if new.as_ref().map(key) != old.map(key) {
        match new {
            Some(thread) => {
                full.extend(dirs(&thread));
                worker.threads.insert(id.clone(), thread);
            }
            None => {
                worker.threads.remove(&id);
            }
        }

        ids.insert(id);
    }
}

/// The checkout dirs of thread T (archived: 0).
fn dirs(thread: &store::Thread) -> Vec<String> {
    if thread.state == "archived" {
        return Vec::new();
    }

    thread
        .all_checkouts()
        .into_iter()
        .map(|checkout| checkout.dir)
        .collect()
}

/// Read thread ID: its checkouts 0 read yet (open), its kept branch (archived).
fn read_thread(id: &str, worker: &mut Worker) {
    let Some(record) = worker.threads.get(id).cloned() else {
        return;
    };

    if record.state == "archived" {
        let land = thread(&record, &prs_of(id), &mut worker.seen);

        worker.kept.insert(id.to_string(), land);
        return;
    }

    for dir in dirs(&record) {
        if !worker.cos.contains_key(&dir) {
            read_co(&dir, true, worker);
        }
    }
}

/// Read checkout DIR: FULL: branch, refs, dirty; else dirty only.
fn read_co(dir: &str, full: bool, worker: &mut Worker) {
    let path = Path::new(dir);

    if !full && let Some(co) = worker.cos.get_mut(dir) {
        if co.branch.is_some() {
            co.dirty = dirty_count(path);
        }

        return;
    }

    let (top, common, own) = crate::infra::watch::git_dirs(path)
        .map(|(top, common, own)| (canon(&top), canon(&common), canon(&own)))
        .unwrap_or_default();

    let (branch, def, plain) = match task_branch(path, None) {
        Ok((branch, def)) => (Some(branch), def, Land::NoCode),
        Err(state) => (None, None, state),
    };

    let (dirty, refs) = match &branch {
        Some(branch) => (dirty_count(path), refs(path, branch, def.as_deref(), &mut worker.seen)),
        None => (0, Refs::NoCode),
    };

    worker.cos.insert(
        dir.to_string(),
        Co {
            top,
            common,
            own,
            branch,
            plain,
            def,
            dirty,
            refs,
        },
    );

    // oids fix the answers: only the size needs a cap
    if worker.seen.len() > 4096 {
        worker.seen.clear();
    }
}

/// The state of thread ID from the last reads (0 git).
fn land(id: &str, worker: &Worker) -> Option<Land> {
    let thread = worker.threads.get(id)?;

    if thread.state == "archived" {
        return worker.kept.get(id).cloned();
    }

    let prs = prs_of(id);
    let lands = dirs(thread)
        .into_iter()
        .filter_map(|dir| worker.cos.get(&dir).cloned())
        .map(|co| match &co.branch {
            None => co.plain.clone(),
            Some(_) if co.dirty > 0 => Land::Dirty(co.dirty),
            Some(branch) => with_prs(&co.refs, branch, &prs),
        });

    Some(lands.min_by_key(Land::rank).unwrap_or(Land::NoCode))
}

/// Fetch each repo of the threads whose last fetch is older than `FETCH_EVERY` (1 at a time per
/// repo, off this thread). New remote refs come back as `Git` topics.
fn fetch_due(worker: &mut Worker) {
    let repos: HashSet<PathBuf> = worker
        .threads
        .values()
        .filter_map(crate::domain::threads::repo_of_thread_store)
        .collect();

    fetch_repositories(worker, repos);
}

fn fetch_repositories(worker: &mut Worker, repos: HashSet<PathBuf>) {
    for repo in repos {
        if worker
            .fetched
            .get(&repo)
            .is_some_and(|fetched_at| fetched_at.elapsed() < FETCH_EVERY)
        {
            continue;
        }

        worker.fetched.insert(repo.clone(), Instant::now());
        let busy = worker.fetching.clone();

        if !busy.lock().is_ok_and(|mut fetching| fetching.insert(repo.clone())) {
            continue;
        }

        std::thread::spawn(move || {
            fetch_origin(&repo);

            finish_repository_fetch(&repo, &busy);
        });
    }
}

fn finish_repository_fetch(repo: &PathBuf, busy: &Arc<std::sync::Mutex<HashSet<PathBuf>>>) {
    if let Ok(mut fetching) = busy.lock() {
        fetching.remove(repo);
    }
}

fn fetch_origin(repo: &Path) {
    if git(repo, &["remote", "get-url", "origin"]).0 {
        fetch(repo);
    }
}

/// Archive each open thread stale for `ARCHIVE_MS`; the next check at the next deadline.
fn auto_archive(worker: &mut Worker) {
    let now = crate::engine::now_ms();
    let mut next = u64::MAX;

    for (id, thread) in &worker.threads {
        // 0 code, agents not done / on main: archived at ACTIVITY + ARCHIVE_MS (stale since
        // STALE_MS)
        if thread.state == "archived"
            || !idle_kind(
                land(id, worker).as_ref(),
                crate::domain::notifications::reported_done(id),
            )
        {
            continue;
        }

        let activity = crate::domain::thread_card::last_item(id)
            .unwrap_or(0)
            .max(thread.updated);
        let due = activity + ARCHIVE_MS;

        if now > due {
            let _ = crate::domain::threads::archive(id);
        } else {
            next = next.min(due);
        }
    }

    // the next deadline (new activity moves it: checked then again); 0 deadline: 1 day (a
    // thread can turn 0 code later; a state change does not wake this timer)
    let wait = Duration::from_millis(next.saturating_sub(now)).min(Duration::from_millis(DAY_MS));

    worker.archive_at = Instant::now() + wait;
}

fn read_batch_changes(
    full: &HashSet<String>,
    dirty: &HashSet<String>,
    ids: HashSet<String>,
    gits: &HashSet<PathBuf>,
    worker: &mut Worker,
) -> Vec<String> {
    for dir in dirty.difference(full) {
        read_co(dir, false, worker);
    }

    let touched: HashSet<&String> = full.iter().chain(dirty).collect();
    let mut out: Vec<String> = ids.into_iter().collect();

    for (id, thread) in &worker.threads {
        let hit = if thread.state == "archived" {
            // its kept branch: a ref change in its repo
            archived_refs_changed(thread, gits)
        } else {
            dirs(thread).iter().any(|dir| touched.contains(dir))
        };

        append_changed_thread(&mut out, id, hit);
    }

    for id in out
        .iter()
        .filter(|id| worker.threads.get(*id).is_some_and(|thread| thread.state == "archived"))
        .cloned()
        .collect::<Vec<_>>()
    {
        read_thread(&id, worker);
    }

    out
}

fn collect_batch_changes(
    batch: Vec<In>,
    full: &mut HashSet<String>,
    dirty: &mut HashSet<String>,
    ids: &mut HashSet<String>,
    gits: &mut HashSet<PathBuf>,
    worker: &mut Worker,
) {
    for message in batch {
        match message {
            In::Bus(Topic::Thread(id, bus::Part::Meta)) => {
                reload_thread_record(full, ids, id, worker);
            }
            In::Bus(Topic::Thread(..)) => {}
            In::Bus(Topic::Git(common, part)) => {
                mark_repo_checkouts_full(full, gits, common, &part, worker);
            }
            In::Bus(Topic::Work(top)) => {
                mark_work_directory_dirty(dirty, &top, worker);
            }
            In::Bus(Topic::Rescan(root)) => {
                rescan_thread_records(full, ids, gits, root, worker);
            }
            In::Prs(id) => {
                ids.insert(id);
            }
            In::Refresh(id) => {
                refresh_thread_checkouts(full, ids, id, worker);
            }
            In::Visible => fetch_due(worker),
        }
    }
}

fn archived_refs_changed(thread: &store::Thread, gits: &HashSet<PathBuf>) -> bool {
    crate::domain::threads::repo_of_thread_store(thread)
        .and_then(|repo| crate::infra::watch::git_dirs(&repo))
        .is_some_and(|(_, common, _)| gits.iter().any(|git_dir| canon(&common).starts_with(git_dir)))
}
