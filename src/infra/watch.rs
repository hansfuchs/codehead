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
    if let Ok(sender) = TX.lock()
        && let Some(tx) = sender.as_ref()
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
        let Ok(mut sender) = TX.lock() else { return };

        if sender.is_some() {
            return;
        }

        *sender = Some(tx.clone());
    }

    let os_tx = tx.clone();

    let handler = move |result: notify::Result<notify::Event>| {
        let message = match result {
            Ok(event) if event.need_rescan() => Msg::Rescan,
            Ok(event) => Msg::Paths(event.paths),
            Err(_) => Msg::Rescan,
        };

        let _ = os_tx.send(message);
    };

    let watcher = match notify::recommended_watcher(handler) {
        Ok(watcher) => watcher,
        Err(event) => {
            crate::engine::trace(&format!("watch: {event}"));
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
    for repo in threads
        .iter()
        .filter(|thread| thread.state == "archived")
        .filter_map(crate::domain::threads::repo_of_thread_store)
    {
        if let Some((_, common, _)) = git_dirs(&repo) {
            gits.insert(canon(&common));
        }
    }

    let dirs = threads
        .iter()
        .filter(|thread| thread.state != "archived" && thread.state != "deleted")
        .flat_map(crate::engine::store::Thread::all_checkouts)
        .map(|checkout| PathBuf::from(checkout.dir));

    for dir in dirs.chain(shown.iter().cloned()) {
        let Some((top, common, head)) = git_dirs(&dir) else {
            // a view's dir without git (a transcript dir): watched as it is, 0 ignore rules
            add_plain_root(&dir, shown, &mut works);

            continue;
        };

        gits.insert(canon(&common));

        // the default branch: the owner's, 0 landed state → 0 work tree watch (a view: yes)
        let branch = std::fs::read_to_string(head.join("HEAD")).unwrap_or_default();
        let on_default = matches!(branch.trim().strip_prefix("ref: refs/heads/"), Some("main" | "master"));

        if !on_default || shown.contains(&dir) {
            works.insert(canon(&top));
        }
    }

    (gits.into_iter().collect(), works.into_iter().collect())
}

/// (top dir, common git dir, own git dir) of the checkout that holds DIR. In-process, 0 git call.
pub(crate) fn git_dirs(dir: &Path) -> Option<(PathBuf, PathBuf, PathBuf)> {
    for ancestor in dir.ancestors().filter(|candidate| !candidate.as_os_str().is_empty()) {
        let marker = ancestor.join(".git");

        if marker.is_dir() {
            return Some((ancestor.to_path_buf(), marker.clone(), marker));
        }

        if marker.is_file() {
            return file_git_dirs(ancestor, &marker);
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
        self.ignores.retain(|work, _| works.contains(work));
        self.plain = works
            .iter()
            .filter(|work| !work.join(".git").exists())
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

    /// REL in work tree WORK: Some(true) ignored (itself or a dir above), Some(false) not, None unknown.
    fn verdict(&self, work: &Path, rel: &Path) -> Option<bool> {
        ignore_verdict(self, work, rel)
    }
}

/// REL's dirs from the top, then REL: `a/b/c` → `a`, `a/b`, `a/b/c`.
fn ancestors(rel: &Path) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = rel
        .ancestors()
        .filter(|ancestor| !ancestor.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .collect();

    dirs.reverse();
    dirs
}

/// The part of a git dir path REL that changes what a read shows: HEAD, index, refs, packed-refs
/// (also of a linked worktree, `worktrees/<name>/`). Objects, logs, locks: None.
fn git_part(rel: &Path) -> Option<Git> {
    let text = rel.to_string_lossy();

    if text.ends_with(".lock") || text.starts_with("objects/") || text.starts_with("logs/") {
        return None;
    }

    if let Some(rest) = text.strip_prefix("worktrees/") {
        let (name, file) = rest.split_once('/')?;

        return matches!(file, "HEAD" | "index" | "MERGE_HEAD").then(|| Git::Worktree(name.to_string()));
    }

    if let Some(branch) = text.strip_prefix("refs/heads/") {
        return Some(Git::Branch(branch.to_string()));
    }

    if let Some(remote_rest) = text.strip_prefix("refs/remotes/") {
        // <remote>/<branch>; <remote>/HEAD: the default branch moved, every checkout; <remote>
        // alone: its dir (its files come as events of their own)
        let branch = remote_rest.split_once('/').map_or("", |(_, branch)| branch);

        return match branch {
            "" => None,
            "HEAD" => Some(Git::All),
            branch => Some(Git::Remote(branch.to_string())),
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

/// `git check-ignore` of RELS in work tree WORK: the ignored ones. 0 answer (git failed): 0 ignored.
fn check_ignore(work: &Path, rels: &[PathBuf]) -> HashSet<PathBuf> {
    use std::io::Write;
    use std::process::{Command, Stdio};

    if rels.is_empty() {
        return HashSet::new();
    }

    let Ok(mut child) = Command::new("git")
        .arg("-C")
        .arg(work)
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

        for rel in rels {
            buf.extend_from_slice(rel.as_os_str().as_encoded_bytes());
            buf.push(0);
        }

        let _ = i.write_all(&buf);
    }

    let Ok(output) = child.wait_with_output() else {
        return HashSet::new();
    };

    output
        .stdout
        .split(|&byte| byte == 0)
        .filter(|part| !part.is_empty())
        .map(|part| PathBuf::from(String::from_utf8_lossy(part).into_owned()))
        .collect()
}

#[cfg(test)]
#[path = "watch_tests.rs"]
mod tests;

fn classify_path(watch: &Watch, path: &Path) -> Class {
    if let Ok(rel) = path.strip_prefix(&watch.threads) {
        let mut parts = rel.components();

        let Some(id) = parts.next().and_then(|part| part.as_os_str().to_str()) else {
            return Class::Drop;
        };

        let part = match parts.next().and_then(|part| part.as_os_str().to_str()) {
            Some("thread.json") => Part::Meta,
            Some("events.jsonl") => Part::Log,
            Some("comments.json") => Part::Comments,
            _ => Part::Other,
        };

        return Class::Topic(Topic::Thread(id.to_string(), part));
    }

    // a git dir (a main checkout's `.git` lies in its work tree: git first)
    if let Some(common) = watch
        .gits
        .iter()
        .filter(|common| path.starts_with(common))
        .max_by_key(|common| common.as_os_str().len())
    {
        return git_part(path.strip_prefix(common).unwrap_or(path))
            .map_or(Class::Drop, |changed| Class::Topic(Topic::Git(common.clone(), changed)));
    }

    let Some(work) = watch
        .works
        .iter()
        .filter(|work| path.starts_with(work))
        .max_by_key(|work| work.as_os_str().len())
    else {
        return Class::Drop;
    };

    let rel = path.strip_prefix(work).unwrap_or(path);

    // the root itself, or a dir without git (0 ignore rules)
    if rel.as_os_str().is_empty() || watch.plain.contains(work) {
        return Class::Topic(Topic::Work(work.clone()));
    }

    if rel.components().next().is_some_and(|first| first.as_os_str() == ".git") {
        return Class::Drop;
    }

    Class::Work(work.clone(), rel.to_path_buf())
}

fn collect_topics(
    watch: &mut Watch,
    paths: &[PathBuf],
    check: &mut dyn FnMut(&Path, &[PathBuf]) -> HashSet<PathBuf>,
) -> Vec<Topic> {
    let mut out: Vec<Topic> = Vec::new();

    let mut pending: HashMap<PathBuf, Vec<PathBuf>> = HashMap::new();

    for path in paths {
        match watch.classify(path) {
            Class::Topic(topic) => add_topic(topic, &mut out),
            Class::Drop => {}
            Class::Work(work, rel) => {
                classify_work(watch, work, rel, &mut pending, &mut out);
            }
        }
    }

    for (work, rels) in pending {
        // each unknown ancestor dir, then the path itself
        let ask = unknown_ancestors(watch, &work, &rels);

        let ignored = check(&work, &ask);
        let verdicts = watch.ignores.entry(work.clone()).or_default();

        if verdicts.len() + ask.len() > MAX_VERDICTS {
            verdicts.clear();
        }

        // a nested repo or worktree (a subagent's): ignored like a gitignored dir
        for dir in ask {
            let skip = ignored.contains(&dir) || work.join(&dir).join(".git").exists();

            verdicts.insert(dir, skip);
        }

        if rels.iter().any(|rel| watch.verdict(&work, rel) == Some(false)) {
            add_topic(Topic::Work(work), &mut out);
        }
    }

    out
}

fn ignore_verdict(watch: &Watch, work: &Path, rel: &Path) -> Option<bool> {
    let verdicts = watch.ignores.get(work)?;

    for dir in ancestors(rel) {
        match verdicts.get(&dir) {
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

#[path = "watch_settle.rs"]
mod settle;

fn add_plain_root(dir: &Path, shown: &[PathBuf], works: &mut HashSet<PathBuf>) {
    if shown.iter().any(|path| path == dir) && dir.is_dir() {
        works.insert(canon(dir));
    }
}

fn file_git_dirs(top: &Path, marker: &Path) -> Option<(PathBuf, PathBuf, PathBuf)> {
    let text = std::fs::read_to_string(marker).ok()?;
    let own = PathBuf::from(text.trim().strip_prefix("gitdir:")?.trim());

    let own = if own.is_relative() { top.join(own) } else { own };

    let common = std::fs::read_to_string(own.join("commondir")).ok().map(|content| {
        let dir = PathBuf::from(content.trim());

        if dir.is_relative() { own.join(dir) } else { dir }
    });

    Some((top.to_path_buf(), common.unwrap_or_else(|| own.clone()), own))
}

fn classify_work(
    watch: &mut Watch,
    work: PathBuf,
    rel: PathBuf,
    pending: &mut HashMap<PathBuf, Vec<PathBuf>>,
    out: &mut Vec<Topic>,
) {
    // a changed ignore file: every verdict of this work tree again
    if rel.file_name().is_some_and(|name| name == ".gitignore") {
        watch.ignores.remove(&work);
    }

    // a temp file of an atomic write: its rename brings the real path
    if crate::infra::stray_paths::transient(&rel.to_string_lossy()) {
        return;
    }

    // a nested repo or worktree came (its `.git`): its dir and all below it are not this work
    // tree's (a verdict of the dir from before its `.git`: replaced)
    if let Some(parent) = rel
        .parent()
        .filter(|_| rel.file_name().is_some_and(|name| name == ".git"))
    {
        watch
            .ignores
            .entry(work)
            .or_default()
            .insert(parent.to_path_buf(), true);

        return;
    }

    match watch.verdict(&work, &rel) {
        Some(true) => {}
        Some(false) => add_topic(Topic::Work(work), out),
        None => pending.entry(work).or_default().push(rel),
    }
}

fn unknown_ancestors(watch: &Watch, work: &Path, rels: &[PathBuf]) -> Vec<PathBuf> {
    let mut ask = Vec::new();

    for dir in rels.iter().flat_map(|rel| ancestors(rel)) {
        if !ask.contains(&dir)
            && !watch
                .ignores
                .get(work)
                .is_some_and(|verdicts| verdicts.contains_key(&dir))
        {
            ask.push(dir);
        }
    }

    ask
}

fn add_topic(topic: Topic, out: &mut Vec<Topic>) {
    if !out.contains(&topic) {
        out.push(topic);
    }
}
