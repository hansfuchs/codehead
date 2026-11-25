//! Landed state of each thread: work not yet in the base branch (open: uncommitted, unpushed,
//! unmerged, PR open) or 0 work left (shipped: landed, abandoned, 0 code). The thread lists put
//! open threads and shipped threads in 2 sections; shipped ones are ready to archive.
//!
//! 1 state per checkout, the worst wins. A checkout on the default branch (main / master): on
//! main, never shipped (0 branch to tell landed work from open work: the owner archives it, or
//! stale after 3 idle days). A dir without git: 0 code. A branch landed when its content is
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
use std::ops::ControlFlow;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use crate::domain::threads::git;
use crate::engine::store;
use crate::infra::bus::{self, Topic};
use crate::infra::gh::State;
use crate::infra::watch::canon;

/// Min time between 2 fetches of a repo (a thread list on screen asks for them).
const FETCH_EVERY: Duration = Duration::from_secs(300);
/// A fetch that takes longer: stopped.
const FETCH_MAX: Duration = Duration::from_secs(60);
const DAY_MS: u64 = 24 * 3600 * 1000;
/// A thread with 0 code, agents not done, idle this long: stale (the owner decides).
pub(crate) const STALE_MS: u64 = 3 * DAY_MS;
/// A stale thread idle this long: archived by the background pass.
pub(crate) const ARCHIVE_MS: u64 = 30 * DAY_MS;

/// A thread with LAND, its agents DONE or not, last ACTIVITY (unix ms): stale at NOW.
pub(crate) fn stale(land: Option<&Land>, done: bool, activity: u64, now: u64) -> bool {
    idle_kind(land, done) && now.saturating_sub(activity) > STALE_MS
}

/// LAND, agents DONE or not: stale once idle (0 code, agents not done; on main: done or not).
fn idle_kind(land: Option<&Land>, done: bool) -> bool {
    match land {
        Some(Land::NoCode) => !done,
        Some(Land::Main) => true,
        _ => false,
    }
}

/// Days left until the auto-archive of a stale thread (last ACTIVITY, unix ms) at NOW.
pub(crate) fn archive_in_days(activity: u64, now: u64) -> u64 {
    (activity + ARCHIVE_MS).saturating_sub(now).div_ceil(DAY_MS)
}

/// The landed state of a thread (the worst of its checkouts).
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) enum Land {
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
    /// 0 task branch (0 git) or a new branch without a commit. Shipped only when every agent
    /// reported done (`Card::shipped`).
    NoCode,
    /// A checkout on the default branch: never shipped (the owner archives it).
    Main,
}

impl Land {
    /// Work not in the base yet: the open section.
    pub(crate) fn open(&self) -> bool {
        matches!(
            self,
            Land::Dirty(_) | Land::Unpushed(_) | Land::Unmerged | Land::PrOpen(_)
        )
    }

    /// The card's state text.
    pub(crate) fn badge(&self) -> String {
        match self {
            Land::Dirty(count) => format!("{count} uncommitted"),
            Land::Unpushed(count_value) => format!("{count_value} unpushed"),
            Land::Unmerged => "pushed, not merged".into(),
            Land::PrOpen(pr) => format!("{pr} open"),
            Land::Abandoned(pr) => format!("abandoned: {pr} closed"),
            Land::Landed => "shipped".into(),
            Land::NoCode => "nothing to ship".into(),
            Land::Main => "on main".into(),
        }
    }

    /// 0: the worst (most work left).
    fn rank(&self) -> u8 {
        match self {
            Land::Dirty(_) => 0,
            Land::Unpushed(_) => 1,
            Land::Unmerged => 2,
            Land::PrOpen(_) => 3,
            // not open, never shipped: worse than a landed checkout of the same thread
            Land::Main => 4,
            Land::Abandoned(_) => 5,
            Land::Landed => 6,
            Land::NoCode => 7,
        }
    }
}

/// A PR of a thread, as the last PR pass read it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PrSeen {
    pub label: String,
    pub state: State,
    /// Its head branch.
    pub head: String,
    pub head_oid: String,
}

static LANDS: Mutex<Option<HashMap<String, Land>>> = Mutex::new(None);
static PRS: Mutex<Option<HashMap<String, Vec<PrSeen>>>> = Mutex::new(None);

/// The landed state of thread ID (None: 0 pass read it yet).
pub(crate) fn of(id: &str) -> Option<Land> {
    LANDS.lock().ok().and_then(|message| message.as_ref()?.get(id).cloned())
}

/// The PRs of thread ID (the PR pass). New ones: its state again; a new merge: a fetch (the
/// merge commit on the base branch).
pub(crate) fn set_prs(id: &str, prs: &[PrSeen]) {
    let Ok(mut message) = PRS.lock() else { return };
    let old = message
        .get_or_insert_with(HashMap::new)
        .insert(id.to_string(), prs.to_owned());

    drop(message);

    if old.as_deref() != Some(prs) {
        let merged = |value: &[PrSeen]| value.iter().filter(|path| path.state == State::Merged).count();

        if merged(prs) > old.as_deref().map_or(0, merged) {
            kick(In::Visible);
        }

        kick(In::Prs(id.to_string()));
    }
}

fn prs_of(id: &str) -> Vec<PrSeen> {
    PRS.lock()
        .ok()
        .and_then(|message| message.as_ref()?.get(id).cloned())
        .unwrap_or_default()
}

static VERSION: AtomicU64 = AtomicU64::new(0);
static READ_US: AtomicU64 = AtomicU64::new(0);
/// When the worker went idle (unix ms), 0: at work.
static IDLE_AT: AtomicU64 = AtomicU64::new(0);

/// The worker waits for topics since 300 ms at least (perf cases: a quiet start).
#[cfg(test)]
pub(crate) fn quiet() -> bool {
    let at = IDLE_AT.load(Ordering::Relaxed);

    at > 0 && crate::engine::now_ms().saturating_sub(at) > 300
}

/// The time of the worker's last read (git processes): the perf cases subtract it.
#[cfg(test)]
pub(crate) fn last_read() -> Duration {
    Duration::from_micros(READ_US.load(Ordering::Relaxed))
}

static BATCHES: AtomicU64 = AtomicU64::new(0);

/// Batches with a git read so far (perf cases: 0 during idle and install storms).
#[cfg(test)]
pub(crate) fn batches() -> u64 {
    BATCHES.load(Ordering::Relaxed)
}

/// Goes up with each state change: the lists take the new states.
pub(crate) fn version() -> u64 {
    VERSION.load(Ordering::Relaxed)
}

/// True: a change.
fn set(id: &str, land: Land) -> bool {
    let Ok(mut message) = LANDS.lock() else { return false };
    let new = message
        .get_or_insert_with(HashMap::new)
        .insert(id.to_string(), land.clone())
        != Some(land);

    if new {
        VERSION.fetch_add(1, Ordering::Relaxed);
    }

    new
}

/// Thread ID gone. True: it had a state.
fn unset(id: &str) -> bool {
    let gone = LANDS.lock().is_ok_and(|mut message| {
        message
            .as_mut()
            .is_some_and(|message_value| message_value.remove(id).is_some())
    });

    if gone {
        VERSION.fetch_add(1, Ordering::Relaxed);
    }

    gone
}

/// Base content checks: (rev oid, base oid) → landed. Oids fix the answer.
type Seen = HashMap<(String, String), bool>;

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

/// A message to the worker: a bus topic or a kick.
enum In {
    Bus(Topic),
    /// The PRs of thread ID changed: its state again (0 git read).
    Prs(String),
    /// Read thread ID again (an own action ended, maybe failed).
    Refresh(String),
    /// A thread list is on screen: fetch repos whose last fetch is older than `FETCH_EVERY`.
    Visible,
}

static KICK: Mutex<Option<Sender<In>>> = Mutex::new(None);

fn kick(message: In) {
    if let Ok(group) = KICK.lock()
        && let Some(tx) = group.as_ref()
    {
        let _ = tx.send(message);
    }
}

/// Own action on thread ID with an expected result LAND (a commit: 1 more unpushed): on screen
/// at once; the git read after the action confirms or corrects it.
pub(crate) fn expect(id: &str, land: Land) {
    if set(id, land) {
        bus::changed();
    }

    kick(In::Refresh(id.to_string()));
}

/// Read thread ID again (an own action failed: its expected state goes).
pub(crate) fn refresh(id: &str) {
    kick(In::Refresh(id.to_string()));
}

/// A thread list is on screen (each draw; 1 kick per 5 s at most).
pub(crate) fn visible() {
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
pub(crate) fn start() {
    if !cfg!(test) {
        start_at(store::threads_dir());
    }
}

/// Start the worker on threads dir THREADS (once per process).
pub(crate) fn start_at(threads: PathBuf) {
    static ON: OnceLock<()> = OnceLock::new();

    if ON.set(()).is_err() {
        return;
    }

    if let Some(message) = std::fs::read(cache_path())
        .ok()
        .and_then(|bytes| serde_json::from_slice::<HashMap<String, Land>>(&bytes).ok())
        && let Ok(mut line) = LANDS.lock()
    {
        *line = Some(message);
    }

    let (tx, rx) = channel();

    if let Ok(mut key) = KICK.lock() {
        *key = Some(tx.clone());
    }

    let topics = bus::subscribe();

    std::thread::spawn(
        move || {
            if let ControlFlow::Break(()) = forward_bus_topics(&tx, topics) {}
        },
    );

    std::thread::Builder::new()
        .name("landed".into())
        .spawn(move || Worker::new(threads).run(&rx))
        .ok();
}

fn forward_bus_topics(tx: &Sender<In>, topics: Receiver<Topic>) -> ControlFlow<()> {
    for thread_value in topics {
        if let ControlFlow::Break(()) = send_bus_topic(tx, thread_value) {
            return ControlFlow::Break(());
        }
    }

    ControlFlow::Continue(())
}

fn send_bus_topic(tx: &Sender<In>, thread_value: Topic) -> ControlFlow<()> {
    if tx.send(In::Bus(thread_value)).is_err() {
        return ControlFlow::Break(());
    }

    ControlFlow::Continue(())
}

/// What the last read of 1 checkout found.
#[derive(Clone, Debug)]
struct Co {
    /// Its top dir, its common git dir (canonical: the watcher's paths).
    top: PathBuf,
    common: PathBuf,
    /// Its own git dir (a linked worktree: `<common>/worktrees/<name>`).
    own: PathBuf,
    /// Its task branch, None: PLAIN.
    branch: Option<String>,
    /// Its state with 0 task branch: 0 code, on main.
    plain: Land,
    /// The repo's default branch.
    def: Option<String>,
    dirty: usize,
    refs: Refs,
}

impl Co {
    /// Git topic part K of its repo changes what this checkout shows.
    fn hit(&self, key: &bus::Git) -> bool {
        let base = |bytes: &str| bytes == "main" || bytes == "master" || self.def.as_deref() == Some(bytes);

        match key {
            bus::Git::All => true,
            // the base moved: every branch's landed check; else only the checkouts on it
            bus::Git::Branch(bytes_value) | bus::Git::Remote(bytes_value) => {
                base(bytes_value) || self.branch.as_deref() == Some(bytes_value)
            }
            bus::Git::Worktree(count) if count.is_empty() => self.own == self.common,
            bus::Git::Worktree(count_value) => {
                self.own.file_name().is_some_and(|file| file == count_value.as_str()) && self.own != self.common
            }
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

#[cfg(test)]
#[path = "landed_tests.rs"]
mod tests;

#[path = "landed_repository.rs"]
mod landed_repository;
pub(crate) use landed_repository::*;

#[path = "landed_worker.rs"]
mod landed_worker;
