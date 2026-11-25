use super::{
    ControlFlow, Git, HashMap, HashSet, Msg, Part, PathBuf, Receiver, Topic, Watch, Watcher, bus, canon, check_ignore,
    roots, store,
};
use std::path::Path;
/// The watcher thread: roots, classify, publish.
pub(super) fn run(mut watcher: notify::RecommendedWatcher, threads: &Path, rx: &Receiver<Msg>) {
    let threads = canon(threads);
    let _ = std::fs::create_dir_all(&threads);
    let mut watch = Watch {
        threads: threads.clone(),
        gits: Vec::new(),
        works: Vec::new(),
        plain: HashSet::new(),
        ignores: HashMap::new(),
    };
    let mut roots_watched: HashSet<PathBuf> = HashSet::new();
    let mut shown: HashMap<u64, PathBuf> = HashMap::new();
    let mut all: HashMap<String, store::Thread> = store::all_in(&threads)
        .into_iter()
        .map(|thread| (thread.id.clone(), thread))
        .collect();
    let mut roots_due = true;

    loop {
        if let ControlFlow::Break(()) = run_once(WatchLoop {
            watcher: &mut watcher,
            rx,
            threads: &threads,
            watch: &mut watch,
            roots_watched: &mut roots_watched,
            shown: &mut shown,
            all: &mut all,
            roots_due: &mut roots_due,
        }) {
            return;
        }
    }
}

fn run_once(settings: WatchLoop<'_>) -> ControlFlow<()> {
    let WatchLoop {
        watcher,
        rx,
        threads,
        watch,
        roots_watched,
        shown,
        all,
        roots_due,
    } = settings;

    if *roots_due {
        *roots_due = false;
        update_watch_roots(watcher, threads, watch, roots_watched, shown, all);
    }

    let Ok(first) = rx.recv() else {
        return ControlFlow::Break(());
    };

    let paths = Vec::new();

    handle_events(
        first,
        paths,
        WatchEvents {
            rx,
            threads,
            watch,
            roots_watched,
            shown,
            all,
            roots_due,
        },
    )
}

fn handle_events(first: Msg, mut paths: Vec<PathBuf>, settings: WatchEvents<'_>) -> ControlFlow<()> {
    let WatchEvents {
        rx,
        threads,
        watch,
        roots_watched,
        shown,
        all,
        roots_due,
    } = settings;

    for message in std::iter::once(first).chain(std::iter::from_fn(|| rx.try_recv().ok())) {
        match message {
            Msg::Paths(changed) => paths.extend(changed),
            Msg::Rescan => {
                *all = store::all_in(threads)
                    .into_iter()
                    .map(|thread| (thread.id.clone(), thread))
                    .collect();
                *roots_due = true;

                publish_rescans(&*roots_watched);
            }
            Msg::Show(token, directory) => {
                *roots_due |= update_shown(shown, token, directory);
            }
        }
    }

    for topic in watch.topics(&paths, &mut check_ignore) {
        // a thread's checkouts or a branch switch: other roots
        if let Topic::Thread(id, Part::Meta) = &topic {
            match store::load_in(threads, id) {
                Some(thread) => all.insert(id.clone(), thread),
                None => all.remove(id),
            };

            *roots_due = true;
        }

        *roots_due |= matches!(topic, Topic::Git(_, Git::Worktree(_)));
        bus::publish(&(topic));
    }

    ControlFlow::Continue(())
}

struct WatchLoop<'context> {
    watcher: &'context mut notify::RecommendedWatcher,
    rx: &'context Receiver<Msg>,
    threads: &'context PathBuf,
    watch: &'context mut Watch,
    roots_watched: &'context mut HashSet<PathBuf>,
    shown: &'context mut HashMap<u64, PathBuf>,
    all: &'context mut HashMap<String, store::Thread>,
    roots_due: &'context mut bool,
}

struct WatchEvents<'context> {
    rx: &'context Receiver<Msg>,
    threads: &'context Path,
    watch: &'context mut Watch,
    roots_watched: &'context mut HashSet<PathBuf>,
    shown: &'context mut HashMap<u64, PathBuf>,
    all: &'context mut HashMap<String, store::Thread>,
    roots_due: &'context mut bool,
}

fn update_watch_roots(
    watcher: &mut notify::RecommendedWatcher,
    threads: &Path,
    watch: &mut Watch,
    roots_watched: &mut HashSet<PathBuf>,
    shown: &HashMap<u64, PathBuf>,
    all: &HashMap<String, store::Thread>,
) {
    let list: Vec<store::Thread> = all.values().cloned().collect();
    let views: Vec<PathBuf> = shown.values().cloned().collect();
    let (gits, works) = roots(&list, &views);

    watch.set(gits, works);
    let want: HashSet<PathBuf> = std::iter::once(threads.to_path_buf())
        .chain(watch.gits.iter().cloned())
        .chain(watch.works.iter().cloned())
        .collect();
    let mut ops: Vec<notify::PathOp> = roots_watched
        .difference(&want)
        .map(|path| notify::PathOp::unwatch(path.clone()))
        .collect();
    let added: Vec<PathBuf> = want.difference(&*roots_watched).cloned().collect();

    ops.extend(added.iter().map(|path| notify::PathOp::watch_recursive(path.clone())));

    if !ops.is_empty() {
        if let Err(error) = watcher.update_paths(ops) {
            crate::engine::trace(&format!("watch: {error}"));
        }

        // new roots: what happened before their watch is unknown (the 1st set: the
        // readers read everything at their start)
        if !roots_watched.is_empty() {
            publish_rescans(&added);
        }

        *roots_watched = want;
    }
}

fn update_shown(shown: &mut HashMap<u64, PathBuf>, token: u64, dir: Option<PathBuf>) -> bool {
    match dir {
        Some(dir) => shown.insert(token, dir.clone()).as_ref().is_none_or(|old| *old != dir),
        None => shown.remove(&token).is_some(),
    }
}

fn publish_rescans<'path>(paths: impl IntoIterator<Item = &'path PathBuf>) {
    for path in paths {
        bus::publish(&Topic::Rescan(path.clone()));
    }
}
