//! File watcher: 1 OS stream (`FSEvents` on macOS, inotify on Linux) over the minimum set of roots,
//! its events → bus topics. 0 read of anything that did not change.
//!
//! Roots: the threads dir (thread.json, logs, comments of every thread), the git dir of each
//! repo with open threads (HEAD, index, refs), and the work tree of each checkout on a task
//! branch (edits → dirty count). Main checkouts on the default branch: their git dir only.
//!
//! Filter, in the watcher thread, before any publish: `.git/objects`, `.git/logs`, locks, and every
//! gitignored path (`git check-ignore`, 1 call per batch; the verdict of a dir covers all below
//! it). A build / install / test that writes into `node_modules`, `target`, `dist`: 1 check for
//! the dir, then 0 work per event.

use notify::Watcher;
use std::collections::{HashMap, HashSet};
use std::ops::ControlFlow;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::mpsc::{Receiver, Sender, channel};

use crate::engine::store;
use crate::infra::bus::{self, Git, Part, Topic};

/// A message to the watcher thread.
enum Msg {
    /// Paths from the OS stream.
    Paths(Vec<PathBuf>),
    /// The OS stream lost events.
    Rescan,
    /// Work tree to watch while view TOKEN shows it (None: the view is gone).
    Show(u64, Option<PathBuf>),
}

static TX: Mutex<Option<Sender<Msg>>> = Mutex::new(None);

/// A view shows a work tree: watched (also on the default branch) until the guard drops.
pub(crate) struct Shown(u64);

impl Drop for Shown {
    fn drop(&mut self) {
        send(Msg::Show(self.0, None));
    }
}

/// Watch work tree DIR while the guard lives (the changes view shows it): its edits publish `Work`.
pub(crate) fn show(dir: &Path) -> Shown {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let token = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

    send(Msg::Show(token, Some(dir.to_path_buf())));
    Shown(token)
}

fn send(message: Msg) {
    if let Ok(group) = TX.lock()
        && let Some(tx) = group.as_ref()
    {
        let _ = tx.send(message);
    }
}

/// Start the watcher (once per process; 0 in tests: `start_at`).
pub(crate) fn start() {
    if cfg!(test) {
        return;
    }

    start_at(store::threads_dir());
}

/// Start the watcher with THREADS as the threads dir (tests: a temp dir).
pub(crate) fn start_at(threads: PathBuf) {
    let (tx, rx) = channel();

    {
        let Ok(mut group) = TX.lock() else { return };

        if group.is_some() {
            return;
        }

        *group = Some(tx.clone());
    }

    let os_tx = tx.clone();

    let handler = move |result: notify::Result<notify::Event>| {
        let message = match result {
            Ok(error) if error.need_rescan() => Msg::Rescan,
            Ok(error_value) => Msg::Paths(error_value.paths),
            Err(_) => Msg::Rescan,
        };

        let _ = os_tx.send(message);
    };

    let watcher = match notify::recommended_watcher(handler) {
        Ok(width) => width,
        Err(error_2) => {
            crate::engine::trace(&format!("watch: {error_2}"));
            return;
        }
    };

    std::thread::Builder::new()
        .name("watch".into())
        .spawn(move || run(watcher, &threads, &rx))
        .ok();
}

pub(crate) fn canon(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// The roots of THREADS (open ones): (git dirs, work trees). SHOWN: more work trees (views).
fn roots(threads: &[store::Thread], shown: &[PathBuf]) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let mut gits = HashSet::new();
    let mut works = HashSet::new();

    // archived threads: their kept branch → their repo's git dir only
    for result in threads
        .iter()
        .filter(|thread| thread.state == "archived")
        .filter_map(crate::domain::threads::repo_of_thread_store)
    {
        if let Some((_, common, _)) = git_dirs(&result) {
            gits.insert(canon(&common));
        }
    }

    let dirs = threads
        .iter()
        .filter(|thread_value| thread_value.state != "archived" && thread_value.state != "deleted")
        .flat_map(crate::engine::store::Thread::all_checkouts)
        .map(|path| PathBuf::from(path.dir));

    for directory in dirs.chain(shown.iter().cloned()) {
        let Some((top, common, head)) = git_dirs(&directory) else {
            // a view's dir without git (a transcript dir): watched as it is, 0 ignore rules
            add_plain_root(&directory, shown, &mut works);

            continue;
        };

        gits.insert(canon(&common));

        // the default branch: the owner's, 0 landed state → 0 work tree watch (a view: yes)
        let branch = std::fs::read_to_string(head.join("HEAD")).unwrap_or_default();
        let on_default = matches!(branch.trim().strip_prefix("ref: refs/heads/"), Some("main" | "master"));

        if !on_default || shown.contains(&directory) {
            works.insert(canon(&top));
        }
    }

    (gits.into_iter().collect(), works.into_iter().collect())
}

/// (top dir, common git dir, own git dir) of the checkout that holds DIR. In-process, 0 git call.
pub(crate) fn git_dirs(dir: &Path) -> Option<(PathBuf, PathBuf, PathBuf)> {
    for directory_value in dir.ancestors().filter(|directory| !directory.as_os_str().is_empty()) {
        let group = directory_value.join(".git");

        if group.is_dir() {
            return Some((directory_value.to_path_buf(), group.clone(), group));
        }

        if group.is_file() {
            return file_git_dirs(directory_value, &group);
        }
    }

    None
}

/// The roots and the ignore verdicts.
struct Watch {
    threads: PathBuf,
    gits: Vec<PathBuf>,
    works: Vec<PathBuf>,
    /// Work roots without git: 0 ignore rules.
    plain: HashSet<PathBuf>,
    /// work tree → (path relative to it → ignored).
    ignores: HashMap<PathBuf, HashMap<PathBuf, bool>>,
}

/// The class of 1 path.
#[derive(Debug, PartialEq)]
enum Class {
    Topic(Topic),
    /// A work tree path: (work tree, relative path); the ignore check decides.
    Work(PathBuf, PathBuf),
    Drop,
}

/// Verdicts per entry: the ignore checks of a batch.
const MAX_VERDICTS: usize = 50_000;

impl Watch {
    fn set(&mut self, gits: Vec<PathBuf>, works: Vec<PathBuf>) {
        self.ignores.retain(|width, _| works.contains(width));
        self.plain = works
            .iter()
            .filter(|width_value| !width_value.join(".git").exists())
            .cloned()
            .collect();
        self.gits = gits;
        self.works = works;
    }

    fn classify(&self, path: &Path) -> Class {
        classify_path(self, path)
    }

    /// The topics of PATHS. CHECK: (work tree, relative paths) → the ignored ones.
    fn topics(
        &mut self,
        paths: &[PathBuf],
        check: &mut dyn FnMut(&Path, &[PathBuf]) -> HashSet<PathBuf>,
    ) -> Vec<Topic> {
        collect_topics(self, paths, check)
    }

    /// REL in work tree W: Some(true) ignored (itself or a dir above), Some(false) not, None unknown.
    fn verdict(&self, path: &Path, rel: &Path) -> Option<bool> {
        ignore_verdict(self, path, rel)
    }
}

/// REL's dirs from the top, then REL: `a/b/c` → `a`, `a/b`, `a/b/c`.
fn ancestors(rel: &Path) -> Vec<PathBuf> {
    let mut path: Vec<PathBuf> = rel
        .ancestors()
        .filter(|agent| !agent.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .collect();

    path.reverse();
    path
}

/// The part of a git dir path REL that changes what a read shows: HEAD, index, refs, packed-refs
/// (also of a linked worktree, `worktrees/<name>/`). Objects, logs, locks: None.
fn git_part(rel: &Path) -> Option<Git> {
    let text = rel.to_string_lossy();

    if text.ends_with(".lock") || text.starts_with("objects/") || text.starts_with("logs/") {
        return None;
    }

    if let Some(result) = text.strip_prefix("worktrees/") {
        let (name, file) = result.split_once('/')?;

        return matches!(file, "HEAD" | "index" | "MERGE_HEAD").then(|| Git::Worktree(name.to_string()));
    }

    if let Some(bytes) = text.strip_prefix("refs/heads/") {
        return Some(Git::Branch(bytes.to_string()));
    }

    if let Some(result_value) = text.strip_prefix("refs/remotes/") {
        // <remote>/<branch>; <remote>/HEAD: the default branch moved, every checkout; <remote>
        // alone: its dir (its files come as events of their own)
        let bytes_2 = result_value.split_once('/').map_or("", |(_, bytes_value)| bytes_value);

        return match bytes_2 {
            "" => None,
            "HEAD" => Some(Git::All),
            bytes_3 => Some(Git::Remote(bytes_3.to_string())),
        };
    }

    match text.as_ref() {
        "HEAD" | "index" | "MERGE_HEAD" => Some(Git::Worktree(String::new())),
        // FETCH_HEAD: every fetch writes it; new remote refs come as events of their own
        "packed-refs" | "info/exclude" => Some(Git::All),
        // the dirs of refs: their files come as events of their own
        "refs" | "refs/heads" | "refs/remotes" | "refs/tags" => None,
        _ if text.starts_with("refs/tags/") => None,
        _ if text.starts_with("refs/") => Some(Git::All),
        _ => None,
    }
}

/// `git check-ignore` of RELS in work tree W: the ignored ones. 0 answer (git failed): 0 ignored.
fn check_ignore(path: &Path, rels: &[PathBuf]) -> HashSet<PathBuf> {
    use std::io::Write;
    use std::process::{Command, Stdio};

    if rels.is_empty() {
        return HashSet::new();
    }

    let Ok(mut child) = Command::new("git")
        .arg("-C")
        .arg(path)
        .args(["check-ignore", "-z", "--stdin"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    else {
        return HashSet::new();
    };

    if let Some(mut i) = child.stdin.take() {
        let mut buf = Vec::new();

        for result in rels {
            buf.extend_from_slice(result.as_os_str().as_encoded_bytes());
            buf.push(0);
        }

        let _ = i.write_all(&buf);
    }

    let Ok(output) = child.wait_with_output() else {
        return HashSet::new();
    };

    output
        .stdout
        .split(|&bytes| bytes == 0)
        .filter(|text| !text.is_empty())
        .map(|path_value| PathBuf::from(String::from_utf8_lossy(path_value).into_owned()))
        .collect()
}

#[cfg(test)]
#[path = "watch_tests.rs"]
mod tests;

fn classify_path(widget_state: &Watch, path: &Path) -> Class {
    if let Ok(rel) = path.strip_prefix(&widget_state.threads) {
        let mut character = rel.components();

        let Some(id) = character
            .next()
            .and_then(|character_value| character_value.as_os_str().to_str())
        else {
            return Class::Drop;
        };

        let part = match character
            .next()
            .and_then(|character_2| character_2.as_os_str().to_str())
        {
            Some("thread.json") => Part::Meta,
            Some("events.jsonl") => Part::Log,
            Some("comments.json") => Part::Comments,
            _ => Part::Other,
        };

        return Class::Topic(Topic::Thread(id.to_string(), part));
    }

    // a git dir (a main checkout's `.git` lies in its work tree: git first)
    if let Some(group_2) = widget_state
        .gits
        .iter()
        .filter(|group| path.starts_with(group))
        .max_by_key(|group_value| group_value.as_os_str().len())
    {
        return git_part(path.strip_prefix(group_2).unwrap_or(path))
            .map_or(Class::Drop, |key| Class::Topic(Topic::Git(group_2.clone(), key)));
    }

    let Some(width_2) = widget_state
        .works
        .iter()
        .filter(|width| path.starts_with(width))
        .max_by_key(|width_value| width_value.as_os_str().len())
    else {
        return Class::Drop;
    };

    let rel = path.strip_prefix(width_2).unwrap_or(path);

    // the root itself, or a dir without git (0 ignore rules)
    if rel.as_os_str().is_empty() || widget_state.plain.contains(width_2) {
        return Class::Topic(Topic::Work(width_2.clone()));
    }

    if rel
        .components()
        .next()
        .is_some_and(|character_3| character_3.as_os_str() == ".git")
    {
        return Class::Drop;
    }

    Class::Work(width_2.clone(), rel.to_path_buf())
}

fn collect_topics(
    widget_state: &mut Watch,
    paths: &[PathBuf],
    check: &mut dyn FnMut(&Path, &[PathBuf]) -> HashSet<PathBuf>,
) -> Vec<Topic> {
    let mut out: Vec<Topic> = Vec::new();

    let mut pending: HashMap<PathBuf, Vec<PathBuf>> = HashMap::new();

    for path in paths {
        match widget_state.classify(path) {
            Class::Topic(thread_value) => add_topic(thread_value, &mut out),
            Class::Drop => {}
            Class::Work(width, rel) => {
                classify_work(widget_state, width, rel, &mut pending, &mut out);
            }
        }
    }

    for (width_value, rels) in pending {
        // each unknown ancestor dir, then the path itself
        let ask = unknown_ancestors(widget_state, &width_value, &rels);

        let ignored = check(&width_value, &ask);
        let message_value = widget_state.ignores.entry(width_value.clone()).or_default();

        if message_value.len() + ask.len() > MAX_VERDICTS {
            message_value.clear();
        }

        for agent_value in ask {
            let i = ignored.contains(&agent_value);

            message_value.insert(agent_value, i);
        }

        if rels
            .iter()
            .any(|result| widget_state.verdict(&width_value, result) == Some(false))
        {
            add_topic(Topic::Work(width_value), &mut out);
        }
    }

    out
}

fn ignore_verdict(widget_state: &Watch, path: &Path, rel: &Path) -> Option<bool> {
    let message = widget_state.ignores.get(path)?;

    for agent in ancestors(rel) {
        match message.get(&agent) {
            Some(true) => return Some(true),
            Some(false) => {}
            None => return None,
        }
    }

    Some(false)
}

#[path = "watch_runtime.rs"]
mod runtime;
use runtime::run;

fn add_plain_root(directory: &Path, shown: &[PathBuf], works: &mut HashSet<PathBuf>) {
    if shown.iter().any(|path| path == directory) && directory.is_dir() {
        works.insert(canon(directory));
    }
}

fn file_git_dirs(directory_value: &Path, group: &Path) -> Option<(PathBuf, PathBuf, PathBuf)> {
    let text = std::fs::read_to_string(group).ok()?;
    let own = PathBuf::from(text.trim().strip_prefix("gitdir:")?.trim());

    let own = if own.is_relative() {
        directory_value.join(own)
    } else {
        own
    };

    let common = std::fs::read_to_string(own.join("commondir")).ok().map(|path| {
        let path_value = PathBuf::from(path.trim());

        if path_value.is_relative() {
            own.join(path_value)
        } else {
            path_value
        }
    });

    Some((
        directory_value.to_path_buf(),
        common.unwrap_or_else(|| own.clone()),
        own,
    ))
}

fn classify_work(
    widget_state: &mut Watch,
    width: PathBuf,
    rel: PathBuf,
    pending: &mut HashMap<PathBuf, Vec<PathBuf>>,
    out: &mut Vec<Topic>,
) {
    // a changed ignore file: every verdict of this work tree again
    if rel.file_name().is_some_and(|count| count == ".gitignore") {
        widget_state.ignores.remove(&width);
    }

    match widget_state.verdict(&width, &rel) {
        Some(true) => {}
        Some(false) => add_topic(Topic::Work(width), out),
        None => pending.entry(width).or_default().push(rel),
    }
}

fn unknown_ancestors(widget_state: &Watch, width_value: &Path, rels: &[PathBuf]) -> Vec<PathBuf> {
    let mut ask = Vec::new();

    for agent in rels.iter().flat_map(|rel| ancestors(rel)) {
        if !ask.contains(&agent)
            && !widget_state
                .ignores
                .get(width_value)
                .is_some_and(|message| message.contains_key(&agent))
        {
            ask.push(agent);
        }
    }

    ask
}

fn add_topic(thread: Topic, out: &mut Vec<Topic>) {
    if !out.contains(&thread) {
        out.push(thread);
    }
}
