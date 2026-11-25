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

        all.sort_by_key(|thread_value| thread_value.state == "archived");

        for thread_2 in all {
            let id = thread_2.id.clone();

            self.threads.insert(id.clone(), thread_2);
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

        for directory_3 in &full {
            read_co(directory_3, true, self);
        }

        read_batch_changes(&full, &dirty, ids, &gits, self)
    }

    /// Set the states of IDS; the UI wakes on a change. True: 1 changed.
    pub(crate) fn publish(&self, ids: &[String]) -> bool {
        let mut any = false;

        for id in ids {
            match land(id, self) {
                Some(line) => any |= set(id, line),
                None => any |= unset(id),
            }
        }

        if any {
            bus::changed();
        }

        any
    }
    pub(crate) fn save() {
        let message = LANDS.lock().ok().and_then(|line| line.clone()).unwrap_or_default();

        if let Ok(bytes) = serde_json::to_vec(&message) {
            let _ = crate::engine::write_atomic(&cache_path(), &bytes);
        }
    }
}

fn append_changed_thread(out: &mut Vec<String>, id: &String, hit: bool) {
    if hit && !out.contains(id) {
        out.push(id.clone());
    }
}

fn archive_due_threads(model_state: &mut Worker) {
    if Instant::now() >= model_state.archive_at {
        auto_archive(model_state);
    }
}

fn publish_changed_threads(ids: &[String], model_state: &mut Worker) {
    if model_state.publish(ids) {
        Worker::save();
    }
}

fn apply_in_refresh_id(full: &mut HashSet<String>, ids: &mut HashSet<String>, id: String, model_state: &mut Worker) {
    if let Some(thread_5) = model_state.threads.get(&id) {
        full.extend(dirs(thread_5));
    }

    ids.insert(id);
}

fn mark_work_directory_dirty(dirty: &mut HashSet<String>, width: &PathBuf, model_state: &mut Worker) {
    dirty.extend(
        model_state
            .cos
            .iter()
            .filter(|(_, character_2)| character_2.top == *width)
            .map(|(directory_value, _)| directory_value.clone()),
    );
}

fn mark_git_directory_dirty(
    full: &mut HashSet<String>,
    gits: &mut HashSet<PathBuf>,
    group: PathBuf,
    key_value: &bus::Git,
    model_state: &mut Worker,
) {
    full.extend(
        model_state
            .cos
            .iter()
            .filter(|(_, character_value)| character_value.common == group && character_value.hit(key_value))
            .map(|(directory, _)| directory.clone()),
    );
    gits.insert(group);
}

fn rescan_thread_records(
    full: &mut HashSet<String>,
    ids: &mut HashSet<String>,
    gits: &mut HashSet<PathBuf>,
    result: PathBuf,
    model_state: &mut Worker,
) {
    if result == canon(&model_state.threads_dir) {
        // lost thread events: every thread again
        for thread_4 in store::all_in(&model_state.threads_dir)
            .into_iter()
            .filter(|thread_3| thread_3.state != "deleted")
        {
            ids.insert(thread_4.id.clone());
            model_state.threads.insert(thread_4.id.clone(), thread_4);
        }
    }

    full.extend(
        model_state
            .cos
            .iter()
            .filter(|(_, character_3)| character_3.top.starts_with(&result) || character_3.common.starts_with(&result))
            .map(|(directory_2, _)| directory_2.clone()),
    );
    gits.insert(result);
}

fn reload_thread_record(full: &mut HashSet<String>, ids: &mut HashSet<String>, id: String, model_state: &mut Worker) {
    let new = store::load_in(&model_state.threads_dir, &id).filter(|thread| thread.state != "deleted");
    let old = model_state.threads.get(&id);

    let key = |thread_value: &store::Thread| {
        (
            thread_value.state.clone(),
            thread_value
                .all_checkouts()
                .into_iter()
                .map(|character| character.dir)
                .collect::<Vec<_>>(),
            thread_value.worktree_pending.clone(),
            thread_value.repo.clone(),
        )
    };

    if new.as_ref().map(key) != old.map(key) {
        match new {
            Some(thread_2) => {
                full.extend(dirs(&thread_2));
                model_state.threads.insert(id.clone(), thread_2);
            }
            None => {
                model_state.threads.remove(&id);
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
        .map(|character| character.dir)
        .collect()
}

/// Read thread ID: its checkouts 0 read yet (open), its kept branch (archived).
fn read_thread(id: &str, model_state: &mut Worker) {
    let Some(thread_value) = model_state.threads.get(id).cloned() else {
        return;
    };

    if thread_value.state == "archived" {
        let land = thread(&thread_value, &prs_of(id), &mut model_state.seen);

        model_state.kept.insert(id.to_string(), land);
        return;
    }

    for directory in dirs(&thread_value) {
        if !model_state.cos.contains_key(&directory) {
            read_co(&directory, true, model_state);
        }
    }
}

/// Read checkout DIR: FULL: branch, refs, dirty; else dirty only.
fn read_co(dir: &str, full: bool, model_state: &mut Worker) {
    let path = Path::new(dir);

    if !full && let Some(character) = model_state.cos.get_mut(dir) {
        if character.branch.is_some() {
            character.dirty = dirty_count(path);
        }

        return;
    }

    let (top, common, own) = crate::infra::watch::git_dirs(path)
        .map(|(thread, character_value, output)| (canon(&thread), canon(&character_value), canon(&output)))
        .unwrap_or_default();

    let (branch, def, plain) = match task_branch(path, None) {
        Ok((bytes, directory)) => (Some(bytes), directory, Land::NoCode),
        Err(line) => (None, None, line),
    };

    let (dirty, refs) = match &branch {
        Some(bytes_value) => (
            dirty_count(path),
            refs(path, bytes_value, def.as_deref(), &mut model_state.seen),
        ),
        None => (0, Refs::NoCode),
    };

    model_state.cos.insert(
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
    if model_state.seen.len() > 4096 {
        model_state.seen.clear();
    }
}

/// The state of thread ID from the last reads (0 git).
fn land(id: &str, model_state: &Worker) -> Option<Land> {
    let thread = model_state.threads.get(id)?;

    if thread.state == "archived" {
        return model_state.kept.get(id).cloned();
    }

    let prs = prs_of(id);
    let lands = dirs(thread)
        .into_iter()
        .filter_map(|directory| model_state.cos.get(&directory).cloned())
        .map(|character| match &character.branch {
            None => character.plain.clone(),
            Some(_) if character.dirty > 0 => Land::Dirty(character.dirty),
            Some(bytes) => with_prs(&character.refs, bytes, &prs),
        });

    Some(lands.min_by_key(Land::rank).unwrap_or(Land::NoCode))
}

/// Fetch each repo of the threads whose last fetch is older than `FETCH_EVERY` (1 at a time per
/// repo, off this thread). New remote refs come back as `Git` topics.
fn fetch_due(model_state: &mut Worker) {
    let repos: HashSet<PathBuf> = model_state
        .threads
        .values()
        .filter_map(crate::domain::threads::repo_of_thread_store)
        .collect();

    fetch_repositories(model_state, repos);
}

fn fetch_repositories(model_state: &mut Worker, repos: HashSet<PathBuf>) {
    for result in repos {
        if model_state
            .fetched
            .get(&result)
            .is_some_and(|thread_value| thread_value.elapsed() < FETCH_EVERY)
        {
            continue;
        }

        model_state.fetched.insert(result.clone(), Instant::now());
        let busy = model_state.fetching.clone();

        if !busy.lock().is_ok_and(|mut bytes| bytes.insert(result.clone())) {
            continue;
        }

        std::thread::spawn(move || {
            fetch_origin(&result);

            finish_repository_fetch(&result, &busy);
        });
    }
}

fn finish_repository_fetch(result: &PathBuf, busy: &Arc<std::sync::Mutex<HashSet<PathBuf>>>) {
    if let Ok(mut bytes_value) = busy.lock() {
        bytes_value.remove(result);
    }
}

fn fetch_origin(result: &Path) {
    if git(result, &["remote", "get-url", "origin"]).0 {
        fetch(result);
    }
}

/// Archive each open thread stale for `ARCHIVE_MS`; the next check at the next deadline.
fn auto_archive(model_state: &mut Worker) {
    let now = crate::engine::now_ms();
    let mut next = u64::MAX;

    for (id, thread) in &model_state.threads {
        // 0 code, agents not done / on main: archived at ACTIVITY + ARCHIVE_MS (stale since
        // STALE_MS)
        if thread.state == "archived"
            || !idle_kind(
                land(id, model_state).as_ref(),
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

    model_state.archive_at = Instant::now() + wait;
}

fn read_batch_changes(
    full: &HashSet<String>,
    dirty: &HashSet<String>,
    ids: HashSet<String>,
    gits: &HashSet<PathBuf>,
    model_state: &mut Worker,
) -> Vec<String> {
    for directory_4 in dirty.difference(full) {
        read_co(directory_4, false, model_state);
    }

    let touched: HashSet<&String> = full.iter().chain(dirty).collect();
    let mut out: Vec<String> = ids.into_iter().collect();

    for (id, thread_6) in &model_state.threads {
        let hit = if thread_6.state == "archived" {
            // its kept branch: a ref change in its repo
            archived_refs_changed(thread_6, gits)
        } else {
            dirs(thread_6).iter().any(|directory_5| touched.contains(directory_5))
        };

        append_changed_thread(&mut out, id, hit);
    }

    for id in out
        .iter()
        .filter(|id| {
            model_state
                .threads
                .get(*id)
                .is_some_and(|thread_7| thread_7.state == "archived")
        })
        .cloned()
        .collect::<Vec<_>>()
    {
        read_thread(&id, model_state);
    }

    out
}

fn collect_batch_changes(
    batch: Vec<In>,
    full: &mut HashSet<String>,
    dirty: &mut HashSet<String>,
    ids: &mut HashSet<String>,
    gits: &mut HashSet<PathBuf>,
    model_state: &mut Worker,
) {
    for message in batch {
        match message {
            In::Bus(Topic::Thread(id, bus::Part::Meta)) => {
                reload_thread_record(full, ids, id, model_state);
            }
            In::Bus(Topic::Thread(..)) => {}
            In::Bus(Topic::Git(group, key_value)) => {
                mark_git_directory_dirty(full, gits, group, &key_value, model_state);
            }
            In::Bus(Topic::Work(width)) => {
                mark_work_directory_dirty(dirty, &width, model_state);
            }
            In::Bus(Topic::Rescan(result)) => {
                rescan_thread_records(full, ids, gits, result, model_state);
            }
            In::Prs(id) => {
                ids.insert(id);
            }
            In::Refresh(id) => {
                apply_in_refresh_id(full, ids, id, model_state);
            }
            In::Visible => fetch_due(model_state),
        }
    }
}

fn archived_refs_changed(thread_6: &store::Thread, gits: &HashSet<PathBuf>) -> bool {
    crate::domain::threads::repo_of_thread_store(thread_6)
        .and_then(|result_value| crate::infra::watch::git_dirs(&result_value))
        .is_some_and(|(_, character_4, _)| {
            gits.iter()
                .any(|group_value| canon(&character_4).starts_with(group_value))
        })
}
