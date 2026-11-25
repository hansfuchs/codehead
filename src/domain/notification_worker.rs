use super::LogSnapshot;
use crate::infra::bus::{Part, Topic};
use crate::infra::gh;
use std::ops::ControlFlow;

use super::{
    Cache, Duration, HashMap, HashSet, Instant, Kind, LAST, Mutex, NOTES, Note, OnceLock, Ordering, PR_EVERY,
    SystemTime, VERSION, all_merged, derive, first_line, load_map, reply_notes, save_map, store,
};

/// Start the background poll (once per process).
pub(crate) fn start() {
    static ON: OnceLock<()> = OnceLock::new();

    if cfg!(test) || ON.set(()).is_err() {
        return;
    }

    // event driven: bus topics name the threads to read again; the PR pass (GitHub: 0 push) on
    // its own timer
    let topics = crate::infra::bus::subscribe();

    std::thread::Builder::new()
        .name("notify".into())
        .spawn(move || {
            let open = |thread: &store::Thread| thread.state != "archived" && thread.state != "deleted";
            let cache = Cache {
                logs: HashMap::new(),
                prs: HashMap::new(),
                first: load_map("prs.json"),
                replies: HashMap::new(),
            };
            let threads: HashMap<String, store::Thread> = store::all()
                .into_iter()
                .filter(open)
                .map(|thread| (thread.id.clone(), thread))
                .collect();
            let due: HashSet<String> = threads.keys().cloned().collect();
            let pr_at: Option<Instant> = None;

            poll_loop(&topics, &open, cache, threads, due, pr_at);
        })
        .ok();
}

fn poll_loop(
    topics: &std::sync::mpsc::Receiver<crate::infra::bus::Topic>,
    open: &impl Fn(&store::Thread) -> bool,
    mut cache: Cache,
    mut threads: HashMap<String, store::Thread>,
    mut due: HashSet<String>,
    mut pr_at: Option<Instant>,
) {
    loop {
        poll_due_pull_requests(&mut cache, &threads, &mut pr_at);

        read_due_threads(&mut cache, &threads, &mut due);

        publish(&threads, &cache);

        // the next topic, or the next PR pass
        let wait = pr_at.map_or(Duration::ZERO, |at| PR_EVERY.saturating_sub(at.elapsed()));

        let first = match topics.recv_timeout(wait) {
            Ok(topic) => Some(topic),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => None,
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
        };

        for topic in first.into_iter().chain(std::iter::from_fn(|| topics.try_recv().ok())) {
            dispatch_topic(topic, open, &mut threads, &mut due);
        }
    }
}

fn read_due_threads(cache: &mut Cache, threads: &HashMap<String, store::Thread>, due: &mut HashSet<String>) {
    for id in due.drain() {
        let Some(thread) = threads.get(&id) else {
            cache.logs.remove(&id);
            cache.replies.remove(&id);
            continue;
        };

        read_thread(thread, cache);
    }
}

fn reload_open_threads(
    open: impl Fn(&store::Thread) -> bool,
    threads: &mut HashMap<String, store::Thread>,
    due: &mut HashSet<String>,
) {
    *threads = store::all()
        .into_iter()
        .filter(open)
        .map(|thread| (thread.id.clone(), thread))
        .collect();
    due.extend(threads.keys().cloned());
}

fn poll_due_pull_requests(cache: &mut Cache, threads: &HashMap<String, store::Thread>, pr_at: &mut Option<Instant>) {
    if pr_at.is_none_or(|at| at.elapsed() > PR_EVERY) {
        *pr_at = Some(Instant::now());
        let list: Vec<store::Thread> = threads.values().cloned().collect();

        pr_pass(&list, cache);
    }
}

fn reload_thread(
    open: impl Fn(&store::Thread) -> bool,
    threads: &mut HashMap<String, store::Thread>,
    due: &mut HashSet<String>,
    id: String,
) {
    match store::load(&id).filter(|thread| open(thread) && thread.of.is_none()) {
        Some(thread) => threads.insert(id.clone(), thread),
        None => threads.remove(&id),
    };

    due.insert(id);
}
/// Read thread T's log (when it changed) and its comment replies (when the file changed).
pub(super) fn read_thread(thread: &store::Thread, cache: &mut Cache) {
    let path = store::events_path(&thread.id);

    if let Ok(meta) = std::fs::metadata(&path) {
        let key = (meta.len(), meta.modified().unwrap_or(SystemTime::UNIX_EPOCH));

        if cache.logs.get(&thread.id).is_none_or(|log| log.stamp != key) {
            let ev = store::events_tail(&thread.id, 512 << 10);
            let end = ev.last().map_or(0, |event| event.ts);
            let dir = thread.work_dir();
            let notes = derive(&ev, &thread.agents)
                .into_iter()
                .map(|(kind, agents, ts, text)| Note {
                    thread: thread.id.clone(),
                    title: thread.title.clone(),
                    dir: dir.clone(),
                    kind,
                    agents,
                    ts,
                    text,
                    comment: String::new(),
                })
                .collect();

            cache.logs.insert(
                thread.id.clone(),
                LogSnapshot {
                    stamp: key,
                    notes,
                    latest: end,
                },
            );
        }
    }

    // unseen agent replies on comments: read when the comments file changed
    let at = crate::domain::changes::comments_stamp(&thread.id);

    if cache.replies.get(&thread.id).is_none_or(|cached| cached.0 != at) {
        cache.replies.insert(thread.id.clone(), (at, reply_notes(thread)));
    }
}
/// The notes of every thread from the caches (0 read); a change wakes the UI.
pub(super) fn publish(threads: &HashMap<String, store::Thread>, cache: &Cache) {
    let mut all = Vec::new();
    let mut last = HashMap::new();
    let mut ids: Vec<&String> = threads.keys().collect();

    // a fixed order: the same notes compare equal
    ids.sort_by_key(|id| std::cmp::Reverse(threads[*id].updated));

    for id in ids {
        all.extend(
            cache
                .replies
                .get(id)
                .into_iter()
                .flat_map(|cached| cached.1.iter().cloned()),
        );

        if let Some(log) = cache.logs.get(id) {
            all.extend(log.notes.iter().cloned());
            last.insert(id.clone(), log.latest);
        }

        all.extend(cache.prs.get(id).into_iter().flatten().cloned());
    }

    if let Ok(mut latest) = LAST.get_or_init(|| Mutex::new(HashMap::new())).lock() {
        *latest = last;
    }

    if let Ok(mut notes) = NOTES.lock()
        && *notes != all
    {
        *notes = all;
        VERSION.fetch_add(1, Ordering::Relaxed);
        crate::infra::bus::wake();
    }
}
/// The remembered PRs of each thread (the merge tab found them): conflicts, failed checks, the
/// latest comment of someone else.
#[derive(Default)]
struct PullRequestNotes {
    notes: Vec<Note>,
    states: Vec<gh::State>,
    seen: Vec<crate::domain::landed::PrSeen>,
}

pub(super) fn pr_pass(threads: &[store::Thread], cache: &mut Cache) {
    let now = crate::engine::now_ms();
    let mut live: HashSet<String> = HashSet::new();

    cache.prs.clear();

    for thread in threads {
        let mut snapshot = PullRequestNotes::default();
        let prs = crate::domain::pull_requests::remembered(&thread.id);

        read_pull_request_notes(cache, now, &mut live, thread, &mut snapshot, &prs);

        crate::domain::landed::set_prs(&thread.id, &snapshot.seen);

        // work after the merge (new commits, files): 0 archive offer
        if all_merged(&snapshot.states) && !crate::domain::landed::of(&thread.id).is_some_and(|land| land.open()) {
            let key = format!("merged:{}", thread.id);
            let ts = *cache.first.entry(key.clone()).or_insert(now);

            live.insert(key);

            let text = merged_text(&prs);

            snapshot.notes.push(Note {
                thread: thread.id.clone(),
                title: thread.title.clone(),
                dir: thread.work_dir(),
                kind: Kind::Merged,
                agents: Vec::new(),
                ts,
                text,
                comment: String::new(),
            });
        }

        if !snapshot.notes.is_empty() {
            cache.prs.insert(thread.id.clone(), snapshot.notes);
        }
    }

    let before = cache.first.len();

    cache.first.retain(|key, _| live.contains(key));

    if cache.first.len() != before || !live.is_empty() {
        save_map("prs.json", &cache.first);
    }
}

fn read_pull_request_notes(
    cache: &mut Cache,
    now: u64,
    live: &mut HashSet<String>,
    thread: &store::Thread,
    snapshot: &mut PullRequestNotes,
    prs: &[crate::infra::gh::PrRef],
) {
    for pr_ref in prs {
        let Ok(pr) = gh::fetch(pr_ref) else {
            // a PR not read: 0 verdict on the thread
            snapshot.states.push(gh::State::Open);
            continue;
        };

        snapshot.states.push(pr.state);
        snapshot.seen.push(crate::domain::landed::PrSeen {
            label: pr_ref.label(),
            state: pr.state,
            head: pr.head.clone(),
            head_oid: pr.head_oid.clone(),
        });

        if let ControlFlow::Break(()) = skip_closed_pull_request(&pr) {
            continue;
        }

        let add = |kind: Kind, key: String, ts: Option<u64>, text: String| {
            let ts = ts.unwrap_or_else(|| *cache.first.entry(key.clone()).or_insert(now));

            live.insert(key);
            snapshot.notes.push(Note {
                thread: thread.id.clone(),
                title: thread.title.clone(),
                dir: thread.work_dir(),
                kind,
                agents: Vec::new(),
                ts,
                text,
                comment: String::new(),
            });
        };

        report_pull_request(pr_ref, &pr, add);
    }
}

fn report_pull_request(pr_ref: &gh::PrRef, pr: &gh::Pr, mut add: impl FnMut(Kind, String, Option<u64>, String)) {
    if pr.conflicts() {
        add(
            Kind::Conflict,
            format!("conflict:{}", pr_ref.key()),
            None,
            format!("{}: merge conflict with {}", pr_ref.label(), pr.base),
        );
    }

    let failed: Vec<&str> = pr
        .checks
        .iter()
        .filter(|check| check.verdict == gh::Verdict::Fail)
        .map(|check| check.name.as_str())
        .collect();

    if !failed.is_empty() {
        add(
            Kind::Ci,
            format!("ci:{}:{}", pr_ref.key(), pr.head_oid),
            None,
            format!("{}: {} failed", pr_ref.label(), failed.join(", ")),
        );
    }

    let latest = pr
        .threads
        .iter()
        .flat_map(|thread| std::iter::once(&thread.root).chain(&thread.replies))
        .filter(|comment| !comment.mine)
        .max_by_key(|comment| comment.created);

    if let Some(comment) = latest {
        add(
            Kind::Review,
            String::new(),
            Some(comment.created * 1000),
            format!("{}: {}: {}", pr_ref.label(), comment.author, first_line(&comment.body)),
        );
    }
}

fn merged_text(prs: &[crate::infra::gh::PrRef]) -> String {
    if prs.len() == 1 {
        format!("{} merged: archive the thread?", prs[0].label())
    } else {
        format!("all {} PRs merged / closed: archive the thread?", prs.len())
    }
}

fn skip_closed_pull_request(pr: &crate::infra::gh::Pr) -> ControlFlow<()> {
    if !matches!(pr.state, crate::infra::gh::State::Open | crate::infra::gh::State::Draft) {
        return ControlFlow::Break(());
    }

    ControlFlow::Continue(())
}

fn dispatch_topic(
    topic: Topic,
    open: &impl Fn(&store::Thread) -> bool,
    threads: &mut HashMap<String, store::Thread>,
    due: &mut HashSet<String>,
) {
    match topic {
        Topic::Thread(id, Part::Meta) => {
            reload_thread(open, threads, due, id);
        }
        Topic::Thread(id, Part::Log | Part::Comments) if threads.contains_key(&id) => {
            due.insert(id);
        }
        Topic::Rescan(root) if root == crate::infra::watch::canon(&store::threads_dir()) => {
            reload_open_threads(open, threads, due);
        }
        _ => {}
    }
}
