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
    /// 1st port of the thread's block of 10 (worktree threads): $CODEHEAD_PORT.
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
    /// Agents stopped on a usage limit: the time (unix ms) the host resumes each.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub resume: BTreeMap<String, u64>,
    /// More checkouts the thread works in (other repos, other worktrees): the agents' edits
    /// there add them. The 1st checkout: DIR / WORKTREE / BRANCH.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub checkouts: Vec<Checkout>,
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
pub struct Checkout {
    pub dir: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
}

fn open() -> String {
    "open".into()
}

fn is_zero(v: &u64) -> bool {
    *v == 0
}

pub fn threads_dir() -> PathBuf {
    data_dir().join("threads")
}

pub fn thread_dir(id: &str) -> PathBuf {
    // a pull request's review state (the merge tab's changes): beside the threads, never one
    match id.strip_prefix("pr~") {
        Some(k) => data_dir().join("prs").join(k),
        None => threads_dir().join(id),
    }
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

    /// The flow of each agent: FLOWS, + an older version's thread flow as the lead's.
    pub fn flow_map(&self) -> BTreeMap<String, super::flows::run::FlowState> {
        let mut m = self.flows.clone();
        if let (Some(f), Some(l)) = (&self.flow, self.lead()) {
            m.entry(l.to_string()).or_insert_with(|| f.clone());
        }
        m
    }

    /// The flow of AGENT.
    pub fn flow_of(&self, agent: &str) -> Option<super::flows::run::FlowState> {
        self.flow_map().remove(agent)
    }

    /// Agent OLD is NEW now: its place, its session, its flow (an older thread's 1 flow: moved in
    /// first).
    pub fn rename_agent(&mut self, old: &str, new: &str) {
        let flow = self.flow_of(old);
        self.set_flow(old, None);
        for a in self.agents.iter_mut().filter(|a| *a == old) {
            *a = new.to_string();
        }
        if let Some(s) = self.sessions.remove(old) {
            self.sessions.insert(new.to_string(), s);
        }
        if flow.is_some() {
            self.set_flow(new, flow);
        }
    }

    /// AGENT's flow is ST (None: 0 flow). The old thread flow moves into FLOWS first.
    pub fn set_flow(&mut self, agent: &str, st: Option<super::flows::run::FlowState>) {
        self.flows = self.flow_map();
        match st {
            Some(s) => self.flows.insert(agent.to_string(), s),
            None => self.flows.remove(agent),
        };
        self.flow = self.lead().and_then(|l| self.flows.get(l)).cloned();
    }

    /// Where the agents work: the worktree, else the dir.
    pub fn work_dir(&self) -> PathBuf {
        PathBuf::from(self.worktree.as_deref().unwrap_or(&self.dir))
    }

    /// Every checkout of the thread: the work dir first, then the added ones (0 dupes).
    pub fn all_checkouts(&self) -> Vec<Checkout> {
        let mut v = vec![Checkout { dir: self.work_dir().display().to_string(), branch: self.branch.clone() }];
        for c in &self.checkouts {
            if !v.iter().any(|x| x.dir == c.dir) {
                v.push(c.clone());
            }
        }
        v
    }

    /// Add checkout C (a dir the thread has: 0 change). True: added.
    pub fn add_checkout(&mut self, c: Checkout) -> bool {
        if self.all_checkouts().iter().any(|x| x.dir == c.dir) {
            return false;
        }
        self.checkouts.push(c);
        true
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

/// Change thread ID: read, F, write, under the thread's `flock` (host, views and the CLI write
/// `thread.json`: 0 lost updates).
pub fn update(id: &str, f: impl FnOnce(&mut Thread)) -> Result<Thread, String> {
    let full = load(id).ok_or_else(|| format!("no thread {id}"))?.id;
    let lock = thread_dir(&full).join("thread.lock");
    super::locked(&lock, || {
        let mut t = load(&full).ok_or_else(|| format!("no thread {id}"))?;
        f(&mut t);
        t.updated = now_ms();
        t.save()?;
        Ok(t)
    })
    .map_err(|e| format!("{}: {e}", lock.display()))?
}

/// Every thread, newest activity first.
/// The thread updated last, by its thread.json's time (1 parse, not 1 per thread).
pub fn latest() -> Option<Thread> {
    let rd = std::fs::read_dir(threads_dir()).ok()?;
    let mut by_time: Vec<(std::time::SystemTime, PathBuf)> = rd.flatten().map(|e| e.path().join("thread.json")).filter_map(|p| Some((std::fs::metadata(&p).ok()?.modified().ok()?, p))).collect();
    by_time.sort_by_key(|(t, _)| std::cmp::Reverse(*t));
    // the newest that is no sub-thread (a running sub-thread writes its thread.json often)
    by_time.iter().filter_map(|(_, p)| serde_json::from_slice::<Thread>(&std::fs::read(p).ok()?).ok()).find(|t| t.of.is_none())
}

pub fn all() -> Vec<Thread> {
    let Ok(rd) = std::fs::read_dir(threads_dir()) else { return Vec::new() };
    let mut v: Vec<Thread> = rd
        .flatten()
        .filter_map(|e| serde_json::from_slice(&std::fs::read(e.path().join("thread.json")).ok()?).ok())
        // sub-threads: shown in the thread they run for, 0 row of their own
        .filter(|t: &Thread| t.of.is_none())
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
    /// An agent thought (a reasoning summary), 1 block. Empty thinking: 0 events.
    Thought { text: String },
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
        /// Size of the context window in tokens (0: unknown, older events).
        #[serde(default, skip_serializing_if = "is_zero")]
        window: u64,
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
    /// The agent stopped on a usage limit: the host sends it `continue` at AT (unix ms), with
    /// codehead open or not. AT 0: the resume is off (it ran, or the owner wrote first).
    Resume { at: u64 },
    /// The agent got a note of edits in its worktree since its last turn, not its own: SUMMARY
    /// (1 line: who, files, line ranges).
    Edits { summary: String },
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
    Flow { op: String, text: String },
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
    /// More about subagent ID: its transcript file (Claude Code), its task id (TaskStop).
    SubInfo { id: String, transcript: String, task: String },
    /// Subagent ID ended: STATE `done`, `failed`, `stopped` or `lost`; RESULT its report (or why).
    SubEnd { id: String, state: String, result: String },
    /// A sub-thread of this thread (`>sub`): STATE `running` (started, TEXT: its task), `done` /
    /// `failed` (TEXT: its result), `held` (a message waits for the sub-threads to end).
    SubThread { id: String, title: String, state: String, text: String },
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

/// The events in the last BYTES of thread ID's log (a cut 1st line left out).
pub fn events_tail(id: &str, bytes: u64) -> Vec<Event> {
    let Ok(mut f) = std::fs::File::open(events_path(id)) else { return Vec::new() };
    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
    let from = len.saturating_sub(bytes);
    if f.seek(SeekFrom::Start(from)).is_err() {
        return Vec::new();
    }
    let mut buf = Vec::new();
    if f.read_to_end(&mut buf).is_err() {
        return Vec::new();
    }
    let body = if from > 0 { buf.iter().position(|&b| b == b'\n').map(|i| &buf[i + 1..]).unwrap_or(&[]) } else { &buf[..] };
    body.split(|&b| b == b'\n').filter(|l| !l.is_empty()).filter_map(|l| serde_json::from_slice(l).ok()).collect()
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

    /// A tail from the last BYTES of the log (its 1st whole line on): the start of the 1st line it
    /// reads, the events before it are `head`'s.
    pub fn from_end(id: &str, bytes: u64) -> (Tail, u64) {
        let path = events_path(id);
        let len = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        if len <= bytes {
            return (Tail { path, offset: 0, partial: Vec::new() }, 0);
        }
        let from = len - bytes;
        // the line cut at FROM: its rest skipped
        let mut start = from;
        if let Ok(mut f) = std::fs::File::open(&path)
            && f.seek(SeekFrom::Start(from - 1)).is_ok()
        {
            let mut cut = Vec::new();
            if std::io::BufRead::read_until(&mut std::io::BufReader::new(f), b'\n', &mut cut).is_ok() {
                start = from - 1 + cut.len() as u64;
            }
        }
        (Tail { path, offset: start, partial: Vec::new() }, start)
    }
}

/// The events of thread ID's log before byte END (the part a `Tail::from_end` skipped).
pub fn head(id: &str, end: u64) -> Vec<Event> {
    let Ok(mut f) = std::fs::File::open(events_path(id)) else { return Vec::new() };
    let mut buf = vec![0u8; end as usize];
    if f.read_exact(&mut buf).is_err() {
        return Vec::new();
    }
    buf.split(|&b| b == b'\n').filter(|l| !l.is_empty()).filter_map(|l| serde_json::from_slice(l).ok()).collect()
}

impl Tail {
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
    fn a_tail_from_the_end_starts_at_a_whole_line_and_the_head_holds_the_rest() {
        let t = Thread::new("t", "do it", Path::new("/tmp"), vec!["ada".into()]);
        t.save().unwrap();
        for i in 0..20 {
            append(&t.id, &Event { ts: i, agent: Some("ada".into()), kind: Kind::Text { text: format!("line {i} {}", "x".repeat(40)) } }).unwrap();
        }
        let len = std::fs::metadata(events_path(&t.id)).unwrap().len();
        let (mut tail, end) = Tail::from_end(&t.id, len / 3);
        assert!(end > 0 && end < len);
        let (head, rest) = (head(&t.id, end), tail.read());
        assert!(!head.is_empty() && !rest.is_empty());
        let ts: Vec<u64> = head.iter().chain(&rest).map(|e| e.ts).collect();
        assert_eq!(ts, (0..20).collect::<Vec<_>>(), "every event once, in order");
        // a short log: the whole of it from the tail, 0 head
        let (_, end) = Tail::from_end(&t.id, len + 10);
        assert_eq!(end, 0);
        let _ = std::fs::remove_dir_all(thread_dir(&t.id));
    }

    #[test]
    fn events_round_trip_and_tail_reads_only_new_lines() {
        // the test process's data dir: 0 $CODEHEAD_HOME (a set var moves every other test's
        // threads mid-run: a stash written before it, read after it, is gone)
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
        assert_eq!(load(&t.id[..14]).unwrap().agents, vec!["ada".to_string()]);
        // 8 writers at once (host, views, CLI): the lock keeps each update
        let writers: Vec<_> = (0..8)
            .map(|w| {
                let id = t.id.clone();
                std::thread::spawn(move || {
                    for i in 0..10 {
                        update(&id, |t| t.agents.push(format!("a{w}-{i}"))).unwrap();
                    }
                })
            })
            .collect();
        for w in writers {
            w.join().unwrap();
        }
        assert_eq!(load(&t.id).unwrap().agents.len(), 81);
        let _ = std::fs::remove_dir_all(thread_dir(&t.id));
    }
}
