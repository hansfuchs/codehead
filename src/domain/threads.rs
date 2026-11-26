//! Threads tab rows: every thread (in a worktree or in place), then worktrees of the repo without a thread.
//! Delete = thread + (linked worktree only) worktree + safe branch.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::engine::{client, store};

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct Thread {
    pub id: String,
    pub title: String,
    pub state: String,
    pub live: usize,
    pub updated: u64,
    /// worktree path of the thread, if it has one
    pub worktree: Option<String>,
    pub branch: Option<String>,
    /// project dir (`path_hint`) of a thread that works in place
    pub dir: Option<String>,
    /// An agent works now.
    #[serde(default)]
    pub busy: bool,
    /// BUSY only by a monitor or background command of a live agent (0 turn, 0 sub-thread).
    #[serde(default)]
    pub watching: bool,
    /// Why the owner is needed: `question`, `error: …`, `setup failed`.
    #[serde(default)]
    pub needs: Option<String>,
    /// Changes against the base: files, lines added, removed.
    #[serde(default)]
    pub stat: Option<(usize, usize, usize)>,
    #[serde(default)]
    pub agents: Vec<String>,
    /// Last status per agent: (agent, state, note).
    #[serde(default)]
    pub notes: Vec<(String, String, String)>,
    #[serde(default)]
    pub task: String,
    #[serde(default)]
    pub port: Option<u16>,
    #[serde(default)]
    pub setup: Option<String>,
    #[serde(default)]
    pub parent: Option<String>,
    /// portless: the worktree's live URLs.
    #[serde(default)]
    pub urls: Vec<String>,
    /// The owner's repo pick (`store::Thread::repo`): wins over DIR / WORKTREE.
    #[serde(default)]
    pub repo: Option<String>,
    /// Projects (`store::Thread::projects`).
    #[serde(default)]
    pub projects: Vec<String>,
}

/// From the tail of thread ID's log: an open question, the last error (a turn that ended in
/// one, after the last owner message), the last status per agent.
fn activity(id: &str) -> (bool, Option<String>, Vec<(String, String, String)>) {
    use crate::engine::store::Kind;
    let mut open: Vec<(String, String)> = Vec::new();
    let mut error: Option<String> = None;
    let mut notes: Vec<(String, String, String)> = Vec::new();

    // the last 400 events: the last 512 KiB of the log hold them (0 full read of a 10 MB log)
    let tail = crate::engine::store::events_tail(id, 512 << 10);

    for event in tail.into_iter().rev().take(400).collect::<Vec<_>>().into_iter().rev() {
        let agent = event.agent.clone().unwrap_or_default();

        match event.kind {
            Kind::Ask { id, .. } => open.push((agent, id)),
            Kind::Answer { id, .. } => open.retain(|ask| ask.1 != id),
            Kind::TurnEnd { error: err, .. } => {
                record_turn_error(&mut open, &mut error, &agent, err);
            }
            Kind::Error { message } => {
                error = Some(format!("{agent}: {}", message.lines().next().unwrap_or_default()));
            }
            Kind::Prompt { from, .. } if from == "owner" => error = None,
            Kind::Status { state, note } => {
                notes.retain(|note| note.0 != agent);
                notes.push((agent, state, note));
            }
            _ => {}
        }
    }

    (!open.is_empty(), error, notes)
}

fn record_turn_error(
    open: &mut Vec<(String, String)>,
    error: &mut Option<String>,
    agent: &String,
    err: Option<String>,
) {
    open.retain(|ask| ask.0 != *agent);
    *error = err
        .filter(|message| message != crate::engine::host::INTERRUPTED && message != "session ended")
        .map(|message| format!("{agent}: {}", message.lines().next().unwrap_or_default()));
}

/// Changes of DIR against its merge-base with origin/<default> (else HEAD), untracked files
/// counted as files: (files, added, removed). None: 0 git.
fn shortstat(dir: &Path) -> Option<(usize, usize, usize)> {
    if !git(dir, &["rev-parse", "--is-inside-work-tree"]).0 {
        return None;
    }

    let def = ["origin/HEAD", "origin/main", "origin/master"]
        .into_iter()
        .find(|rev| git(dir, &["rev-parse", "--verify", "--quiet", rev]).0);

    let base = def
        .and_then(|def| {
            Some(git(dir, &["merge-base", "HEAD", def]).1.trim().to_string()).filter(|base| !base.is_empty())
        })
        .unwrap_or_else(|| "HEAD".into());

    // plumbing: `git diff` writes the refreshed index, the watcher's topic reads every view again
    let stat = git(dir, &["diff-index", "--shortstat", &base]).1;

    let num = |key: &str| {
        stat.split(',')
            .find(|part| part.contains(key))
            .and_then(|part| part.split_whitespace().next())
            .and_then(|number| number.parse::<usize>().ok())
            .unwrap_or(0)
    };

    let untracked = git(dir, &["ls-files", "--others", "--exclude-standard"])
        .1
        .lines()
        .count();

    Some((num("file") + untracked, num("insertion"), num("deletion")))
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct Worktree {
    pub path: String,
    pub branch: String,
    pub thread: Option<Thread>,
    pub unpushed: usize,
    pub when: String,
    /// A linked git worktree (delete may remove it). False: thread works in place.
    pub linked: bool,
    /// Main checkout of the row's git repo (repo filter). None: outside any repo.
    #[serde(default)]
    pub repo: Option<PathBuf>,
}

impl Worktree {
    /// Selection key: thread id, else path (in-place threads share paths).
    pub(crate) fn key(&self) -> String {
        self.thread
            .as_ref()
            .map_or_else(|| self.path.clone(), |thread| thread.id.clone())
    }

    /// Name for the list: branch of a worktree, dir name of an in-place thread.
    pub(crate) fn label(&self) -> String {
        if self.linked {
            return self.branch.clone();
        }

        Path::new(&self.path)
            .file_name()
            .map_or_else(|| "scratch".into(), |name| name.to_string_lossy().into_owned())
    }
}

/// Every thread, newest first, with git stats and live URLs (threads tab).
pub(crate) fn all_threads() -> Vec<Thread> {
    scan(true)
}

/// Every thread, newest first, 0 git stats, 0 URLs: thread header, mention menu, boot.
pub(crate) fn all_threads_lite() -> Vec<Thread> {
    scan(false)
}

/// F over ITEMS on up to 8 OS threads, order kept.
pub(crate) fn par_map<T: Sync, R: Send>(items: &[T], apply: impl Fn(&T) -> R + Sync) -> Vec<R> {
    let workers = std::thread::available_parallelism()
        .map_or(4, std::num::NonZero::get)
        .clamp(1, 8);

    if items.len() < 2 || workers == 1 {
        return items.iter().map(&apply).collect();
    }

    let size = items.len().div_ceil(workers);

    std::thread::scope(|scope| {
        let parts: Vec<_> = items
            .chunks(size)
            .map(|chunk| scope.spawn(|| chunk.iter().map(&apply).collect::<Vec<R>>()))
            .collect();

        parts
            .into_iter()
            .flat_map(|handle| handle.join().unwrap_or_default())
            .collect()
    })
}

fn scan(full: bool) -> Vec<Thread> {
    // portless: 1 read of the live routes for every thread
    let routes = if full && crate::infra::portless::active() {
        crate::infra::portless::routes()
    } else {
        Vec::new()
    };

    let list = crate::engine::store::all();

    // git stats: once per work dir (threads share dirs), archived threads skipped
    let stats: std::collections::HashMap<PathBuf, Option<(usize, usize, usize)>> = if full {
        let mut dirs: Vec<PathBuf> = list
            .iter()
            .filter(|thread| thread.state != "archived" && thread.state != "deleted")
            .map(crate::engine::store::Thread::work_dir)
            .collect();

        dirs.sort();
        dirs.dedup();
        let results = par_map(&dirs, |dir| shortstat(dir));

        dirs.into_iter().zip(results).collect()
    } else {
        std::collections::HashMap::default()
    };

    par_map(&list, |thread| scan_thread(&routes, &stats, thread))
}

fn scan_thread(
    routes: &[(String, String)],
    stats: &std::collections::HashMap<PathBuf, Option<(usize, usize, usize)>>,
    stored: &store::Thread,
) -> Thread {
    let thread = stored.clone();
    let alive = crate::engine::client::alive(&thread.id);

    let (live, busy) = if alive {
        crate::engine::client::state(&thread.id)
    } else {
        (Vec::new(), Vec::new())
    };

    // work goes on without a turn: a running sub-thread, a monitor or background command
    let turn = !busy.is_empty() || !thread.kids.is_empty();
    let watching = !turn && alive && crate::domain::monitors::watching(&thread.id, &live);
    let busy = turn || watching;
    let (ask, error, notes) = activity(&thread.id);

    let needs = if ask {
        Some("question".to_string())
    } else if thread.setup.as_deref() == Some("failed") {
        Some("setup failed".to_string())
    } else {
        error.map(|error| format!("error: {error}"))
    };

    Thread {
        live: live.len(),
        busy,
        watching,
        needs,
        stat: stats.get(&thread.work_dir()).copied().flatten(),
        agents: thread.agents.clone(),
        notes,
        task: thread.task.clone(),
        port: thread.port,
        setup: thread.setup.clone(),
        parent: thread.parent.clone(),
        urls: thread
            .worktree
            .as_ref()
            .and(thread.branch.as_deref())
            .map(|branch| crate::infra::portless::urls_of(routes, &crate::infra::portless::prefix(branch)))
            .unwrap_or_default(),
        updated: thread.updated / 1000,
        repo: thread.repo.clone(),
        projects: thread.projects.clone(),
        worktree: thread.worktree.clone(),
        branch: thread.branch.clone(),
        dir: Some(thread.dir.clone()),
        title: thread.title,
        state: thread.state,
        id: thread.id,
    }
}

fn when_of(path: &Path) -> String {
    let (_, ct, _) = git(path, &["log", "-1", "--format=%ct"]);

    ct.trim().parse::<u64>().map_or_else(
        |_| "?".into(),
        |secs| crate::domain::notes::ago(std::time::UNIX_EPOCH + std::time::Duration::from_secs(secs)),
    )
}

/// Rows: every thread (newest first), then worktrees of REPO without a thread.
pub(crate) fn load(repo: Option<&Path>) -> Vec<Worktree> {
    let wts = repo.map(worktrees).unwrap_or_default();
    let mut used = std::collections::HashSet::new();
    let mut out = Vec::new();

    for thread in all_threads() {
        let wt = thread
            .worktree
            .as_ref()
            .and_then(|worktree| wts.iter().find(|(wp, _)| real(wp) == real(worktree)));
        let rp = repo_of_thread(&thread);

        let row = match (wt, &thread.worktree) {
            (Some((path, branch)), _) => {
                used.insert(path.clone());
                let dir = Path::new(path);

                Worktree {
                    unpushed: unpushed(branch, dir),
                    when: when_of(dir),
                    path: path.clone(),
                    branch: branch.clone(),
                    linked: true,
                    thread: Some(thread),
                    repo: rp,
                }
            }
            // worktree of another repo
            (None, Some(path)) if Path::new(path).exists() => {
                let branch = thread.branch.clone().unwrap_or_else(|| "(detached)".into());
                let dir = Path::new(path);

                Worktree {
                    unpushed: unpushed(&branch, dir),
                    when: when_of(dir),
                    path: path.clone(),
                    branch,
                    linked: true,
                    thread: Some(thread),
                    repo: rp,
                }
            }
            _ => in_place_row(thread, rp),
        };

        out.push(row);
    }

    for (path, branch) in wts {
        if used.contains(&path) {
            continue;
        }

        let dir = Path::new(&path);

        out.push(Worktree {
            thread: None,
            unpushed: unpushed(&branch, dir),
            when: when_of(dir),
            path,
            branch,
            linked: true,
            repo: repo.map(Path::to_path_buf),
        });
    }

    out
}

fn in_place_row(thread: Thread, rp: Option<PathBuf>) -> Worktree {
    let when = crate::domain::notes::ago(std::time::UNIX_EPOCH + std::time::Duration::from_secs(thread.updated));

    Worktree {
        path: thread.dir.clone().unwrap_or_default(),
        branch: "in place".into(),
        unpushed: 0,
        when,
        linked: false,
        thread: Some(thread),
        repo: rp,
    }
}

/// The owner's edit of a thread: title, repo (None: the repo of its dirs), linked repos (the
/// checkouts kept), agents (lead first).
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Edit {
    pub title: String,
    pub repo: Option<String>,
    pub checkouts: Vec<String>,
    pub agents: Vec<String>,
    /// The owner's projects (slugs); None: auto (the agents assign them).
    pub projects: Option<Vec<String>>,
}

#[cfg(test)]
#[path = "threads_tests.rs"]
mod tests;

#[path = "thread_repository.rs"]
mod thread_repository;
pub(crate) use thread_repository::*;

#[path = "thread_actions.rs"]
mod thread_actions;
pub(crate) use thread_actions::*;
