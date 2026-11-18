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
pub struct Thread {
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
    /// `open` or `done`.
    #[serde(default = "open")]
    pub state: String,
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
}

fn open() -> String {
    "open".into()
}

pub fn threads_dir() -> PathBuf {
    data_dir().join("threads")
}

pub fn thread_dir(id: &str) -> PathBuf {
    threads_dir().join(id)
}

pub fn events_path(id: &str) -> PathBuf {
    thread_dir(id).join("events.jsonl")
}

impl Thread {
    pub fn new(title: &str, task: &str, dir: &Path, agents: Vec<String>) -> Thread {
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

    pub fn lead(&self) -> Option<&str> {
        self.agents.first().map(String::as_str)
    }

    /// Where the agents work: the worktree, else the dir.
    pub fn work_dir(&self) -> PathBuf {
        PathBuf::from(self.worktree.as_deref().unwrap_or(&self.dir))
    }

    pub fn save(&self) -> Result<(), String> {
        let path = thread_dir(&self.id).join("thread.json");
        let text = serde_json::to_vec_pretty(self).map_err(|e| e.to_string())?;
        write_atomic(&path, &text).map_err(|e| format!("{}: {e}", path.display()))
    }
}

/// Thread ID (full id, or a unique prefix).
pub fn load(id: &str) -> Option<Thread> {
    let exact = thread_dir(id).join("thread.json");
    let path = if exact.is_file() {
        exact
    } else {
        let mut hits = std::fs::read_dir(threads_dir())
            .ok()?
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with(id))
            .map(|e| e.path().join("thread.json"));
        let first = hits.next()?;
        if hits.next().is_some() {
            return None;
        }
        first
    };
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

/// Change thread ID: read, F, write. Last writer wins (the host and the view rarely touch the
/// same fields at once).
pub fn update(id: &str, f: impl FnOnce(&mut Thread)) -> Result<Thread, String> {
    let mut t = load(id).ok_or_else(|| format!("no thread {id}"))?;
    f(&mut t);
    t.updated = now_ms();
    t.save()?;
    Ok(t)
}

/// Every thread, newest activity first.
pub fn all() -> Vec<Thread> {
    let Ok(rd) = std::fs::read_dir(threads_dir()) else { return Vec::new() };
    let mut v: Vec<Thread> = rd
        .flatten()
        .filter_map(|e| serde_json::from_slice(&std::fs::read(e.path().join("thread.json")).ok()?).ok())
        .collect();
    v.sort_by_key(|t: &Thread| std::cmp::Reverse(t.updated));
    v
}

/// A copy of thread ID: same members, dir and events; each agent's next session forks the
/// session it resumes. Returns the new thread.
pub fn fork(id: &str) -> Result<Thread, String> {
    let old = load(id).ok_or_else(|| format!("no thread {id}"))?;
    let now = now_ms();
    let t = Thread {
        id: super::new_id(),
        title: format!("{} (branch)", old.title.lines().next().unwrap_or_default()),
        created: now,
        updated: now,
        state: open(),
        fork: old.sessions.keys().cloned().collect(),
        ..old.clone()
    };
    t.save()?;
    let from = events_path(&old.id);
    if from.is_file() {
        std::fs::copy(&from, events_path(&t.id)).map_err(|e| format!("{}: {e}", from.display()))?;
    }
    Ok(t)
}

/// Remove thread ID with its events.
pub fn delete(id: &str) -> Result<(), String> {
    let dir = thread_dir(id);
    if !dir.join("thread.json").is_file() {
        return Err(format!("no thread {id}"));
    }
    std::fs::remove_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))
}

// ---------- events ----------

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Event {
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
pub enum Kind {
    /// A message into the thread: FROM `owner` or an agent, TO the agents it went to.
    Prompt { from: String, to: Vec<String>, text: String },
    /// Agent text, 1 block (markdown).
    Text { text: String },
    /// A tool call of the agent.
    Tool { id: String, name: String, input: Value },
    /// The result of tool call ID (OUTPUT cut to 4 KB).
    ToolResult { id: String, ok: bool, output: String },
    /// The agent waits on the owner: a permission request or a question (AskUserQuestion).
    Ask { id: String, tool: String, input: Value },
    /// The owner's answer to ask ID.
    Answer { id: String, allow: bool, label: String },
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
        ms: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    /// A harness session started: its id, model.
    Session { session: String, model: String },
    /// The agent reported its state (`working`, `blocked`, `done`) with the `set_status` tool.
    Status { state: String, note: String },
    /// The agent joined / left the thread.
    Join,
    Leave,
    /// A session failed or ended without a turn end.
    Error { message: String },
}

impl Event {
    pub fn new(agent: Option<&str>, kind: Kind) -> Event {
        Event { ts: now_ms(), agent: agent.map(String::from), kind }
    }
}

/// Append E to thread ID's log: 1 `write` per line (O_APPEND: lines never interleave).
pub fn append(id: &str, e: &Event) -> Result<(), String> {
    let path = events_path(id);
    let mut line = serde_json::to_vec(e).map_err(|e| e.to_string())?;
    line.push(b'\n');
    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    f.write_all(&line).map_err(|e| format!("{}: {e}", path.display()))
}

/// Every event of thread ID.
pub fn events(id: &str) -> Vec<Event> {
    let mut t = Tail::new(id);
    t.read()
}

/// Reads new events of a thread from a byte offset. A line without its `\n` yet waits.
pub struct Tail {
    path: PathBuf,
    offset: u64,
    partial: Vec<u8>,
}

impl Tail {
    pub fn new(id: &str) -> Tail {
        Tail { path: events_path(id), offset: 0, partial: Vec::new() }
    }

    /// Events written since the last read.
    pub fn read(&mut self) -> Vec<Event> {
        let Ok(mut f) = std::fs::File::open(&self.path) else { return Vec::new() };
        if f.seek(SeekFrom::Start(self.offset)).is_err() {
            return Vec::new();
        }
        let mut buf = Vec::new();
        if f.read_to_end(&mut buf).is_err() {
            return Vec::new();
        }
        self.offset += buf.len() as u64;
        self.partial.extend_from_slice(&buf);
        let Some(end) = self.partial.iter().rposition(|&b| b == b'\n') else { return Vec::new() };
        let rest = self.partial.split_off(end + 1);
        let done = std::mem::replace(&mut self.partial, rest);
        done.split(|&b| b == b'\n').filter(|l| !l.is_empty()).filter_map(|l| serde_json::from_slice(l).ok()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_round_trip_and_tail_reads_only_new_lines() {
        let home = std::env::temp_dir().join(format!("codehead-store-{}", super::super::new_id()));
        // SAFETY: test-local env; the tests of this module do not run other env readers.
        unsafe { std::env::set_var("CODEHEAD_HOME", &home) };
        let t = Thread::new("t", "do it", Path::new("/tmp"), vec!["ada".into()]);
        t.save().unwrap();
        let mut tail = Tail::new(&t.id);
        append(&t.id, &Event::new(None, Kind::Prompt { from: "owner".into(), to: vec!["ada".into()], text: "hi".into() })).unwrap();
        append(&t.id, &Event::new(Some("ada"), Kind::Text { text: "hello".into() })).unwrap();
        assert_eq!(tail.read().len(), 2);
        assert!(tail.read().is_empty());
        append(&t.id, &Event::new(Some("ada"), Kind::TurnStart)).unwrap();
        let e = tail.read();
        assert_eq!(e.len(), 1);
        assert_eq!(e[0].kind, Kind::TurnStart);
        assert_eq!(load(&t.id[..10]).unwrap().agents, vec!["ada".to_string()]);
        let _ = std::fs::remove_dir_all(home);
    }
}
