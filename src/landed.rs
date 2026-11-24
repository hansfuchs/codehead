//! Landed state of each thread: work not yet in the base branch (open: uncommitted, unpushed,
//! unmerged, PR open) or 0 work left (shipped: landed, abandoned, 0 code). The thread lists put
//! open threads and shipped threads in 2 sections; shipped ones are ready to archive.
//!
//! 1 state per checkout, the worst wins. A checkout on the default branch (main / master) and a
//! dir without git count as 0 code: the owner handles main. A branch landed when its content is
//! in `origin/<default>` or the local `<default>` (a local merge): ancestor, same patches
//! (`git cherry`: rebase merges), or a merge that changes 0 (`git merge-tree`: squash merges).
//! A background pass reads every thread (archived ones: their kept branch, 5 min), `git fetch`
//! per repo every 5 min.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use crate::engine::store;
use crate::gh::State;
use crate::threads::git;

/// Time between 2 passes over the active threads.
const PASS_EVERY: Duration = Duration::from_secs(30);
/// Time between 2 fetches per repo, 2 passes over the archived threads.
const FETCH_EVERY: Duration = Duration::from_secs(300);
/// A fetch that takes longer: stopped.
const FETCH_MAX: Duration = Duration::from_secs(60);

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
    /// 0 task branch: 0 git, or the default branch.
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

/// The PRs of thread ID (the PR pass).
pub fn set_prs(id: &str, prs: Vec<PrSeen>) {
    if let Ok(mut m) = PRS.lock() {
        m.get_or_insert_with(HashMap::new).insert(id.to_string(), prs);
    }
}

fn prs_of(id: &str) -> Vec<PrSeen> {
    PRS.lock().ok().and_then(|m| m.as_ref()?.get(id).cloned()).unwrap_or_default()
}

fn set(id: &str, land: Land) {
    let Ok(mut m) = LANDS.lock() else { return };
    m.get_or_insert_with(HashMap::new).insert(id.to_string(), land);
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
    if !dir.is_dir() || !git(dir, &["rev-parse", "--git-dir"]).0 {
        return Land::NoCode;
    }
    let b = branch.map(String::from).unwrap_or_else(|| crate::threads::current_branch(dir));
    let def = default_branch(dir);
    if b.is_empty() || Some(&b) == def.as_ref() || b == "main" || b == "master" {
        return Land::NoCode;
    }
    if dirty {
        let n = git(dir, &["status", "--porcelain"]).1.lines().count();
        if n > 0 {
            return Land::Dirty(n);
        }
    }
    let Some(rev) = oid(dir, &format!("refs/heads/{b}")) else { return Land::NoCode };
    let bases: Vec<String> = def.iter().flat_map(|d| [format!("origin/{d}"), d.clone()]).filter_map(|r| oid(dir, &r)).collect();
    if bases.iter().any(|base| contained(dir, &rev, base, seen)) {
        return Land::Landed;
    }
    let mine: Vec<&PrSeen> = prs.iter().filter(|p| p.head == b).collect();
    if mine.iter().any(|p| p.state == State::Merged && p.head_oid == rev) {
        return Land::Landed;
    }
    let unpushed = git(dir, &["rev-list", "--count", &rev, "--not", "--remotes"]).1.trim().parse().unwrap_or(0);
    if unpushed > 0 {
        return Land::Unpushed(unpushed);
    }
    if let Some(p) = mine.iter().find(|p| matches!(p.state, State::Open | State::Draft)) {
        return Land::PrOpen(p.label.clone());
    }
    let closed = mine.iter().find(|p| p.state == State::Closed && p.head_oid == rev);
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

/// Start the background passes (once per process).
pub fn start() {
    static ON: OnceLock<()> = OnceLock::new();
    if cfg!(test) || ON.set(()).is_err() {
        return;
    }
    std::thread::spawn(|| {
        let mut seen = Seen::new();
        let mut slow_at: Option<Instant> = None;
        loop {
            let slow = slow_at.is_none_or(|t| t.elapsed() >= FETCH_EVERY);
            let threads: Vec<store::Thread> = store::all().into_iter().filter(|t| t.state != "deleted" && (slow || t.state != "archived")).collect();
            if slow {
                slow_at = Some(Instant::now());
                let repos: HashSet<PathBuf> = threads.iter().filter(|t| t.state != "archived").filter_map(crate::threads::repo_of_thread_store).collect();
                for r in repos.iter().filter(|r| oid(r, "origin/HEAD").is_some() || git(r, &["remote", "get-url", "origin"]).0) {
                    fetch(r);
                }
            }
            for t in &threads {
                let land = thread(t, &prs_of(&t.id), &mut seen);
                set(&t.id, land);
            }
            // oids fix the answers: only the size needs a cap
            if seen.len() > 4096 {
                seen.clear();
            }
            std::thread::sleep(PASS_EVERY);
        }
    });
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
}
