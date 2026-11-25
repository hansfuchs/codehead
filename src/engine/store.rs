//! Thread records under `<data>/threads/<id>/`:
//! - `thread.json`: title, task, dir, branch, members, harness session per agent, state.
//! - `events.jsonl`: 1 JSON event per line, append only. The host is the only writer; any
//!   process reads it (the view tails it by byte offset).

use std::collections::BTreeMap;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{data_dir, now_ms, write_atomic};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct Thread {
    pub id: String,
    pub title: String,
    /// The first message, as the owner wrote it.
    #[serde(default)]
    pub task: String,
    /// Where the agents work.
    pub dir: String,
    /// Linked git worktree codehead made for the thread.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    /// 1st port of the thread's block of 10 (worktree threads): $`CODEHEAD_PORT`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    /// The worktree's setup script: `pending` (the host runs it before the 1st turn), `done`,
    /// `failed`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub setup: Option<String>,
    /// A thread on main / master in an existing checkout: the host pulls (`git pull --ff-only`)
    /// before the 1st turn; false once done.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub pull: bool,
    /// A new thread's linked worktree on this branch: the host makes it (`git fetch`, `git
    /// worktree add`, local files, port) before the 1st turn; None once done.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree_pending: Option<String>,
    /// The task goes to the agents when the host starts (the UI never waits for a host): false
    /// once the host logged it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub kickoff: bool,
    /// The thread this one continues (`> handoff`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    /// A sub-thread (`>sub`): the thread it runs for. 0 history of it, same checkout; it reports
    /// to it and goes when its agents are done. Thread lists leave it out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub of: Option<String>,
    /// The running sub-threads of this thread (ids): its agents' messages wait while any runs.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub kids: Vec<String>,
    /// `open`, `done` or `archived` (0 worktree: `threads::archive` removed it, the branch stays;
    /// `worktree_pending` holds the branch for the next host start).
    #[serde(default = "open")]
    pub state: String,
    /// The thread's repo (main checkout), set by the owner (edit form) or by an archive: it wins
    /// over the repo of DIR / WORKTREE. None: the repo of DIR / WORKTREE.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo: Option<String>,
    /// Projects of the thread's repo (slugs, `projects.rs`): 0..n.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub projects: Vec<String>,
    #[serde(flatten)]
    pub owner_choices: OwnerChoices,
    pub created: u64,
    pub updated: u64,
    /// Agents on the thread, lead first: an owner message without @name goes to the lead.
    #[serde(default)]
    pub agents: Vec<String>,
    /// Harness session id per agent: the next session of the agent resumes it.
    #[serde(default)]
    pub sessions: BTreeMap<String, String>,
    /// Agents whose next session forks its resumed one (a branched thread): the original thread
    /// keeps its own.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fork: Vec<String>,
    /// Agents stopped on a usage limit: the time (unix ms) the host resumes each.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub resume: BTreeMap<String, u64>,
    /// More checkouts the thread works in (other repos, other worktrees): the agents' edits
    /// there add them. The 1st checkout: DIR / WORKTREE / BRANCH.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub checkouts: Vec<Checkout>,
    /// CHECKOUTS checked against the edits in the thread's events (`checkouts::prune`): older
    /// versions added a checkout per `cd DIR` / `git -C DIR`, reads too.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub checkouts_pruned: bool,
    /// The flow of each agent (`flows::run`): agent → its flow. Read it with `flow_map` /
    /// `flow_of`, write it with `set_flow`.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub flows: BTreeMap<String, super::flows::run::FlowState>,
    /// The thread's 1 flow of older codehead versions: the lead's. `set_flow` keeps it equal to
    /// the lead's entry of FLOWS (an older version reads it).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flow: Option<super::flows::run::FlowState>,
}

/// A checkout of a thread: its dir (a git toplevel), its branch when added.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Checkout {
    pub dir: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
}

fn open() -> String {
    "open".into()
}

pub(crate) fn threads_dir() -> PathBuf {
    data_dir().join("threads")
}

pub(crate) fn thread_dir(id: &str) -> PathBuf {
    // a pull request's review state (the merge tab's changes): beside the threads, never one
    match id.strip_prefix("pr~") {
        Some(key) => data_dir().join("prs").join(key),
        None => threads_dir().join(id),
    }
}

pub(crate) fn events_path(id: &str) -> PathBuf {
    thread_dir(id).join("events.jsonl")
}

impl Thread {
    pub(crate) fn new(title: &str, task: &str, dir: &Path, agents: Vec<String>) -> Thread {
        let now = now_ms();

        Thread {
            id: super::new_id(),
            title: title.to_string(),
            task: task.to_string(),
            dir: dir.display().to_string(),
            state: open(),
            created: now,
            updated: now,
            agents,
            ..Default::default()
        }
    }

    pub(crate) fn lead(&self) -> Option<&str> {
        self.agents.first().map(String::as_str)
    }

    /// The flow of each agent: FLOWS, + an older version's thread flow as the lead's.
    pub(crate) fn flow_map(&self) -> BTreeMap<String, super::flows::run::FlowState> {
        let mut flows = self.flows.clone();

        if let (Some(flow), Some(lead)) = (&self.flow, self.lead()) {
            flows.entry(lead.to_string()).or_insert_with(|| flow.clone());
        }

        flows
    }

    /// The flow of AGENT.
    pub(crate) fn flow_of(&self, agent: &str) -> Option<super::flows::run::FlowState> {
        self.flow_map().remove(agent)
    }

    /// Agent OLD is NEW now: its place, its session, its flow (an older thread's 1 flow: moved in
    /// first).
    pub(crate) fn rename_agent(&mut self, old: &str, new: &str) {
        let flow = self.flow_of(old);

        self.set_flow(old, None);

        for name in self.agents.iter_mut().filter(|agent| *agent == old) {
            *name = new.to_string();
        }

        if let Some(session) = self.sessions.remove(old) {
            self.sessions.insert(new.to_string(), session);
        }

        if flow.is_some() {
            self.set_flow(new, flow);
        }
    }

    /// AGENT's flow is FLOW (None: 0 flow). The old thread flow moves into FLOWS first.
    pub(crate) fn set_flow(&mut self, agent: &str, flow: Option<super::flows::run::FlowState>) {
        self.flows = self.flow_map();

        match flow {
            Some(flow) => self.flows.insert(agent.to_string(), flow),
            None => self.flows.remove(agent),
        };

        self.flow = self.lead().and_then(|lead| self.flows.get(lead)).cloned();
    }

    /// Where the agents work: the worktree, else the dir.
    pub(crate) fn work_dir(&self) -> PathBuf {
        PathBuf::from(self.worktree.as_deref().unwrap_or(&self.dir))
    }

    /// Every checkout of the thread: the work dir first, then the added ones (0 dupes).
    pub(crate) fn all_checkouts(&self) -> Vec<Checkout> {
        let mut checkouts = vec![Checkout {
            dir: self.work_dir().display().to_string(),
            branch: self.branch.clone(),
        }];

        for checkout in &self.checkouts {
            append_checkout(&mut checkouts, checkout);
        }

        checkouts
    }

    /// Add checkout CHECKOUT (a dir the thread has: 0 change). True: added.
    pub(crate) fn add_checkout(&mut self, checkout: Checkout) -> bool {
        if self.all_checkouts().iter().any(|known| known.dir == checkout.dir) {
            return false;
        }

        self.checkouts.push(checkout);
        true
    }

    pub(crate) fn save(&self) -> Result<(), String> {
        let path = thread_dir(&self.id).join("thread.json");
        let text = serde_json::to_vec_pretty(self).map_err(|error| error.to_string())?;

        write_atomic(&path, &text).map_err(|error| format!("{}: {error}", path.display()))
    }
}

fn append_checkout(checkouts: &mut Vec<Checkout>, checkout: &Checkout) {
    if !checkouts.iter().any(|known| known.dir == checkout.dir) {
        checkouts.push(checkout.clone());
    }
}

/// Thread ID of threads dir DIR (exact id, 0 prefix match).
pub(crate) fn load_in(dir: &Path, id: &str) -> Option<Thread> {
    serde_json::from_slice(&std::fs::read(dir.join(id).join("thread.json")).ok()?).ok()
}

/// Thread ID (full id, or a unique prefix).
pub(crate) fn load(id: &str) -> Option<Thread> {
    let exact = thread_dir(id).join("thread.json");

    let path = if exact.is_file() {
        exact
    } else {
        let mut hits = std::fs::read_dir(threads_dir())
            .ok()?
            .flatten()
            .filter(|entry| entry.file_name().to_string_lossy().starts_with(id))
            .map(|entry| entry.path().join("thread.json"));
        let first = hits.next()?;

        if hits.next().is_some() {
            return None;
        }

        first
    };

    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

/// Change thread ID: read, CHANGE, write, under the thread's `flock` (host, views and the CLI write
/// `thread.json`: 0 lost updates).
pub(crate) fn update(id: &str, change: impl FnOnce(&mut Thread)) -> Result<Thread, String> {
    let full = load(id).ok_or_else(|| format!("no thread {id}"))?.id;
    let lock = thread_dir(&full).join("thread.lock");

    super::locked(&lock, || {
        let mut thread = load(&full).ok_or_else(|| format!("no thread {id}"))?;

        change(&mut thread);
        thread.updated = now_ms();
        thread.save()?;
        Ok(thread)
    })
    .map_err(|error| format!("{}: {error}", lock.display()))?
}

/// The thread updated last, by its thread.json's time (1 parse, not 1 per thread).
pub(crate) fn latest() -> Option<Thread> {
    let rd = std::fs::read_dir(threads_dir()).ok()?;
    let mut by_time: Vec<(std::time::SystemTime, PathBuf)> = rd
        .flatten()
        .map(|entry| entry.path().join("thread.json"))
        .filter_map(|path| Some((std::fs::metadata(&path).ok()?.modified().ok()?, path)))
        .collect();

    by_time.sort_by_key(|(modified, _)| std::cmp::Reverse(*modified));

    // the newest that is no sub-thread (a running sub-thread writes its thread.json often), not
    // archived (an archive writes its thread.json last)
    by_time
        .iter()
        .filter_map(|(_, path)| serde_json::from_slice::<Thread>(&std::fs::read(path).ok()?).ok())
        .find(|thread| thread.of.is_none() && thread.state != "archived")
}

/// Every thread, newest activity first.
pub(crate) fn all() -> Vec<Thread> {
    all_in(&threads_dir())
}

/// `all` of the threads dir DIR (tests: a temp dir).
pub(crate) fn all_in(dir: &Path) -> Vec<Thread> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Vec::new();
    };

    let mut threads: Vec<Thread> = rd
        .flatten()
        .filter_map(|entry| serde_json::from_slice(&std::fs::read(entry.path().join("thread.json")).ok()?).ok())
        // sub-threads: shown in the thread they run for, 0 row of their own
        .filter(|thread: &Thread| thread.of.is_none())
        .collect();

    threads.sort_by_key(|thread: &Thread| std::cmp::Reverse(thread.updated));
    threads
}

/// A copy of thread ID: same members, dir and events; each agent's next session forks the
/// session it resumes. Returns the new thread.
pub(crate) fn fork(id: &str) -> Result<Thread, String> {
    let old = load(id).ok_or_else(|| format!("no thread {id}"))?;
    let now = now_ms();
    let thread = Thread {
        id: super::new_id(),
        title: format!("{} (branch)", old.title.lines().next().unwrap_or_default()),
        created: now,
        updated: now,
        state: open(),
        fork: old.sessions.keys().cloned().collect(),
        ..old.clone()
    };

    thread.save()?;
    let from = events_path(&old.id);

    if from.is_file() {
        std::fs::copy(&from, events_path(&thread.id)).map_err(|error| format!("{}: {error}", from.display()))?;
    }

    Ok(thread)
}

/// Remove thread ID with its events.
pub(crate) fn delete(id: &str) -> Result<(), String> {
    let dir = thread_dir(id);

    if !dir.join("thread.json").is_file() {
        return Err(format!("no thread {id}"));
    }

    std::fs::remove_dir_all(&dir).map_err(|error| format!("{}: {error}", dir.display()))
}

// ---------- events ----------

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Event {
    /// Milliseconds since the epoch.
    pub ts: u64,
    /// The agent the event is about (None: the owner or the thread).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    #[serde(flatten)]
    pub kind: Kind,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum Kind {
    /// A message into the thread: FROM `owner` or an agent, TO the agents it went to.
    Prompt {
        from: String,
        to: Vec<String>,
        text: String,
    },
    /// Agent text, 1 block (markdown).
    Text {
        text: String,
    },
    /// An agent thought (a reasoning summary), 1 block. Empty thinking: 0 events.
    Thought {
        text: String,
    },
    /// A tool call of the agent.
    Tool {
        id: String,
        name: String,
        input: Value,
    },
    /// The result of tool call ID (OUTPUT cut to 4 KB).
    ToolResult {
        id: String,
        ok: bool,
        output: String,
    },
    /// The agent waits on the owner: a permission request or a question (`AskUserQuestion`).
    Ask {
        id: String,
        tool: String,
        input: Value,
    },
    /// The owner's answer to ask ID.
    Answer {
        id: String,
        allow: bool,
        label: String,
    },
    /// The agent started a turn.
    TurnStart,
    /// The agent ended a turn: its cost in USD, tokens, context size, error if any.
    TurnEnd {
        cost: f64,
        input: u64,
        output: u64,
        cache_read: u64,
        cache_write: u64,
        /// Tokens in the context window after the turn.
        ctx: u64,
        /// Size of the context window in tokens (0: unknown, older events).
        #[serde(default, skip_serializing_if = "crate::domain::serialization::is_default")]
        window: u64,
        ms: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    /// A harness session started: its id, model.
    Session {
        session: String,
        model: String,
    },
    /// The agent reported its state (`working`, `blocked`, `review`, `done`) with the `set_status` tool.
    Status {
        state: String,
        note: String,
    },
    /// The agent joined / left the thread.
    Join,
    Leave,
    /// A session failed or ended without a turn end.
    Error {
        message: String,
    },
    /// The agent stopped on a usage limit: the host sends it `continue` at AT (unix ms), with
    /// codehead open or not. AT 0: the resume is off (it ran, or the owner wrote first).
    Resume {
        at: u64,
    },
    /// The agent got a note of edits in its worktree since its last turn, not its own: SUMMARY
    /// (1 line: who, files, line ranges).
    Edits {
        summary: String,
    },
    /// The owner ran COMMAND in the thread's worktree (shell mode, `!` in the prompt): its
    /// OUTPUT (stdout and stderr, cut to 16 KB), its exit CODE (-1: 0 code, a signal / timeout),
    /// the DIR it ran in (older events: none).
    Shell {
        command: String,
        output: String,
        code: i32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        dir: Option<String>,
    },
    /// The thread's flow moved: OP `set`, `step`, `wait`, `answer`, `skip`, `jump`, `switch`,
    /// `queued`, `pending`, `role`, `finished`, `clear`; TEXT the line the thread shows.
    Flow {
        op: String,
        text: String,
    },
    /// A subagent of the agent (its master) started: ID (the tool call that started it), PARENT
    /// (the subagent that started it; None: the master itself), what it does.
    SubStart {
        id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parent: Option<String>,
        description: String,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        agent_type: String,
        #[serde(default)]
        background: bool,
        /// Its task (cut to 2 KB): the master picks up from it when the subagent is lost.
        #[serde(default, skip_serializing_if = "String::is_empty")]
        prompt: String,
    },
    /// More about subagent ID: its transcript file (Claude Code), its task id (`TaskStop`).
    SubInfo {
        id: String,
        transcript: String,
        task: String,
    },
    /// Subagent ID ended: STATE `done`, `failed`, `stopped` or `lost`; RESULT its report (or why).
    SubEnd {
        id: String,
        state: String,
        result: String,
    },
    /// Subagent ID, ended, runs again (its master sent it a message: Claude Code `SendMessage`).
    /// A later `SubEnd` ends it again.
    SubResume {
        id: String,
    },
    /// A sub-thread of this thread (`>sub`): STATE `running` (started, TEXT: its task), `done` /
    /// `failed` (TEXT: its result), `held` (a message waits for the sub-threads to end).
    SubThread {
        id: String,
        title: String,
        state: String,
        text: String,
    },
}

impl Event {
    pub(crate) fn new(agent: Option<&str>, kind: Kind) -> Event {
        Event {
            ts: now_ms(),
            agent: agent.map(String::from),
            kind,
        }
    }
}

/// Reads new events of a thread from a byte offset. A line without its `\n` yet waits.
pub(crate) struct Tail {
    path: PathBuf,
    offset: u64,
    partial: Vec<u8>,
}

#[cfg(test)]
#[path = "store_tests.rs"]
mod tests;

#[path = "store_events.rs"]
mod store_events;
pub(crate) use store_events::*;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct OwnerChoices {
    /// The owner picked PROJECTS (new form, edit form): the agents' `set_projects` leaves them.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub projects_set: bool,
    /// The owner wrote the title (new form, edit form): the agents' `set_title` leaves it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub title_set: bool,
}
