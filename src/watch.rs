//! File watcher: 1 OS stream (FSEvents on macOS, inotify on Linux) over the minimum set of roots,
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

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::mpsc::{Receiver, Sender, channel};

use crate::bus::{self, Git, Part, Topic};
use crate::engine::store;

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
pub struct Shown(u64);

impl Drop for Shown {
    fn drop(&mut self) {
        send(Msg::Show(self.0, None));
    }
}

/// Watch work tree DIR while the guard lives (the changes view shows it): its edits publish `Work`.
pub fn show(dir: &Path) -> Shown {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let token = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    send(Msg::Show(token, Some(dir.to_path_buf())));
    Shown(token)
}

fn send(m: Msg) {
    if let Ok(g) = TX.lock()
        && let Some(tx) = g.as_ref()
    {
        let _ = tx.send(m);
    }
}

/// Start the watcher (once per process; 0 in tests: `start_at`).
pub fn start() {
    if cfg!(test) {
        return;
    }
    start_at(store::threads_dir());
}

/// Start the watcher with THREADS as the threads dir (tests: a temp dir).
pub fn start_at(threads: PathBuf) {
    let (tx, rx) = channel();
    {
        let Ok(mut g) = TX.lock() else { return };
        if g.is_some() {
            return;
        }
        *g = Some(tx.clone());
    }
    let os_tx = tx.clone();
    let handler = move |r: notify::Result<notify::Event>| {
        let m = match r {
            Ok(e) if e.need_rescan() => Msg::Rescan,
            Ok(e) => Msg::Paths(e.paths),
            Err(_) => Msg::Rescan,
        };
        let _ = os_tx.send(m);
    };
    let watcher = match notify::recommended_watcher(handler) {
        Ok(w) => w,
        Err(e) => {
            crate::engine::trace(&format!("watch: {e}"));
            return;
        }
    };
    std::thread::Builder::new().name("watch".into()).spawn(move || run(watcher, threads, rx)).ok();
}

/// The watcher thread: roots, classify, publish.
fn run(mut watcher: notify::RecommendedWatcher, threads: PathBuf, rx: Receiver<Msg>) {
    use notify::Watcher;
    let threads = canon(&threads);
    let _ = std::fs::create_dir_all(&threads);
    let mut w = Watch { threads: threads.clone(), gits: Vec::new(), works: Vec::new(), plain: HashSet::new(), ignores: HashMap::new() };
    let mut watched: HashSet<PathBuf> = HashSet::new();
    let mut shown: HashMap<u64, PathBuf> = HashMap::new();
    let mut all: HashMap<String, store::Thread> = store::all_in(&threads).into_iter().map(|t| (t.id.clone(), t)).collect();
    let mut roots_due = true;
    loop {
        if roots_due {
            roots_due = false;
            let list: Vec<store::Thread> = all.values().cloned().collect();
            let views: Vec<PathBuf> = shown.values().cloned().collect();
            let (gits, works) = roots(&list, &views);
            w.set(gits, works);
            let want: HashSet<PathBuf> = std::iter::once(threads.clone()).chain(w.gits.iter().cloned()).chain(w.works.iter().cloned()).collect();
            let mut ops: Vec<notify::PathOp> = watched.difference(&want).map(|p| notify::PathOp::unwatch(p.clone())).collect();
            let added: Vec<PathBuf> = want.difference(&watched).cloned().collect();
            ops.extend(added.iter().map(|p| notify::PathOp::watch_recursive(p.clone())));
            if !ops.is_empty() {
                if let Err(e) = watcher.update_paths(ops) {
                    crate::engine::trace(&format!("watch: {e}"));
                }
                // new roots: what happened before their watch is unknown (the 1st set: the
                // readers read everything at their start)
                if !watched.is_empty() {
                    for p in &added {
                        bus::publish(Topic::Rescan(p.clone()));
                    }
                }
                watched = want;
            }
        }
        let Ok(first) = rx.recv() else { return };
        let mut paths = Vec::new();
        for m in std::iter::once(first).chain(std::iter::from_fn(|| rx.try_recv().ok())) {
            match m {
                Msg::Paths(p) => paths.extend(p),
                Msg::Rescan => {
                    all = store::all_in(&threads).into_iter().map(|t| (t.id.clone(), t)).collect();
                    roots_due = true;
                    for p in &watched {
                        bus::publish(Topic::Rescan(p.clone()));
                    }
                }
                Msg::Show(token, d) => {
                    let old = match d {
                        Some(d) => shown.insert(token, d.clone()).filter(|o| *o == d).is_none(),
                        None => shown.remove(&token).is_some(),
                    };
                    roots_due |= old;
                }
            }
        }
        for t in w.topics(&paths, &mut check_ignore) {
            // a thread's checkouts or a branch switch: other roots
            if let Topic::Thread(id, Part::Meta) = &t {
                match store::load_in(&threads, id) {
                    Some(n) => all.insert(id.clone(), n),
                    None => all.remove(id),
                };
                roots_due = true;
            }
            roots_due |= matches!(t, Topic::Git(_, Git::Worktree(_)));
            bus::publish(t);
        }
    }
}

pub fn canon(p: &Path) -> PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}

/// The roots of THREADS (open ones): (git dirs, work trees). SHOWN: more work trees (views).
fn roots(threads: &[store::Thread], shown: &[PathBuf]) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let mut gits = HashSet::new();
    let mut works = HashSet::new();
    // archived threads: their kept branch → their repo's git dir only
    for r in threads.iter().filter(|t| t.state == "archived").filter_map(crate::threads::repo_of_thread_store) {
        if let Some((_, common, _)) = git_dirs(&r) {
            gits.insert(canon(&common));
        }
    }
    let dirs = threads.iter().filter(|t| t.state != "archived" && t.state != "deleted").flat_map(|t| t.all_checkouts()).map(|c| PathBuf::from(c.dir));
    for d in dirs.chain(shown.iter().cloned()) {
        let Some((top, common, head)) = git_dirs(&d) else {
            // a view's dir without git (a transcript dir): watched as it is, 0 ignore rules
            if shown.contains(&d) && d.is_dir() {
                works.insert(canon(&d));
            }
            continue;
        };
        gits.insert(canon(&common));
        // the default branch: the owner's, 0 landed state → 0 work tree watch (a view: yes)
        let branch = std::fs::read_to_string(head.join("HEAD")).unwrap_or_default();
        let on_default = matches!(branch.trim().strip_prefix("ref: refs/heads/"), Some("main" | "master"));
        if !on_default || shown.contains(&d) {
            works.insert(canon(&top));
        }
    }
    (gits.into_iter().collect(), works.into_iter().collect())
}

/// (top dir, common git dir, own git dir) of the checkout that holds DIR. In-process, 0 git call.
pub fn git_dirs(dir: &Path) -> Option<(PathBuf, PathBuf, PathBuf)> {
    for d in dir.ancestors().filter(|d| !d.as_os_str().is_empty()) {
        let g = d.join(".git");
        if g.is_dir() {
            return Some((d.to_path_buf(), g.clone(), g));
        }
        if g.is_file() {
            let text = std::fs::read_to_string(&g).ok()?;
            let own = PathBuf::from(text.trim().strip_prefix("gitdir:")?.trim());
            let own = if own.is_relative() { d.join(own) } else { own };
            let common = std::fs::read_to_string(own.join("commondir")).ok().map(|c| {
                let c = PathBuf::from(c.trim());
                if c.is_relative() { own.join(c) } else { c }
            });
            return Some((d.to_path_buf(), common.unwrap_or_else(|| own.clone()), own));
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
        self.ignores.retain(|w, _| works.contains(w));
        self.plain = works.iter().filter(|w| !w.join(".git").exists()).cloned().collect();
        self.gits = gits;
        self.works = works;
    }

    fn classify(&self, p: &Path) -> Class {
        if let Ok(rel) = p.strip_prefix(&self.threads) {
            let mut c = rel.components();
            let Some(id) = c.next().and_then(|c| c.as_os_str().to_str()) else { return Class::Drop };
            let part = match c.next().and_then(|c| c.as_os_str().to_str()) {
                Some("thread.json") => Part::Meta,
                Some("events.jsonl") => Part::Log,
                Some("comments.json") => Part::Comments,
                _ => Part::Other,
            };
            return Class::Topic(Topic::Thread(id.to_string(), part));
        }
        // a git dir (a main checkout's `.git` lies in its work tree: git first)
        if let Some(g) = self.gits.iter().filter(|g| p.starts_with(g)).max_by_key(|g| g.as_os_str().len()) {
            return git_part(p.strip_prefix(g).unwrap_or(p)).map_or(Class::Drop, |k| Class::Topic(Topic::Git(g.clone(), k)));
        }
        let Some(w) = self.works.iter().filter(|w| p.starts_with(w)).max_by_key(|w| w.as_os_str().len()) else { return Class::Drop };
        let rel = p.strip_prefix(w).unwrap_or(p);
        // the root itself, or a dir without git (0 ignore rules)
        if rel.as_os_str().is_empty() || self.plain.contains(w) {
            return Class::Topic(Topic::Work(w.clone()));
        }
        if rel.components().next().is_some_and(|c| c.as_os_str() == ".git") {
            return Class::Drop;
        }
        Class::Work(w.clone(), rel.to_path_buf())
    }

    /// The topics of PATHS. CHECK: (work tree, relative paths) → the ignored ones.
    fn topics(&mut self, paths: &[PathBuf], check: &mut dyn FnMut(&Path, &[PathBuf]) -> HashSet<PathBuf>) -> Vec<Topic> {
        let mut out: Vec<Topic> = Vec::new();
        let add = |t: Topic, out: &mut Vec<Topic>| {
            if !out.contains(&t) {
                out.push(t);
            }
        };
        let mut pending: HashMap<PathBuf, Vec<PathBuf>> = HashMap::new();
        for p in paths {
            match self.classify(p) {
                Class::Topic(t) => add(t, &mut out),
                Class::Drop => {}
                Class::Work(w, rel) => {
                    // a changed ignore file: every verdict of this work tree again
                    if rel.file_name().is_some_and(|n| n == ".gitignore") {
                        self.ignores.remove(&w);
                    }
                    match self.verdict(&w, &rel) {
                        Some(true) => {}
                        Some(false) => add(Topic::Work(w), &mut out),
                        None => pending.entry(w).or_default().push(rel),
                    }
                }
            }
        }
        for (w, rels) in pending {
            // each unknown ancestor dir, then the path itself
            let mut ask: Vec<PathBuf> = Vec::new();
            for rel in &rels {
                for a in ancestors(rel) {
                    if !ask.contains(&a) && !self.ignores.get(&w).is_some_and(|m| m.contains_key(&a)) {
                        ask.push(a);
                    }
                }
            }
            let ignored = check(&w, &ask);
            let m = self.ignores.entry(w.clone()).or_default();
            if m.len() + ask.len() > MAX_VERDICTS {
                m.clear();
            }
            for a in ask {
                let i = ignored.contains(&a);
                m.insert(a, i);
            }
            if rels.iter().any(|r| self.verdict(&w, r) == Some(false)) {
                add(Topic::Work(w), &mut out);
            }
        }
        out
    }

    /// REL in work tree W: Some(true) ignored (itself or a dir above), Some(false) not, None unknown.
    fn verdict(&self, w: &Path, rel: &Path) -> Option<bool> {
        let m = self.ignores.get(w)?;
        for a in ancestors(rel) {
            match m.get(&a) {
                Some(true) => return Some(true),
                Some(false) => {}
                None => return None,
            }
        }
        Some(false)
    }
}

/// REL's dirs from the top, then REL: `a/b/c` → `a`, `a/b`, `a/b/c`.
fn ancestors(rel: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = rel.ancestors().filter(|a| !a.as_os_str().is_empty()).map(Path::to_path_buf).collect();
    v.reverse();
    v
}

/// The part of a git dir path REL that changes what a read shows: HEAD, index, refs, packed-refs
/// (also of a linked worktree, `worktrees/<name>/`). Objects, logs, locks: None.
fn git_part(rel: &Path) -> Option<Git> {
    let s = rel.to_string_lossy();
    if s.ends_with(".lock") || s.starts_with("objects/") || s.starts_with("logs/") {
        return None;
    }
    if let Some(r) = s.strip_prefix("worktrees/") {
        let (name, f) = r.split_once('/')?;
        return matches!(f, "HEAD" | "index" | "MERGE_HEAD").then(|| Git::Worktree(name.to_string()));
    }
    if let Some(b) = s.strip_prefix("refs/heads/") {
        return Some(Git::Branch(b.to_string()));
    }
    if let Some(r) = s.strip_prefix("refs/remotes/") {
        // <remote>/<branch>; <remote>/HEAD: the default branch moved, every checkout; <remote>
        // alone: its dir (its files come as events of their own)
        let b = r.split_once('/').map_or("", |(_, b)| b);
        return match b {
            "" => None,
            "HEAD" => Some(Git::All),
            b => Some(Git::Remote(b.to_string())),
        };
    }
    match s.as_ref() {
        "HEAD" | "index" | "MERGE_HEAD" => Some(Git::Worktree(String::new())),
        // FETCH_HEAD: every fetch writes it; new remote refs come as events of their own
        "packed-refs" | "info/exclude" => Some(Git::All),
        // the dirs of refs: their files come as events of their own
        "refs" | "refs/heads" | "refs/remotes" | "refs/tags" => None,
        _ if s.starts_with("refs/tags/") => None,
        _ if s.starts_with("refs/") => Some(Git::All),
        _ => None,
    }
}

/// `git check-ignore` of RELS in work tree W: the ignored ones. 0 answer (git failed): 0 ignored.
fn check_ignore(w: &Path, rels: &[PathBuf]) -> HashSet<PathBuf> {
    use std::io::Write;
    use std::process::{Command, Stdio};
    if rels.is_empty() {
        return HashSet::new();
    }
    let Ok(mut child) = Command::new("git").arg("-C").arg(w).args(["check-ignore", "-z", "--stdin"]).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn() else { return HashSet::new() };
    if let Some(mut i) = child.stdin.take() {
        let mut buf = Vec::new();
        for r in rels {
            buf.extend_from_slice(r.as_os_str().as_encoded_bytes());
            buf.push(0);
        }
        let _ = i.write_all(&buf);
    }
    let Ok(o) = child.wait_with_output() else { return HashSet::new() };
    o.stdout.split(|&b| b == 0).filter(|s| !s.is_empty()).map(|s| PathBuf::from(String::from_utf8_lossy(s).into_owned())).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn w() -> Watch {
        Watch { threads: "/d/threads".into(), gits: vec!["/r/.git".into(), "/r2/.git".into()], works: vec!["/wt".into(), "/r".into()], plain: HashSet::new(), ignores: HashMap::new() }
    }

    /// Thread files by part, git metadata only, work tree paths to the ignore check.
    #[test]
    fn classify_maps_paths_to_topics() {
        let w = w();
        let t = |id: &str, p| Class::Topic(Topic::Thread(id.into(), p));
        assert_eq!(w.classify(Path::new("/d/threads/ab/thread.json")), t("ab", Part::Meta));
        assert_eq!(w.classify(Path::new("/d/threads/ab/events.jsonl")), t("ab", Part::Log));
        assert_eq!(w.classify(Path::new("/d/threads/ab/thread.tmp-12")), t("ab", Part::Other));

        let git = |k| Class::Topic(Topic::Git("/r/.git".into(), k));
        assert_eq!(w.classify(Path::new("/r/.git/refs/heads/task/x")), git(Git::Branch("task/x".into())));
        assert_eq!(w.classify(Path::new("/r/.git/refs/remotes/origin/task/x")), git(Git::Remote("task/x".into())));
        assert_eq!(w.classify(Path::new("/r/.git/refs/remotes/origin/HEAD")), git(Git::All));
        assert_eq!(w.classify(Path::new("/r/.git/worktrees/wt/HEAD")), git(Git::Worktree("wt".into())));
        assert_eq!(w.classify(Path::new("/r/.git/worktrees/wt/ORIG_HEAD")), Class::Drop);
        assert_eq!(w.classify(Path::new("/r/.git/index")), git(Git::Worktree(String::new())));
        assert_eq!(w.classify(Path::new("/r/.git/packed-refs")), git(Git::All));
        assert_eq!(w.classify(Path::new("/r/.git/objects/ab/cdef")), Class::Drop);
        assert_eq!(w.classify(Path::new("/r/.git/index.lock")), Class::Drop);
        assert_eq!(w.classify(Path::new("/r/.git/logs/HEAD")), Class::Drop);

        assert_eq!(w.classify(Path::new("/wt/src/a.rs")), Class::Work("/wt".into(), "src/a.rs".into()));
        assert_eq!(w.classify(Path::new("/wt/.git")), Class::Drop);
        assert_eq!(w.classify(Path::new("/elsewhere/x")), Class::Drop);
    }

    /// 1 check per new dir: an install storm in `node_modules` costs 1 check, then 0; a tracked
    /// file edit publishes 1 `Work`.
    #[test]
    fn ignored_dirs_are_checked_once() {
        let mut w = w();
        let calls = std::cell::Cell::new(0);
        let mut check = |_: &Path, rels: &[PathBuf]| {
            calls.set(calls.get() + 1);
            rels.iter().filter(|r| r.starts_with("node_modules")).cloned().collect::<HashSet<_>>()
        };
        let storm: Vec<PathBuf> = (0..1000).map(|i| PathBuf::from(format!("/wt/node_modules/p{i}/index.js"))).collect();
        assert!(w.topics(&storm, &mut check).is_empty());
        assert!(w.topics(&storm, &mut check).is_empty());
        assert!(w.topics(&[PathBuf::from("/wt/src/a.rs")], &mut check) == vec![Topic::Work("/wt".into())]);
        assert!(w.topics(&[PathBuf::from("/wt/src/a.rs")], &mut check) == vec![Topic::Work("/wt".into())]);
        assert_eq!(calls.get(), 2, "node_modules once, src/a.rs once");

        // a .gitignore edit: verdicts again
        w.topics(&[PathBuf::from("/wt/.gitignore")], &mut check);
        assert!(w.ignores.get(Path::new("/wt")).is_none_or(|m| !m.contains_key(Path::new("node_modules"))));
    }

    /// Git dirs of a main checkout and a linked worktree (common dir from `commondir`); the real
    /// `git check-ignore` on a dir.
    #[test]
    fn git_dirs_and_check_ignore() {
        let base = std::env::temp_dir().join(format!("ch-watch-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let sh = |c: &str| assert!(Command::new("sh").arg("-c").arg(c).current_dir(&base).env("GIT_AUTHOR_NAME", "t").env("GIT_AUTHOR_EMAIL", "t@t").env("GIT_COMMITTER_NAME", "t").env("GIT_COMMITTER_EMAIL", "t@t").output().unwrap().status.success(), "{c}");
        sh("git init -q -b main r && cd r && echo node_modules/ > .gitignore && mkdir -p node_modules src && git add .gitignore && git commit -q -m 1 && git worktree add -q -b feat ../w");
        let r = base.join("r");
        let (top, common, own) = git_dirs(&r.join("src")).unwrap();
        assert_eq!((top.as_path(), common.as_path(), own.as_path()), (r.as_path(), r.join(".git").as_path(), r.join(".git").as_path()));
        let (top, common, own) = git_dirs(&base.join("w")).unwrap();
        assert_eq!(top, base.join("w"));
        assert_eq!(canon(&common), canon(&r.join(".git")));
        assert_eq!(canon(&own), canon(&r.join(".git/worktrees/w")));

        let ig = check_ignore(&r, &["node_modules".into(), "src".into(), ".gitignore".into()]);
        assert_eq!(ig, HashSet::from([PathBuf::from("node_modules")]));
        std::fs::remove_dir_all(&base).unwrap();
    }
}
