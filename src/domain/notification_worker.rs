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
            let open =
                |thread_value: &store::Thread| thread_value.state != "archived" && thread_value.state != "deleted";
            let character = Cache {
                logs: HashMap::new(),
                prs: HashMap::new(),
                first: load_map("prs.json"),
                replies: HashMap::new(),
            };
            let threads: HashMap<String, store::Thread> = store::all()
                .into_iter()
                .filter(open)
                .map(|thread_2| (thread_2.id.clone(), thread_2))
                .collect();
            let due: HashSet<String> = threads.keys().cloned().collect();
            let pr_at: Option<Instant> = None;

            start_loop(&topics, &open, character, threads, due, pr_at);
        })
        .ok();
}

fn start_loop(
    topics: &std::sync::mpsc::Receiver<crate::infra::bus::Topic>,
    open: &impl Fn(&store::Thread) -> bool,
    mut character: Cache,
    mut threads: HashMap<String, store::Thread>,
    mut due: HashSet<String>,
    mut pr_at: Option<Instant>,
) {
    loop {
        poll_due_pull_requests(&mut character, &threads, &mut pr_at);

        read_due_threads(&mut character, &threads, &mut due);

        publish(&threads, &character);

        // the next topic, or the next PR pass
        let wait = pr_at.map_or(Duration::ZERO, |thread_5| PR_EVERY.saturating_sub(thread_5.elapsed()));

        let first = match topics.recv_timeout(wait) {
            Ok(thread_6) => Some(thread_6),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => None,
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
        };

        for thread_7 in first.into_iter().chain(std::iter::from_fn(|| topics.try_recv().ok())) {
            dispatch_topic(thread_7, open, &mut threads, &mut due);
        }
    }
}

fn read_due_threads(character: &mut Cache, threads: &HashMap<String, store::Thread>, due: &mut HashSet<String>) {
    for id in due.drain() {
        let Some(thread_4) = threads.get(&id) else {
            character.logs.remove(&id);
            character.replies.remove(&id);
            continue;
        };

        read_thread(thread_4, character);
    }
}

fn start_topic_rescan_result_if(
    open: impl Fn(&store::Thread) -> bool,
    threads: &mut HashMap<String, store::Thread>,
    due: &mut HashSet<String>,
) {
    *threads = store::all()
        .into_iter()
        .filter(open)
        .map(|thread_10| (thread_10.id.clone(), thread_10))
        .collect();
    due.extend(threads.keys().cloned());
}

fn poll_due_pull_requests(
    character: &mut Cache,
    threads: &HashMap<String, store::Thread>,
    pr_at: &mut Option<Instant>,
) {
    if pr_at.is_none_or(|thread_3| thread_3.elapsed() > PR_EVERY) {
        *pr_at = Some(Instant::now());
        let list: Vec<store::Thread> = threads.values().cloned().collect();

        pr_pass(&list, character);
    }
}

fn start_topic_thread_id_part(
    open: impl Fn(&store::Thread) -> bool,
    threads: &mut HashMap<String, store::Thread>,
    due: &mut HashSet<String>,
    id: String,
) {
    match store::load(&id).filter(|thread_8| open(thread_8) && thread_8.of.is_none()) {
        Some(thread_9) => threads.insert(id.clone(), thread_9),
        None => threads.remove(&id),
    };

    due.insert(id);
}
/// Read thread T's log (when it changed) and its comment replies (when the file changed).
pub(super) fn read_thread(thread_value: &store::Thread, character: &mut Cache) {
    let path = store::events_path(&thread_value.id);

    if let Ok(message) = std::fs::metadata(&path) {
        let key = (message.len(), message.modified().unwrap_or(SystemTime::UNIX_EPOCH));

        if character.logs.get(&thread_value.id).is_none_or(|log| log.stamp != key) {
            let ev = store::events_tail(&thread_value.id, 512 << 10);
            let end = ev.last().map_or(0, |error| error.ts);
            let dir = thread_value.work_dir();
            let notes = derive(&ev, &thread_value.agents)
                .into_iter()
                .map(|(kind, agents, ts, text)| Note {
                    thread: thread_value.id.clone(),
                    title: thread_value.title.clone(),
                    dir: dir.clone(),
                    kind,
                    agents,
                    ts,
                    text,
                    comment: String::new(),
                })
                .collect();

            character.logs.insert(
                thread_value.id.clone(),
                LogSnapshot {
                    stamp: key,
                    notes,
                    latest: end,
                },
            );
        }
    }

    // unseen agent replies on comments: read when the comments file changed
    let at = crate::domain::changes::comments_stamp(&thread_value.id);

    if character
        .replies
        .get(&thread_value.id)
        .is_none_or(|result| result.0 != at)
    {
        character
            .replies
            .insert(thread_value.id.clone(), (at, reply_notes(thread_value)));
    }
}
/// The notes of every thread from the caches (0 read); a change wakes the UI.
pub(super) fn publish(threads: &HashMap<String, store::Thread>, character: &Cache) {
    let mut all = Vec::new();
    let mut last = HashMap::new();
    let mut ids: Vec<&String> = threads.keys().collect();

    // a fixed order: the same notes compare equal
    ids.sort_by_key(|id| std::cmp::Reverse(threads[*id].updated));

    for id in ids {
        all.extend(
            character
                .replies
                .get(id)
                .into_iter()
                .flat_map(|result| result.1.iter().cloned()),
        );

        if let Some(log) = character.logs.get(id) {
            all.extend(log.notes.iter().cloned());
            last.insert(id.clone(), log.latest);
        }

        all.extend(character.prs.get(id).into_iter().flatten().cloned());
    }

    if let Ok(mut line) = LAST.get_or_init(|| Mutex::new(HashMap::new())).lock() {
        *line = last;
    }

    if let Ok(mut count) = NOTES.lock()
        && *count != all
    {
        *count = all;
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

pub(super) fn pr_pass(threads: &[store::Thread], character: &mut Cache) {
    let now = crate::engine::now_ms();
    let mut live: HashSet<String> = HashSet::new();

    character.prs.clear();

    for thread_value in threads {
        let mut snapshot = PullRequestNotes::default();
        let prs = crate::domain::pull_requests::remembered(&thread_value.id);

        read_pull_request_notes(character, now, &mut live, thread_value, &mut snapshot, &prs);

        crate::domain::landed::set_prs(&thread_value.id, &snapshot.seen);

        // work after the merge (new commits, files): 0 archive offer
        if all_merged(&snapshot.states) && !crate::domain::landed::of(&thread_value.id).is_some_and(|line| line.open())
        {
            let key = format!("merged:{}", thread_value.id);
            let ts = *character.first.entry(key.clone()).or_insert(now);

            live.insert(key);

            let text = text(&prs);

            snapshot.notes.push(Note {
                thread: thread_value.id.clone(),
                title: thread_value.title.clone(),
                dir: thread_value.work_dir(),
                kind: Kind::Merged,
                agents: Vec::new(),
                ts,
                text,
                comment: String::new(),
            });
        }

        if !snapshot.notes.is_empty() {
            character.prs.insert(thread_value.id.clone(), snapshot.notes);
        }
    }

    let before = character.first.len();

    character.first.retain(|key_value, _| live.contains(key_value));

    if character.first.len() != before || !live.is_empty() {
        save_map("prs.json", &character.first);
    }
}

fn read_pull_request_notes(
    character: &mut Cache,
    now: u64,
    live: &mut HashSet<String>,
    thread_value: &store::Thread,
    snapshot: &mut PullRequestNotes,
    prs: &[crate::infra::gh::PrRef],
) {
    for result in prs {
        let Ok(path) = gh::fetch(result) else {
            // a PR not read: 0 verdict on the thread
            snapshot.states.push(gh::State::Open);
            continue;
        };

        snapshot.states.push(path.state);
        snapshot.seen.push(crate::domain::landed::PrSeen {
            label: result.label(),
            state: path.state,
            head: path.head.clone(),
            head_oid: path.head_oid.clone(),
        });

        if let ControlFlow::Break(()) = skip_closed_pull_request(&path) {
            continue;
        }

        let add = |kind: Kind, key: String, ts: Option<u64>, text: String| {
            let ts = ts.unwrap_or_else(|| *character.first.entry(key.clone()).or_insert(now));

            live.insert(key);
            snapshot.notes.push(Note {
                thread: thread_value.id.clone(),
                title: thread_value.title.clone(),
                dir: thread_value.work_dir(),
                kind,
                agents: Vec::new(),
                ts,
                text,
                comment: String::new(),
            });
        };

        report_pull_request(result, &path, add);
    }
}

fn report_pull_request(result: &gh::PrRef, path: &gh::Pr, mut add: impl FnMut(Kind, String, Option<u64>, String)) {
    if path.conflicts() {
        add(
            Kind::Conflict,
            format!("conflict:{}", result.key()),
            None,
            format!("{}: merge conflict with {}", result.label(), path.base),
        );
    }

    let failed: Vec<&str> = path
        .checks
        .iter()
        .filter(|x| x.verdict == gh::Verdict::Fail)
        .map(|x| x.name.as_str())
        .collect();

    if !failed.is_empty() {
        add(
            Kind::Ci,
            format!("ci:{}:{}", result.key(), path.head_oid),
            None,
            format!("{}: {} failed", result.label(), failed.join(", ")),
        );
    }

    let latest = path
        .threads
        .iter()
        .flat_map(|thread_2| std::iter::once(&thread_2.root).chain(&thread_2.replies))
        .filter(|message| !message.mine)
        .max_by_key(|message_value| message_value.created);

    if let Some(message_2) = latest {
        add(
            Kind::Review,
            String::new(),
            Some(message_2.created * 1000),
            format!(
                "{}: {}: {}",
                result.label(),
                message_2.author,
                first_line(&message_2.body)
            ),
        );
    }
}

fn text(prs: &[crate::infra::gh::PrRef]) -> String {
    if prs.len() == 1 {
        format!("{} merged: archive the thread?", prs[0].label())
    } else {
        format!("all {} PRs merged / closed: archive the thread?", prs.len())
    }
}

fn skip_closed_pull_request(path: &crate::infra::gh::Pr) -> ControlFlow<()> {
    if !matches!(
        path.state,
        crate::infra::gh::State::Open | crate::infra::gh::State::Draft
    ) {
        return ControlFlow::Break(());
    }

    ControlFlow::Continue(())
}

fn dispatch_topic(
    thread_7: Topic,
    open: &impl Fn(&store::Thread) -> bool,
    threads: &mut HashMap<String, store::Thread>,
    due: &mut HashSet<String>,
) {
    match thread_7 {
        Topic::Thread(id, Part::Meta) => {
            start_topic_thread_id_part(open, threads, due, id);
        }
        Topic::Thread(id, Part::Log | Part::Comments) if threads.contains_key(&id) => {
            due.insert(id);
        }
        Topic::Rescan(result) if result == crate::infra::watch::canon(&store::threads_dir()) => {
            start_topic_rescan_result_if(open, threads, due);
        }
        _ => {}
    }
}
