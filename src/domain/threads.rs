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

    for error_value in tail.into_iter().rev().take(400).collect::<Vec<_>>().into_iter().rev() {
        let agent_value = error_value.agent.clone().unwrap_or_default();

        match error_value.kind {
            Kind::Ask { id, .. } => open.push((agent_value, id)),
            Kind::Answer { id, .. } => open.retain(|x| x.1 != id),
            Kind::TurnEnd { error: err, .. } => {
                record_turn_error(&mut open, &mut error, &agent_value, err);
            }
            Kind::Error { message } => {
                error = Some(format!("{agent_value}: {}", message.lines().next().unwrap_or_default()));
            }
            Kind::Prompt { from, .. } if from == "owner" => error = None,
            Kind::Status { state, note } => {
                notes.retain(|x| x.0 != agent_value);
                notes.push((agent_value, state, note));
            }
            _ => {}
        }
    }

    (!open.is_empty(), error, notes)
}

fn record_turn_error(
    open: &mut Vec<(String, String)>,
    error: &mut Option<String>,
    agent_value: &String,
    err: Option<String>,
) {
    open.retain(|x| x.0 != *agent_value);
    *error = err
        .filter(|message_value| message_value != crate::engine::host::INTERRUPTED && message_value != "session ended")
        .map(|message_2| format!("{agent_value}: {}", message_2.lines().next().unwrap_or_default()));
}

/// Changes of DIR against its merge-base with origin/<default> (else HEAD), untracked files
/// counted as files: (files, added, removed). None: 0 git.
fn shortstat(dir: &Path) -> Option<(usize, usize, usize)> {
    if !git(dir, &["rev-parse", "--is-inside-work-tree"]).0 {
        return None;
    }

    let def = ["origin/HEAD", "origin/main", "origin/master"]
        .into_iter()
        .find(|result| git(dir, &["rev-parse", "--verify", "--quiet", result]).0);

    let base = def
        .and_then(|directory| {
            Some(git(dir, &["merge-base", "HEAD", directory]).1.trim().to_string()).filter(|text| !text.is_empty())
        })
        .unwrap_or_else(|| "HEAD".into());

    let text_value = git(dir, &["diff", "--shortstat", &base]).1;

    let num = |key: &str| {
        text_value
            .split(',')
            .find(|path| path.contains(key))
            .and_then(|path_value| path_value.split_whitespace().next())
            .and_then(|count_value| count_value.parse::<usize>().ok())
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
            .map_or_else(|| self.path.clone(), |thread_value| thread_value.id.clone())
    }

    /// Name for the list: branch of a worktree, dir name of an in-place thread.
    pub(crate) fn label(&self) -> String {
        if self.linked {
            return self.branch.clone();
        }

        Path::new(&self.path)
            .file_name()
            .map_or_else(|| "scratch".into(), |count| count.to_string_lossy().into_owned())
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
pub(crate) fn par_map<T: Sync, R: Send>(items: &[T], file: impl Fn(&T) -> R + Sync) -> Vec<R> {
    let count = std::thread::available_parallelism()
        .map_or(4, std::num::NonZero::get)
        .clamp(1, 8);

    if items.len() < 2 || count == 1 {
        return items.iter().map(&file).collect();
    }

    let size = items.len().div_ceil(count);

    std::thread::scope(|text| {
        let parts: Vec<_> = items
            .chunks(size)
            .map(|character| text.spawn(|| character.iter().map(&file).collect::<Vec<R>>()))
            .collect();

        parts
            .into_iter()
            .flat_map(|height| height.join().unwrap_or_default())
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
        let text = par_map(&dirs, |directory| shortstat(directory));

        dirs.into_iter().zip(text).collect()
    } else {
        std::collections::HashMap::default()
    };

    par_map(&list, |thread_value| scan_thread(&routes, &stats, thread_value))
}

fn scan_thread(
    routes: &[(String, String)],
    stats: &std::collections::HashMap<PathBuf, Option<(usize, usize, usize)>>,
    thread_value: &store::Thread,
) -> Thread {
    let thread_2 = thread_value.clone();
    let alive = crate::engine::client::alive(&thread_2.id);

    let (live, busy) = if alive {
        crate::engine::client::state(&thread_2.id)
    } else {
        (Vec::new(), Vec::new())
    };

    // work goes on without a turn: a running sub-thread, a monitor or background command
    let turn = !busy.is_empty() || !thread_2.kids.is_empty();
    let watching = !turn && alive && crate::domain::monitors::watching(&thread_2.id, &live);
    let busy = turn || watching;
    let (ask, error, notes) = activity(&thread_2.id);

    let needs = if ask {
        Some("question".to_string())
    } else if thread_2.setup.as_deref() == Some("failed") {
        Some("setup failed".to_string())
    } else {
        error.map(|error_value| format!("error: {error_value}"))
    };

    Thread {
        live: live.len(),
        busy,
        watching,
        needs,
        stat: stats.get(&thread_2.work_dir()).copied().flatten(),
        agents: thread_2.agents.clone(),
        notes,
        task: thread_2.task.clone(),
        port: thread_2.port,
        setup: thread_2.setup.clone(),
        parent: thread_2.parent.clone(),
        urls: thread_2
            .worktree
            .as_ref()
            .and(thread_2.branch.as_deref())
            .map(|bytes| crate::infra::portless::urls_of(routes, &crate::infra::portless::prefix(bytes)))
            .unwrap_or_default(),
        updated: thread_2.updated / 1000,
        repo: thread_2.repo.clone(),
        projects: thread_2.projects.clone(),
        worktree: thread_2.worktree.clone(),
        branch: thread_2.branch.clone(),
        dir: Some(thread_2.dir.clone()),
        title: thread_2.title,
        state: thread_2.state,
        id: thread_2.id,
    }
}

fn when_of(path: &Path) -> String {
    let (_, ct, _) = git(path, &["log", "-1", "--format=%ct"]);

    ct.trim().parse::<u64>().map_or_else(
        |_| "?".into(),
        |text| crate::domain::notes::ago(std::time::UNIX_EPOCH + std::time::Duration::from_secs(text)),
    )
}

/// Rows: every thread (newest first), then worktrees of REPO without a thread.
pub(crate) fn load(repo: Option<&Path>) -> Vec<Worktree> {
    let wts = repo.map(worktrees).unwrap_or_default();
    let mut used = std::collections::HashSet::new();
    let mut out = Vec::new();

    for thread_value in all_threads() {
        let wt = thread_value
            .worktree
            .as_ref()
            .and_then(|path_value| wts.iter().find(|(wp, _)| real(wp) == real(path_value)));
        let rp = repo_of_thread(&thread_value);

        let row = match (wt, &thread_value.worktree) {
            (Some((path, branch)), _) => {
                used.insert(path.clone());
                let path_2 = Path::new(path);

                Worktree {
                    unpushed: unpushed(branch, path_2),
                    when: when_of(path_2),
                    path: path.clone(),
                    branch: branch.clone(),
                    linked: true,
                    thread: Some(thread_value),
                    repo: rp,
                }
            }
            // worktree of another repo
            (None, Some(path)) if Path::new(path).exists() => {
                let branch = thread_value.branch.clone().unwrap_or_else(|| "(detached)".into());
                let path_3 = Path::new(path);

                Worktree {
                    unpushed: unpushed(&branch, path_3),
                    when: when_of(path_3),
                    path: path.clone(),
                    branch,
                    linked: true,
                    thread: Some(thread_value),
                    repo: rp,
                }
            }
            _ => load_match(thread_value, rp),
        };

        out.push(row);
    }

    for (path, branch) in wts {
        if used.contains(&path) {
            continue;
        }

        let path_4 = Path::new(&path);

        out.push(Worktree {
            thread: None,
            unpushed: unpushed(&branch, path_4),
            when: when_of(path_4),
            path,
            branch,
            linked: true,
            repo: repo.map(Path::to_path_buf),
        });
    }

    out
}

fn load_match(thread_value: Thread, rp: Option<PathBuf>) -> Worktree {
    let when = crate::domain::notes::ago(std::time::UNIX_EPOCH + std::time::Duration::from_secs(thread_value.updated));

    Worktree {
        path: thread_value.dir.clone().unwrap_or_default(),
        branch: "in place".into(),
        unpushed: 0,
        when,
        linked: false,
        thread: Some(thread_value),
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
