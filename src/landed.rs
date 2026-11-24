//! Landed state of each thread: work not yet in the base branch (open: uncommitted, unpushed,
//! unmerged, PR open) or 0 work left (shipped: landed, abandoned, 0 code). The thread lists put
//! open threads and shipped threads in 2 sections; shipped ones are ready to archive.
//!
//! 1 state per checkout, the worst wins. A checkout on the default branch (main / master) and a
//! dir without git count as 0 code: the owner handles main. A branch landed when its content is
//! in `origin/<default>` or the local `<default>` (a local merge): ancestor, same patches
//! (`git cherry`: rebase merges), or a merge that changes 0 (`git merge-tree`: squash merges).
//! A new branch (0 commit since its creation) counts as 0 code too. 0 code: shipped only when
//! every agent reported done; until then a plain thread (a new thread has 0 changes yet).
//! 0 code, agents not done, idle 3 days: stale (the owner decides); idle 30 days: archived.
//!
//! Event driven, 0 pass: a worker reads every thread once at the start (the disk cache is on
//! screen meanwhile), then only what a bus topic names: a work tree edit → `git status` of that
//! checkout; a git dir change (commit, push, fetch, branch switch) → the checkouts of that repo;
//! a thread.json change → that thread. Own actions set the expected state at once (`expect`).
//! `git fetch` only for a thread list on screen (5 min per repo at most) and after a PR merge.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use crate::bus::{self, Topic};
use crate::watch::canon;
use crate::engine::store;
use crate::gh::State;
use crate::threads::git;

/// Min time between 2 fetches of a repo (a thread list on screen asks for them).
const FETCH_EVERY: Duration = Duration::from_secs(300);
/// A fetch that takes longer: stopped.
const FETCH_MAX: Duration = Duration::from_secs(60);
const DAY_MS: u64 = 24 * 3600 * 1000;
/// A thread with 0 code, agents not done, idle this long: stale (the owner decides).
pub const STALE_MS: u64 = 3 * DAY_MS;
/// A stale thread idle this long: archived by the background pass.
pub const ARCHIVE_MS: u64 = 30 * DAY_MS;

/// A thread with LAND, its agents DONE or not, last ACTIVITY (unix ms): stale at NOW.
pub fn stale(land: Option<&Land>, done: bool, activity: u64, now: u64) -> bool {
    land == Some(&Land::NoCode) && !done && now.saturating_sub(activity) > STALE_MS
}

/// Days left until the auto-archive of a stale thread (last ACTIVITY, unix ms) at NOW.
pub fn archive_in_days(activity: u64, now: u64) -> u64 {
    (activity + ARCHIVE_MS).saturating_sub(now).div_ceil(DAY_MS)
}

/// The landed state of a thread (the worst of its checkouts).
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Land {
    /// N uncommitted / untracked files.
    Dirty(usize),
    /// N commits on 0 remote.
    Unpushed(usize),
    /// Pushed, content not in the base, 0 open PR.
    Unmerged,
    /// The PR (label) is open / draft.
    PrOpen(String),
    /// The PR (label) closed without a merge, the branch as it was at the close.
    Abandoned(String),
    /// The branch's content is in the base.
    Landed,
    /// 0 task branch (0 git, the default branch) or a new branch without a commit. Shipped only
    /// when every agent reported done (`Card::shipped`).
    NoCode,
}

impl Land {
    /// Work not in the base yet: the open section.
    pub fn open(&self) -> bool {
        matches!(self, Land::Dirty(_) | Land::Unpushed(_) | Land::Unmerged | Land::PrOpen(_))
    }

    /// The card's state text.
    pub fn badge(&self) -> String {
        match self {
            Land::Dirty(n) => format!("{n} uncommitted"),
            Land::Unpushed(n) => format!("{n} unpushed"),
            Land::Unmerged => "pushed, not merged".into(),
            Land::PrOpen(pr) => format!("{pr} open"),
            Land::Abandoned(pr) => format!("abandoned: {pr} closed"),
            Land::Landed => "shipped".into(),
            Land::NoCode => "nothing to ship".into(),
        }
    }

    /// 0: the worst (most work left).
    fn rank(&self) -> u8 {
        match self {
            Land::Dirty(_) => 0,
            Land::Unpushed(_) => 1,
            Land::Unmerged => 2,
            Land::PrOpen(_) => 3,
            Land::Abandoned(_) => 4,
            Land::Landed => 5,
            Land::NoCode => 6,
        }
    }
}

/// A PR of a thread, as the last PR pass read it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrSeen {
    pub label: String,
    pub state: State,
    /// Its head branch.
    pub head: String,
    pub head_oid: String,
}

static LANDS: Mutex<Option<HashMap<String, Land>>> = Mutex::new(None);
static PRS: Mutex<Option<HashMap<String, Vec<PrSeen>>>> = Mutex::new(None);

/// The landed state of thread ID (None: 0 pass read it yet).
pub fn of(id: &str) -> Option<Land> {
    LANDS.lock().ok().and_then(|m| m.as_ref()?.get(id).cloned())
}

/// The PRs of thread ID (the PR pass). New ones: its state again; a new merge: a fetch (the
/// merge commit on the base branch).
pub fn set_prs(id: &str, prs: Vec<PrSeen>) {
    let Ok(mut m) = PRS.lock() else { return };
    let old = m.get_or_insert_with(HashMap::new).insert(id.to_string(), prs.clone());
    drop(m);
    if old.as_ref() != Some(&prs) {
        let merged = |v: &[PrSeen]| v.iter().filter(|p| p.state == State::Merged).count();
        if merged(&prs) > old.as_deref().map_or(0, merged) {
            kick(In::Visible);
        }
        kick(In::Prs(id.to_string()));
    }
}

fn prs_of(id: &str) -> Vec<PrSeen> {
    PRS.lock().ok().and_then(|m| m.as_ref()?.get(id).cloned()).unwrap_or_default()
}

static VERSION: AtomicU64 = AtomicU64::new(0);
static READ_US: AtomicU64 = AtomicU64::new(0);
/// When the worker went idle (unix ms), 0: at work.
static IDLE_AT: AtomicU64 = AtomicU64::new(0);

/// The worker waits for topics since 300 ms at least (perf cases: a quiet start).
#[cfg(test)]
pub fn quiet() -> bool {
    let at = IDLE_AT.load(Ordering::Relaxed);
    at > 0 && crate::engine::now_ms().saturating_sub(at) > 300
}

/// The time of the worker's last read (git processes): the perf cases subtract it.
#[cfg(test)]
pub fn last_read() -> Duration {
    Duration::from_micros(READ_US.load(Ordering::Relaxed))
}

static BATCHES: AtomicU64 = AtomicU64::new(0);

/// Batches with a git read so far (perf cases: 0 during idle and install storms).
#[cfg(test)]
pub fn batches() -> u64 {
    BATCHES.load(Ordering::Relaxed)
}

/// Goes up with each state change: the lists take the new states.
pub fn version() -> u64 {
    VERSION.load(Ordering::Relaxed)
}

/// True: a change.
fn set(id: &str, land: Land) -> bool {
    let Ok(mut m) = LANDS.lock() else { return false };
    let new = m.get_or_insert_with(HashMap::new).insert(id.to_string(), land.clone()) != Some(land);
    if new {
        VERSION.fetch_add(1, Ordering::Relaxed);
    }
    new
}

/// Thread ID gone. True: it had a state.
fn unset(id: &str) -> bool {
    let gone = LANDS.lock().ok().is_some_and(|mut m| m.as_mut().is_some_and(|m| m.remove(id).is_some()));
    if gone {
        VERSION.fetch_add(1, Ordering::Relaxed);
    }
    gone
}

/// Base content checks: (rev oid, base oid) → landed. Oids fix the answer.
type Seen = HashMap<(String, String), bool>;

/// The default branch of DIR's repo: `origin/HEAD`, else main / master (remote or local).
fn default_branch(dir: &Path) -> Option<String> {
    let (ok, out, _) = git(dir, &["symbolic-ref", "--quiet", "--short", "refs/remotes/origin/HEAD"]);
    if let Some(b) = out.trim().strip_prefix("origin/").filter(|_| ok) {
        return Some(b.to_string());
    }
    ["main", "master"].into_iter().find(|b| oid(dir, &format!("origin/{b}")).is_some() || oid(dir, b).is_some()).map(String::from)
}

fn oid(dir: &Path, rev: &str) -> Option<String> {
    let (ok, out, _) = git(dir, &["rev-parse", "--verify", "--quiet", &format!("{rev}^{{commit}}")]);
    let o = out.trim().to_string();
    (ok && !o.is_empty()).then_some(o)
}

/// The content of commit REV is in commit BASE: ancestor, same patches, or a merge of REV into
/// BASE changes 0 (squash merge).
fn contained(dir: &Path, rev: &str, base: &str, seen: &mut Seen) -> bool {
    let key = (rev.to_string(), base.to_string());
    if let Some(&b) = seen.get(&key) {
        return b;
    }
    let ancestor = || git(dir, &["merge-base", "--is-ancestor", rev, base]).0;
    let cherry = || {
        let (ok, out, _) = git(dir, &["cherry", base, rev]);
        ok && out.lines().all(|l| l.starts_with('-'))
    };
    let merge = || {
        let (ok, out, _) = git(dir, &["merge-tree", "--write-tree", base, rev]);
        let tree = git(dir, &["rev-parse", &format!("{base}^{{tree}}")]).1;
        ok && out.lines().next().map(str::trim) == Some(tree.trim())
    };
    let b = ancestor() || cherry() || merge();
    seen.insert(key, b);
    b
}

/// The state of checkout DIR. BRANCH: the branch to read (an archived thread's kept branch in
/// its repo; None: DIR's current one). DIRTY: count uncommitted files.
fn checkout(dir: &Path, branch: Option<&str>, dirty: bool, prs: &[PrSeen], seen: &mut Seen) -> Land {
    let Some((b, def)) = task_branch(dir, branch) else { return Land::NoCode };
    if dirty {
        let n = dirty_count(dir);
        if n > 0 {
            return Land::Dirty(n);
        }
    }
    with_prs(&refs(dir, &b, def.as_deref(), seen), &b, prs)
}

/// The task branch of checkout DIR (BRANCH: that one, None: DIR's current one) and the repo's
/// default branch. None: 0 code (0 git, the default branch).
fn task_branch(dir: &Path, branch: Option<&str>) -> Option<(String, Option<String>)> {
    if !dir.is_dir() || !git(dir, &["rev-parse", "--git-dir"]).0 {
        return None;
    }
    let b = branch.map(String::from).unwrap_or_else(|| crate::threads::current_branch(dir));
    let def = default_branch(dir);
    (!(b.is_empty() || Some(&b) == def.as_ref() || b == "main" || b == "master")).then_some((b, def))
}

/// Uncommitted + untracked files of checkout DIR.
fn dirty_count(dir: &Path) -> usize {
    // 0 index refresh: a write of the index would come back as a topic (a loop)
    git(dir, &["--no-optional-locks", "status", "--porcelain"]).1.lines().count()
}

/// What git says about a task branch, PRs apart.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
enum Refs {
    /// 0 branch, or a new one (0 move since its creation, its tip in the base).
    NoCode,
    /// Its content is in the base.
    Landed,
    /// Its tip REV, N commits on 0 remote.
    Ahead { rev: String, unpushed: usize },
}

/// The refs state of branch B in checkout DIR (DEF: the repo's default branch).
fn refs(dir: &Path, b: &str, def: Option<&str>, seen: &mut Seen) -> Refs {
    let Some(rev) = oid(dir, &format!("refs/heads/{b}")) else { return Refs::NoCode };
    let bases: Vec<String> = def.iter().flat_map(|d| [format!("origin/{d}"), d.to_string()]).filter_map(|r| oid(dir, &r)).collect();
    // a new branch (0 move since its creation, its tip in the base): 0 work yet, not shipped
    let fresh = git(dir, &["reflog", "show", "--format=%H", &format!("refs/heads/{b}")]).1.lines().count() == 1;
    if fresh && bases.iter().any(|base| git(dir, &["merge-base", "--is-ancestor", &rev, base]).0) {
        return Refs::NoCode;
    }
    if bases.iter().any(|base| contained(dir, &rev, base, seen)) {
        return Refs::Landed;
    }
    let unpushed = git(dir, &["rev-list", "--count", &rev, "--not", "--remotes"]).1.trim().parse().unwrap_or(0);
    Refs::Ahead { rev, unpushed }
}

/// The state of branch B with refs state R and the thread's PRS.
fn with_prs(r: &Refs, b: &str, prs: &[PrSeen]) -> Land {
    let (rev, unpushed) = match r {
        Refs::NoCode => return Land::NoCode,
        Refs::Landed => return Land::Landed,
        Refs::Ahead { rev, unpushed } => (rev, *unpushed),
    };
    let mine: Vec<&PrSeen> = prs.iter().filter(|p| p.head == b).collect();
    if mine.iter().any(|p| p.state == State::Merged && &p.head_oid == rev) {
        return Land::Landed;
    }
    if unpushed > 0 {
        return Land::Unpushed(unpushed);
    }
    if let Some(p) = mine.iter().find(|p| matches!(p.state, State::Open | State::Draft)) {
        return Land::PrOpen(p.label.clone());
    }
    let closed = mine.iter().find(|p| p.state == State::Closed && &p.head_oid == rev);
    if let Some(p) = closed.filter(|_| !mine.iter().any(|p| p.state == State::Merged)) {
        return Land::Abandoned(p.label.clone());
    }
    Land::Unmerged
}

/// The state of thread T: the worst of its checkouts. Archived: its kept branch in its repo.
pub fn thread(t: &store::Thread, prs: &[PrSeen], seen: &mut Seen) -> Land {
    let lands: Vec<Land> = if t.state == "archived" {
        let repo = crate::threads::repo_of_thread_store(t);
        match (repo, t.worktree_pending.as_deref()) {
            (Some(r), Some(b)) => vec![checkout(&r, Some(b), false, prs, seen)],
            _ => Vec::new(),
        }
    } else {
        t.all_checkouts().iter().map(|c| checkout(Path::new(&c.dir), None, true, prs, seen)).collect()
    };
    lands.into_iter().min_by_key(Land::rank).unwrap_or(Land::NoCode)
}

/// `git fetch origin` in REPO: 0 prompt, FETCH_MAX at most.
fn fetch(repo: &Path) {
    let argv = ["git", "fetch", "--quiet", "origin"].map(String::from);
    let env = [("GIT_TERMINAL_PROMPT".to_string(), "0".to_string())];
    let _ = crate::setup::run(&argv, repo, &env, FETCH_MAX);
}

/// A message to the worker: a bus topic or a kick.
enum In {
    Bus(Topic),
    /// The PRs of thread ID changed: its state again (0 git read).
    Prs(String),
    /// Read thread ID again (an own action ended, maybe failed).
    Refresh(String),
    /// A thread list is on screen: fetch repos whose last fetch is older than FETCH_EVERY.
    Visible,
}

static KICK: Mutex<Option<Sender<In>>> = Mutex::new(None);

fn kick(m: In) {
    if let Ok(g) = KICK.lock()
        && let Some(tx) = g.as_ref()
    {
        let _ = tx.send(m);
    }
}

/// Own action on thread ID with an expected result LAND (a commit: 1 more unpushed): on screen
/// at once; the git read after the action confirms or corrects it.
pub fn expect(id: &str, land: Land) {
    if set(id, land) {
        bus::changed();
    }
    kick(In::Refresh(id.to_string()));
}

/// Read thread ID again (an own action failed: its expected state goes).
pub fn refresh(id: &str) {
    kick(In::Refresh(id.to_string()));
}

/// A thread list is on screen (each draw; 1 kick per 5 s at most).
pub fn visible() {
    static AT: AtomicU64 = AtomicU64::new(0);
    let now = crate::engine::now_ms();
    if now.saturating_sub(AT.swap(now, Ordering::Relaxed)) > 5000 {
        kick(In::Visible);
    }
}

/// The disk cache of the last states: labels on the 1st frame.
fn cache_path() -> PathBuf {
    crate::engine::data_dir().join("landed.json")
}

/// Start the worker (once per process; 0 in tests: `start_at`).
pub fn start() {
    if !cfg!(test) {
        start_at(store::threads_dir());
    }
}

/// Start the worker on threads dir THREADS (once per process).
pub fn start_at(threads: PathBuf) {
    static ON: OnceLock<()> = OnceLock::new();
    if ON.set(()).is_err() {
        return;
    }
    if let Some(m) = std::fs::read(cache_path()).ok().and_then(|b| serde_json::from_slice::<HashMap<String, Land>>(&b).ok())
        && let Ok(mut l) = LANDS.lock()
    {
        *l = Some(m);
    }
    let (tx, rx) = channel();
    if let Ok(mut k) = KICK.lock() {
        *k = Some(tx.clone());
    }
    let topics = bus::subscribe();
    std::thread::spawn(move || {
        for t in topics {
            if tx.send(In::Bus(t)).is_err() {
                return;
            }
        }
    });
    std::thread::Builder::new().name("landed".into()).spawn(move || Worker::new(threads).run(rx)).ok();
}

/// What the last read of 1 checkout found.
#[derive(Clone, Debug)]
struct Co {
    /// Its top dir, its common git dir (canonical: the watcher's paths).
    top: PathBuf,
    common: PathBuf,
    /// Its own git dir (a linked worktree: `<common>/worktrees/<name>`).
    own: PathBuf,
    /// Its task branch, None: 0 code.
    branch: Option<String>,
    /// The repo's default branch.
    def: Option<String>,
    dirty: usize,
    refs: Refs,
}

impl Co {
    /// Git topic part K of its repo changes what this checkout shows.
    fn hit(&self, k: &bus::Git) -> bool {
        let base = |b: &str| b == "main" || b == "master" || self.def.as_deref() == Some(b);
        match k {
            bus::Git::All => true,
            // the base moved: every branch's landed check; else only the checkouts on it
            bus::Git::Branch(b) | bus::Git::Remote(b) => base(b) || self.branch.as_deref() == Some(b),
            bus::Git::Worktree(n) if n.is_empty() => self.own == self.common,
            bus::Git::Worktree(n) => self.own.file_name().is_some_and(|f| f == n.as_str()) && self.own != self.common,
        }
    }
}

/// The worker: reads only what a topic names, keeps the result per checkout.
struct Worker {
    threads_dir: PathBuf,
    threads: HashMap<String, store::Thread>,
    /// Checkout dir (as the thread stores it) → its last read.
    cos: HashMap<String, Co>,
    /// Archived thread → the state of its kept branch.
    kept: HashMap<String, Land>,
    seen: Seen,
    fetched: HashMap<PathBuf, Instant>,
    fetching: Arc<Mutex<HashSet<PathBuf>>>,
    /// The next auto-archive check.
    archive_at: Instant,
}

/// The 1st auto-archive check after the start: notify's 1st pass has the done notes.
const ARCHIVE_FIRST: Duration = Duration::from_secs(60);

impl Worker {
    fn new(threads_dir: PathBuf) -> Self {
        Worker { threads_dir, threads: HashMap::new(), cos: HashMap::new(), kept: HashMap::new(), seen: Seen::new(), fetched: HashMap::new(), fetching: Arc::default(), archive_at: Instant::now() + ARCHIVE_FIRST }
    }

    /// Every thread once: open ones first (the cache is on screen meanwhile).
    fn init(&mut self) {
        let mut all: Vec<store::Thread> = store::all_in(&self.threads_dir).into_iter().filter(|t| t.state != "deleted").collect();
        all.sort_by_key(|t| t.state == "archived");
        for t in all {
            let id = t.id.clone();
            self.threads.insert(id.clone(), t);
            self.read_thread(&id);
            self.publish(&[id]);
        }
    }

    fn run(mut self, rx: Receiver<In>) {
        self.init();
        self.save();
        loop {
            IDLE_AT.store(crate::engine::now_ms(), Ordering::Relaxed);
            let wait = self.archive_at.saturating_duration_since(Instant::now());
            let first = match rx.recv_timeout(wait) {
                Ok(m) => Some(m),
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => return,
            };
            IDLE_AT.store(0, Ordering::Relaxed);
            let batch: Vec<In> = first.into_iter().chain(std::iter::from_fn(|| rx.try_recv().ok())).collect();
            let at = Instant::now();
            let ids = self.apply(batch);
            READ_US.store(at.elapsed().as_micros() as u64, Ordering::Relaxed);
            if self.publish(&ids) {
                self.save();
            }
            if Instant::now() >= self.archive_at {
                self.auto_archive();
            }
        }
    }

    /// Read what BATCH names. The threads whose state may differ now.
    fn apply(&mut self, batch: Vec<In>) -> Vec<String> {
        let mut full: HashSet<String> = HashSet::new();
        let mut dirty: HashSet<String> = HashSet::new();
        let mut ids: HashSet<String> = HashSet::new();
        let mut gits: HashSet<PathBuf> = HashSet::new();
        for m in batch {
            match m {
                In::Bus(Topic::Thread(id, bus::Part::Meta)) => {
                    let new = store::load_in(&self.threads_dir, &id).filter(|t| t.state != "deleted");
                    let old = self.threads.get(&id);
                    let key = |t: &store::Thread| (t.state.clone(), t.all_checkouts().into_iter().map(|c| c.dir).collect::<Vec<_>>(), t.worktree_pending.clone(), t.repo.clone());
                    if new.as_ref().map(key) != old.map(key) {
                        match new {
                            Some(t) => {
                                full.extend(self.dirs(&t));
                                self.threads.insert(id.clone(), t);
                            }
                            None => {
                                self.threads.remove(&id);
                            }
                        }
                        ids.insert(id);
                    }
                }
                In::Bus(Topic::Thread(..)) => {}
                In::Bus(Topic::Git(g, k)) => {
                    full.extend(self.cos.iter().filter(|(_, c)| c.common == g && c.hit(&k)).map(|(d, _)| d.clone()));
                    gits.insert(g);
                }
                In::Bus(Topic::Work(w)) => dirty.extend(self.cos.iter().filter(|(_, c)| c.top == w).map(|(d, _)| d.clone())),
                In::Bus(Topic::Rescan(r)) => {
                    if r == canon(&self.threads_dir) {
                        // lost thread events: every thread again
                        for t in store::all_in(&self.threads_dir).into_iter().filter(|t| t.state != "deleted") {
                            ids.insert(t.id.clone());
                            self.threads.insert(t.id.clone(), t);
                        }
                    }
                    full.extend(self.cos.iter().filter(|(_, c)| c.top.starts_with(&r) || c.common.starts_with(&r)).map(|(d, _)| d.clone()));
                    gits.insert(r);
                }
                In::Prs(id) => {
                    ids.insert(id);
                }
                In::Refresh(id) => {
                    if let Some(t) = self.threads.get(&id) {
                        full.extend(self.dirs(t));
                    }
                    ids.insert(id);
                }
                In::Visible => self.fetch_due(),
            }
        }
        if !full.is_empty() || !dirty.is_empty() {
            BATCHES.fetch_add(1, Ordering::Relaxed);
        }
        for d in &full {
            self.read_co(d, true);
        }
        for d in dirty.difference(&full) {
            self.read_co(d, false);
        }
        let touched: HashSet<&String> = full.iter().chain(&dirty).collect();
        let mut out: Vec<String> = ids.into_iter().collect();
        for (id, t) in &self.threads {
            let hit = if t.state == "archived" {
                // its kept branch: a ref change in its repo
                crate::threads::repo_of_thread_store(t).and_then(|r| crate::watch::git_dirs(&r)).is_some_and(|(_, c, _)| gits.iter().any(|g| canon(&c).starts_with(g)))
            } else {
                self.dirs(t).iter().any(|d| touched.contains(d))
            };
            if hit && !out.contains(id) {
                out.push(id.clone());
            }
        }
        for id in out.iter().filter(|id| self.threads.get(*id).is_some_and(|t| t.state == "archived")).cloned().collect::<Vec<_>>() {
            self.read_thread(&id);
        }
        out
    }

    /// The checkout dirs of thread T (archived: 0).
    fn dirs(&self, t: &store::Thread) -> Vec<String> {
        if t.state == "archived" {
            return Vec::new();
        }
        t.all_checkouts().into_iter().map(|c| c.dir).collect()
    }

    /// Read thread ID: its checkouts 0 read yet (open), its kept branch (archived).
    fn read_thread(&mut self, id: &str) {
        let Some(t) = self.threads.get(id).cloned() else { return };
        if t.state == "archived" {
            let land = thread(&t, &prs_of(id), &mut self.seen);
            self.kept.insert(id.to_string(), land);
            return;
        }
        for d in self.dirs(&t) {
            if !self.cos.contains_key(&d) {
                self.read_co(&d, true);
            }
        }
    }

    /// Read checkout DIR: FULL: branch, refs, dirty; else dirty only.
    fn read_co(&mut self, dir: &str, full: bool) {
        let p = Path::new(dir);
        if !full && let Some(c) = self.cos.get_mut(dir) {
            if c.branch.is_some() {
                c.dirty = dirty_count(p);
            }
            return;
        }
        let (top, common, own) = crate::watch::git_dirs(p).map(|(t, c, o)| (canon(&t), canon(&c), canon(&o))).unwrap_or_default();
        let (branch, def) = task_branch(p, None).map_or((None, None), |(b, d)| (Some(b), d));
        let (dirty, refs) = match &branch {
            Some(b) => (dirty_count(p), refs(p, b, def.as_deref(), &mut self.seen)),
            None => (0, Refs::NoCode),
        };
        self.cos.insert(dir.to_string(), Co { top, common, own, branch, def, dirty, refs });
        // oids fix the answers: only the size needs a cap
        if self.seen.len() > 4096 {
            self.seen.clear();
        }
    }

    /// The state of thread ID from the last reads (0 git).
    fn land(&self, id: &str) -> Option<Land> {
        let t = self.threads.get(id)?;
        if t.state == "archived" {
            return self.kept.get(id).cloned();
        }
        let prs = prs_of(id);
        let lands = self.dirs(t).into_iter().filter_map(|d| self.cos.get(&d).cloned()).map(|c| match &c.branch {
            None => Land::NoCode,
            Some(_) if c.dirty > 0 => Land::Dirty(c.dirty),
            Some(b) => with_prs(&c.refs, b, &prs),
        });
        Some(lands.min_by_key(Land::rank).unwrap_or(Land::NoCode))
    }

    /// Set the states of IDS; the UI wakes on a change. True: 1 changed.
    fn publish(&self, ids: &[String]) -> bool {
        let mut any = false;
        for id in ids {
            match self.land(id) {
                Some(l) => any |= set(id, l),
                None => any |= unset(id),
            }
        }
        if any {
            bus::changed();
        }
        any
    }

    fn save(&self) {
        let m = LANDS.lock().ok().and_then(|l| l.clone()).unwrap_or_default();
        if let Ok(b) = serde_json::to_vec(&m) {
            let _ = crate::engine::write_atomic(&cache_path(), &b);
        }
    }

    /// Fetch each repo of the threads whose last fetch is older than FETCH_EVERY (1 at a time per
    /// repo, off this thread). New remote refs come back as `Git` topics.
    fn fetch_due(&mut self) {
        let repos: HashSet<PathBuf> = self.threads.values().filter_map(crate::threads::repo_of_thread_store).collect();
        for r in repos {
            if self.fetched.get(&r).is_some_and(|t| t.elapsed() < FETCH_EVERY) {
                continue;
            }
            self.fetched.insert(r.clone(), Instant::now());
            let busy = self.fetching.clone();
            if !busy.lock().is_ok_and(|mut b| b.insert(r.clone())) {
                continue;
            }
            std::thread::spawn(move || {
                if git(&r, &["remote", "get-url", "origin"]).0 {
                    fetch(&r);
                }
                if let Ok(mut b) = busy.lock() {
                    b.remove(&r);
                }
            });
        }
    }

    /// Archive each open thread stale for ARCHIVE_MS; the next check at the next deadline.
    fn auto_archive(&mut self) {
        let now = crate::engine::now_ms();
        let mut next = u64::MAX;
        for (id, t) in &self.threads {
            // 0 code, agents not done: archived at ACTIVITY + ARCHIVE_MS (stale since STALE_MS)
            if t.state == "archived" || self.land(id) != Some(Land::NoCode) || crate::notify::reported_done(id) {
                continue;
            }
            let activity = crate::nav::last_item(id).unwrap_or(0).max(t.updated);
            let due = activity + ARCHIVE_MS;
            if now > due {
                let _ = crate::threads::archive(id);
            } else {
                next = next.min(due);
            }
        }
        // the next deadline (new activity moves it: checked then again); 0 deadline: 1 day (a
        // thread can turn 0 code later; a state change does not wake this timer)
        let wait = Duration::from_millis(next.saturating_sub(now)).min(Duration::from_millis(DAY_MS));
        self.archive_at = Instant::now() + wait;
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use std::process::{Command, Stdio};

    fn sh(dir: &Path, c: &str) {
        let ok = Command::new("sh").arg("-c").arg(c).current_dir(dir).env("GIT_AUTHOR_NAME", "t").env("GIT_AUTHOR_EMAIL", "t@t").env("GIT_COMMITTER_NAME", "t").env("GIT_COMMITTER_EMAIL", "t@t").stdout(Stdio::null()).stderr(Stdio::null()).status().unwrap().success();
        assert!(ok, "{c}");
    }

    /// A remote, its clone (main checkout) and a worktree on branch `feat` with 1 commit.
    fn setup(name: &str) -> (PathBuf, PathBuf, PathBuf) {
        let base = std::env::temp_dir().join(format!("ch-landed-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        sh(&base, "git init -q --bare -b main o.git && git clone -q o.git r 2>/dev/null && cd r && printf 'a\\nb\\nc\\n' > f && git add f && git commit -q -m 1 && git push -q origin main && git remote set-head origin main");
        let repo = base.join("r");
        let wt = base.join("w");
        sh(&repo, "git worktree add -q -b feat ../w && cd ../w && echo d >> f && git commit -q -am feat");
        (base, repo, wt)
    }

    fn land(dir: &Path, prs: &[PrSeen]) -> Land {
        checkout(dir, None, true, prs, &mut Seen::new())
    }

    fn pr(state: State, oid: &str) -> PrSeen {
        PrSeen { label: "r#7".into(), state, head: "feat".into(), head_oid: oid.into() }
    }

    /// The path of a branch: dirty → unpushed → pushed → PR open → squash merged on the remote:
    /// shipped. The main checkout and a dir without git: 0 code.
    #[test]
    fn a_branch_goes_from_open_to_shipped() {
        let (base, repo, wt) = setup("path");
        assert_eq!(land(&repo, &[]), Land::NoCode, "main checkout: skipped");
        assert_eq!(land(&base, &[]), Land::NoCode, "0 git");

        // a new thread's branch, 0 commit yet: 0 code (not shipped); a branch made at commits
        // not in the base (a colleague's, fetched): open
        sh(&repo, "git worktree add -q -b fresh ../f && git branch theirs feat");
        assert_eq!(land(&base.join("f"), &[]), Land::NoCode);
        assert_eq!(checkout(&repo, Some("theirs"), false, &[], &mut Seen::new()), Land::Unpushed(1));

        sh(&wt, "echo x > new.txt");
        assert_eq!(land(&wt, &[]), Land::Dirty(1));

        sh(&wt, "rm new.txt && echo e >> f && git commit -q -am 2nd");
        assert_eq!(land(&wt, &[]), Land::Unpushed(2));

        sh(&wt, "git push -q origin feat");
        assert_eq!(land(&wt, &[]), Land::Unmerged);

        let head = oid(&wt, "HEAD").unwrap();
        assert_eq!(land(&wt, &[pr(State::Open, &head)]), Land::PrOpen("r#7".into()));
        assert!(Land::PrOpen("r#7".into()).open() && !Land::Landed.open());

        // squash merge of the 2 commits on the remote: 1 other commit, same content (0 cherry
        // match: the merge-tree check finds it)
        sh(&base, "git clone -q o.git s 2>/dev/null && cd s && printf 'd\\ne\\n' >> f && git commit -q -am squash && git push -q origin main");
        sh(&repo, "git fetch -q origin");
        assert_eq!(land(&wt, &[pr(State::Merged, &head)]), Land::Landed);
        assert_eq!(land(&wt, &[]), Land::Landed, "content check alone: squash found");

        std::fs::remove_dir_all(&base).unwrap();
    }

    /// A local merge into main (main unpushed): shipped (D2). A closed PR, branch as at the
    /// close: abandoned; a new commit after the close: unpushed.
    #[test]
    fn local_merge_ships_closed_pr_abandons() {
        let (base, repo, wt) = setup("local");
        sh(&repo, "git merge -q --no-ff -m m feat");
        assert_eq!(land(&wt, &[]), Land::Landed);

        let (base2, _, wt2) = setup("closed");
        sh(&wt2, "git push -q origin feat");
        let head = oid(&wt2, "HEAD").unwrap();
        assert_eq!(land(&wt2, &[pr(State::Closed, &head)]), Land::Abandoned("r#7".into()));
        assert!(!Land::Abandoned("r#7".into()).open());

        sh(&wt2, "echo e >> f && git commit -q -am more");
        assert_eq!(land(&wt2, &[pr(State::Closed, &head)]), Land::Unpushed(1));

        // a merged PR, new commits after the merge: open (the old archive offer's bug)
        sh(&wt2, "git push -q origin feat");
        assert_eq!(land(&wt2, &[pr(State::Merged, &head)]), Land::Unmerged);

        std::fs::remove_dir_all(&base).unwrap();
        std::fs::remove_dir_all(&base2).unwrap();
    }

    /// Stale: 0 code, agents not done, idle 3+ days; the days left to the auto-archive.
    #[test]
    fn stale_after_3_days_archive_after_30() {
        let (now, day) = (100 * DAY_MS, DAY_MS);
        assert!(stale(Some(&Land::NoCode), false, now - 3 * day - 1, now));
        assert!(!stale(Some(&Land::NoCode), false, now - 2 * day, now), "recent");
        assert!(!stale(Some(&Land::NoCode), true, now - 9 * day, now), "done: shipped");
        assert!(!stale(Some(&Land::Unpushed(1)), false, now - 9 * day, now), "code: open");
        assert!(!stale(None, false, 0, now), "0 state read");
        assert_eq!(archive_in_days(now - 5 * day - 1, now), 25);
        assert_eq!(archive_in_days(now - 31 * day, now), 0);
    }

    /// A thread: the worst of its checkouts; archived: its kept branch in the repo, 0 dirty read.
    #[test]
    fn thread_takes_the_worst_checkout() {
        let (base, repo, wt) = setup("thread");
        let mut t = store::Thread::new("t", "x", &repo, vec!["ada".into()]);
        assert_eq!(thread(&t, &[], &mut Seen::new()), Land::NoCode, "on main: 0 code");

        t.checkouts.push(store::Checkout { dir: wt.display().to_string(), branch: Some("feat".into()) });
        assert_eq!(thread(&t, &[], &mut Seen::new()), Land::Unpushed(1));

        let mut a = store::Thread::new("a", "x", &repo, vec!["ada".into()]);
        a.state = "archived".into();
        a.repo = Some(repo.display().to_string());
        a.worktree_pending = Some("feat".into());
        sh(&wt, "echo x > new.txt");
        assert_eq!(thread(&a, &[], &mut Seen::new()), Land::Unpushed(1), "archived: 0 dirty read");

        std::fs::remove_dir_all(&base).unwrap();
    }

    /// The worker reads only what a topic names: a work tree edit → dirty, a git dir change →
    /// refs (a commit, a push); an own action's expected state shows at once.
    #[test]
    fn worker_reads_what_a_topic_names() {
        let (base, repo, wt) = setup("worker");
        let threads = base.join("threads");
        let mut t = store::Thread::new("w", "x", &repo, vec!["ada".into()]);
        t.checkouts.push(store::Checkout { dir: wt.display().to_string(), branch: Some("feat".into()) });
        std::fs::create_dir_all(threads.join(&t.id)).unwrap();
        std::fs::write(threads.join(&t.id).join("thread.json"), serde_json::to_vec(&t).unwrap()).unwrap();

        let mut w = Worker::new(threads.clone());
        w.init();
        assert_eq!(of(&t.id), Some(Land::Unpushed(1)));
        let (top, common, _) = crate::watch::git_dirs(&wt).unwrap();
        let (top, common) = (canon(&top), canon(&common));

        sh(&wt, "echo x > new.txt");
        let ids = w.apply(vec![In::Bus(Topic::Work(top.clone()))]);
        assert!(w.publish(&ids));
        assert_eq!(of(&t.id), Some(Land::Dirty(1)), "edit: status only");

        sh(&wt, "git add new.txt && git commit -q -m 2");
        let ids = w.apply(vec![In::Bus(Topic::Git(common.clone(), bus::Git::All)), In::Bus(Topic::Work(top.clone()))]);
        w.publish(&ids);
        assert_eq!(of(&t.id), Some(Land::Unpushed(2)), "commit");

        sh(&wt, "git push -q origin feat");
        let ids = w.apply(vec![In::Bus(Topic::Git(common.clone(), bus::Git::All))]);
        w.publish(&ids);
        assert_eq!(of(&t.id), Some(Land::Unmerged), "push");

        // another repo's change: 0 read, 0 change
        assert!(w.apply(vec![In::Bus(Topic::Git(PathBuf::from("/elsewhere/.git"), bus::Git::All))]).is_empty());

        // optimistic: at once; a refresh puts the truth back
        set(&t.id, Land::Unpushed(9));
        assert_eq!(of(&t.id), Some(Land::Unpushed(9)));
        let ids = w.apply(vec![In::Refresh(t.id.clone())]);
        w.publish(&ids);
        assert_eq!(of(&t.id), Some(Land::Unmerged));

        std::fs::remove_dir_all(&base).unwrap();
    }
}
