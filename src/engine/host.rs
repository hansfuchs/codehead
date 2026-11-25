//! `codehead host <thread>`: the detached process that owns a thread. It keeps 1 harness session
//! per working agent, writes every event to the thread's log, and takes commands on a Unix socket
//! (`<data>/run/<thread>.sock`, 1 JSON object per line, 1 reply line each).
//!
//! Commands (`op`):
//! - `prompt` {text, to?}: an owner message. TO, else the @names in TEXT, else the lead.
//! - `send` {from, to, text}: an agent's message (the `send` tool).
//! - `answer` {id, allow, answers?, option?, label}: the owner's pick for an open ask
//!   (OPTION: an ACP agent's option id).
//! - `interrupt`: stop the running turns.
//! - `join` {agent, text?}, `leave` {agent}: thread members.
//! - `status` {agent, state, note}: the `set_status` tool.
//! - `reload` {agent, fresh?}: end the agent's idle session (its profile changed).
//! - `ping`: live / busy agents, open asks.
//! - `subscribe`: the connection stays open; the host writes 1 JSON line per push: `live`
//!   {agent, text} (the live text so far, once), `delta` {agent, text} (a token chunk), `wake`
//!   (new log lines).
//! - `stop`: end every session, exit.
//!
//! Sessions start on the first message to their agent and resume the agent's last harness
//! session. 15 min. without a turn, an open ask or a command: the host exits (sessions resume
//! on the next message).

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Sender};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::harness::{self, Out, Session};
use super::roster;
use super::store::{self, Event, Kind};

const IDLE: Duration = Duration::from_secs(15 * 60);
/// Error of a turn the owner interrupted (the stream shows it as a note, not an error).
pub const INTERRUPTED: &str = "interrupted";

pub fn socket(id: &str) -> PathBuf {
    super::run_dir().join(format!("{id}.sock"))
}

enum Input {
    Cmd(Value, Sender<Value>),
    /// A view subscribed: lines for it (`wake`, `delta`) go to this sender.
    Subscribe(Sender<String>),
    Harness(String, Out),
    /// The worktree setup ended: its label, ok, output.
    Setup(String, bool, String),
    /// The pull of main before the 1st turn ended: ok, its last line.
    Pull(bool, String),
    /// The new thread's worktree: (its path, its port) or the error.
    Worktree(Result<(PathBuf, u16), String>),
}

struct Live {
    session: Session,
    busy: bool,
    /// The owner interrupted the running turn: its end is no error.
    interrupted: bool,
    /// The agent's MCP servers at the session's start (`servers::stamp`): another stamp at the
    /// next message → a new process (resumed) with the new set.
    mcp: String,
}

struct Host {
    id: String,
    exe: PathBuf,
    sessions: HashMap<String, Live>,
    /// Open asks: request id → (agent, request).
    asks: HashMap<String, (String, Value)>,
    harness_tx: Sender<(String, Out)>,
    last: Instant,
    /// `stop` came: exit after its reply.
    stopping: bool,
    /// Open views (`subscribe`): each gets `wake` per log line and the live text chunks.
    subs: std::cell::RefCell<Vec<Sender<String>>>,
    /// Per agent: live text since its last logged message: a new subscriber gets it first.
    live: HashMap<String, String>,
    /// Per agent in a turn: its tokens so far (`ping` gives them to the cost box).
    turn_use: HashMap<String, super::harness::TurnUse>,
    /// The worktree setup (or the pull of main) runs: messages wait in QUEUE (agent, text)
    /// until it ends.
    setup_running: bool,
    queue: Vec<(String, String)>,
    /// Messages of the last command (agent, text): delivered after its reply. A harness start
    /// and the edits snapshot take a second in a big repo; the sender never waits for them.
    later: Vec<(String, String)>,
    /// Subagents of the agents (their start, steps, end into the log).
    subagents: super::subagents::Tracker,
    sub_scan: Instant,
    /// Agents stopped on a usage limit: when each gets `continue` (unix ms; thread.json keeps it).
    resume: HashMap<String, u64>,
    /// Messages to this thread's agents while a sub-thread of it runs (same checkout): they go
    /// at its `sub_done`.
    held: Vec<(String, String)>,
    /// This thread as a sub-thread: its end.
    kid: Kid,
    /// The last look at this thread's running sub-threads (`kids_check`).
    kids_at: Instant,
}

/// A sub-thread whose host is down this long after its start: lost.
const KID_LOST: u64 = 20_000;

/// A sub-thread's run: turns ended, the error that failed it, idle since (its end: idle a while
/// after 1+ turns).
#[derive(Default)]
struct Kid {
    turns: usize,
    failed: Option<String>,
    idle_at: Option<Instant>,
}

/// A sub-thread idle this long after its last turn: done (an agent's `send` starts the next
/// turn within it).
const KID_DONE: Duration = Duration::from_secs(3);

/// A sub-thread's result to its thread: at most this many chars.
const KID_TEXT: usize = 20_000;

/// The text an agent gets when its usage window reset.
pub const RESUME_TEXT: &str = "[codehead: your usage limit window reset. Continue the task where you stopped.]";

/// When a usage limit of error ERR ends (unix ms): the claude plan window at 100 % that resets
/// last, else the `resets 1:50pm` of ERR (local time, the next one), else 30 min; + 1 min.
pub fn resume_at(err: &str, now: u64) -> u64 {
    let full = super::limits::read().into_iter().filter(|w| w.harness == "claude" && w.pct >= 99.5).filter_map(|w| w.resets).map(|r| r * 1000).filter(|&r| r > now).max();
    let parsed = || {
        static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
        let c = RE.get_or_init(|| regex::Regex::new(r"(?i)resets\s+(?:at\s+)?(\d{1,2})(?::(\d{2}))?\s*(am|pm)?").expect("valid regex")).captures(err)?;
        let mut h: i32 = c[1].parse().ok()?;
        let m: i32 = c.get(2).map_or(Some(0), |m| m.as_str().parse().ok())?;
        match c.get(3).map(|x| x.as_str().to_ascii_lowercase()) {
            Some(ap) if ap == "pm" && h < 12 => h += 12,
            Some(ap) if ap == "am" && h == 12 => h = 0,
            _ => {}
        }
        let secs = (now / 1000) as libc::time_t;
        // SAFETY: localtime_r / mktime read and write only TM.
        let mut tm: libc::tm = unsafe { std::mem::zeroed() };
        unsafe { libc::localtime_r(&secs, &mut tm) };
        tm.tm_hour = h;
        tm.tm_min = m;
        tm.tm_sec = 0;
        let mut at = unsafe { libc::mktime(&mut tm) } as i64 * 1000;
        if at <= now as i64 {
            at += 86_400_000;
        }
        u64::try_from(at).ok()
    };
    full.or_else(parsed).unwrap_or(now + 30 * 60_000) + 60_000
}

/// @names in TEXT that name an active agent, in order, once each.
pub fn mentions(text: &str, names: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for (i, _) in text.match_indices('@') {
        if i > 0 && text[..i].chars().next_back().is_some_and(|c| c.is_alphanumeric() || c == '`') {
            continue;
        }
        let word: String = text[i + 1..].chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_').collect();
        if let Some(n) = names.iter().find(|n| n.eq_ignore_ascii_case(&word))
            && !out.contains(n)
        {
            out.push(n.clone());
        }
    }
    out
}

impl Host {
    fn log(&self, agent: Option<&str>, kind: Kind) {
        if let Err(e) = store::append(&self.id, &Event::new(agent, kind)) {
            eprintln!("log: {e}");
        }
        self.push(&json!({"op": "wake"}));
    }

    /// A view subscribed: the live text so far first, then each push.
    fn subscribe(&self, s: Sender<String>) {
        for (agent, text) in &self.live {
            let _ = s.send(json!({"op": "live", "agent": agent, "text": text}).to_string());
        }
        self.subs.borrow_mut().push(s);
    }

    /// Send line V to each subscribed view; closed views go.
    fn push(&self, v: &Value) {
        let line = v.to_string();
        self.subs.borrow_mut().retain(|s| s.send(line.clone()).is_ok());
    }

    fn thread(&self) -> Result<store::Thread, String> {
        store::load(&self.id).ok_or_else(|| format!("thread {} gone", self.id))
    }

    /// AGENT on the thread (joins it first), with a running session.
    fn ensure(&mut self, agent: &str) -> Result<(), String> {
        let mut t = self.thread()?;
        if !t.agents.iter().any(|a| a == agent) {
            roster::get(agent).filter(|a| !a.archived).ok_or_else(|| format!("no agent {agent}"))?;
            t = store::update(&self.id, |t| t.agents.push(agent.to_string()))?;
            self.log(Some(agent), Kind::Join);
        }
        let a = roster::get(agent).ok_or_else(|| format!("no agent {agent}"))?;
        let mcp = super::servers::stamp(&a);
        if let Some(l) = self.sessions.get(agent) {
            // the MCP servers changed: an idle session restarts (resumed: the conversation stays)
            if l.busy || l.mcp == mcp {
                return Ok(());
            }
            if let Some(mut l) = self.sessions.remove(agent) {
                l.session.kill();
            }
        }
        let roster = roster::active();
        let system = harness::system_prompt(&a, &t, &roster);
        let dir = t.work_dir();
        if let Some(p) = super::checkouts::prune(&t) {
            t = p;
        }
        let extra: Vec<PathBuf> = t.all_checkouts().into_iter().skip(1).map(|c| PathBuf::from(c.dir)).filter(|d| d.is_dir()).collect();
        let spec = harness::Spec { agent: &a, thread: &self.id, dir: &dir, resume: t.sessions.get(agent).map(String::as_str), fork: t.fork.iter().any(|a| a == agent), system: &system, exe: &self.exe, extra: &extra };
        let session = harness::spawn(&spec, self.harness_tx.clone())?;
        self.sessions.insert(agent.to_string(), Live { session, busy: false, interrupted: false, mcp });
        Ok(())
    }

    /// AGENT stopped on usage limit ERR: `continue` at the window's reset.
    fn schedule_resume(&mut self, agent: &str, err: &str) {
        let at = resume_at(err, super::now_ms());
        self.resume.insert(agent.to_string(), at);
        let a = agent.to_string();
        let _ = store::update(&self.id, move |t| {
            t.resume.insert(a, at);
        });
        self.log(Some(agent), Kind::Resume { at });
    }

    /// A message for AGENT came first: its resume is off.
    fn cancel_resume(&mut self, agent: &str) {
        if self.resume.remove(agent).is_some() {
            let a = agent.to_string();
            let _ = store::update(&self.id, move |t| {
                t.resume.remove(&a);
            });
            self.log(Some(agent), Kind::Resume { at: 0 });
        }
    }

    /// Resumes due: `continue` to each agent (a message of codehead in the log).
    fn run_resumes(&mut self) {
        let now = super::now_ms();
        let due: Vec<String> = self.resume.iter().filter(|(_, at)| **at <= now).map(|(a, _)| a.clone()).collect();
        for agent in due {
            self.cancel_resume(&agent);
            self.last = Instant::now();
            self.log(None, Kind::Prompt { from: "codehead".into(), to: vec![agent.clone()], text: "continue · the usage limit window reset".into() });
            if let Err(e) = self.deliver(&agent, RESUME_TEXT) {
                self.log(Some(&agent), Kind::Error { message: format!("auto resume: {e}") });
            }
        }
    }

    fn deliver(&mut self, agent: &str, text: &str) -> Result<(), String> {
        // the worktree setup runs: the message waits for its end
        if self.setup_running {
            self.queue.push((agent.to_string(), text.to_string()));
            // its session boots meanwhile (a harness takes seconds): only the message waits
            self.boot_queued();
            return Ok(());
        }
        // a sub-thread works in the same checkout: the message waits for its end (`sub_done`)
        if self.thread().is_ok_and(|t| !t.kids.is_empty()) {
            if self.held.is_empty() {
                self.log(None, Kind::SubThread { id: String::new(), title: String::new(), state: "held".into(), text: "messages wait until the sub-thread ends (same checkout)".into() });
            }
            self.held.push((agent.to_string(), text.to_string()));
            return Ok(());
        }
        self.ensure(agent)?;
        let mut text = text.to_string();
        // a new turn: edits in the worktree since the agent's last turn, not its own, go first
        if self.sessions.get(agent).is_some_and(|l| !l.busy) {
            let others = self.sessions.iter().any(|(n, l)| n != agent && l.busy);
            let dir = self.thread()?.work_dir();
            if let Some(n) = super::edits::turn_start(&self.id, &dir, agent, others) {
                self.log(Some(agent), Kind::Edits { summary: n.summary });
                text = format!("{}\n\n{text}", n.text);
            }
        }
        let live = self.sessions.get_mut(agent).ok_or("0 session")?;
        live.session.prompt(&text)?;
        if !live.busy {
            live.busy = true;
            self.log(Some(agent), Kind::TurnStart);
        }
        Ok(())
    }

    fn cmd(&mut self, c: &Value) -> Result<Value, String> {
        // a status poll (`ping`) is no activity: an open view must not keep the host alive
        if c["op"].as_str() != Some("ping") {
            self.last = Instant::now();
        }
        let s = |k: &str| c[k].as_str().unwrap_or_default().to_string();
        let list = |k: &str| -> Vec<String> {
            match &c[k] {
                Value::String(x) => x.split(',').map(|x| x.trim().trim_start_matches('@').to_string()).filter(|x| !x.is_empty()).collect(),
                Value::Array(a) => a.iter().filter_map(Value::as_str).map(|x| x.trim_start_matches('@').to_string()).collect(),
                _ => Vec::new(),
            }
        };
        match c["op"].as_str().unwrap_or_default() {
            "ping" => Ok(json!({
                "live": self.sessions.keys().collect::<Vec<_>>(),
                // busy: in a turn, or waiting on running subagents
                "busy": self.sessions.iter().filter(|(n, l)| l.busy || self.subagents.masters().contains(*n)).map(|(n, _)| n).collect::<Vec<_>>(),
                "asks": self.asks.len(),
                "usage": self.turn_use,
            })),
            "prompt" => {
                let text = s("text");
                if text.trim().is_empty() {
                    return Err("empty message".into());
                }
                let t = self.thread()?;
                let mut to = list("to");
                if to.is_empty() {
                    let names: Vec<String> = roster::active().into_iter().map(|a| a.name).collect();
                    to = mentions(&text, &names);
                }
                if to.is_empty() {
                    to = t.lead().map(|l| vec![l.to_string()]).ok_or("thread has 0 agents")?;
                }
                self.log(None, Kind::Prompt { from: "owner".into(), to: to.clone(), text: text.clone() });
                for a in &to {
                    self.cancel_resume(a);
                }
                if t.state != "open" {
                    store::update(&self.id, |t| t.state = "open".into())?;
                }
                // CONTEXT (the owner's shell commands since the last message): for the agents only
                let ctx = s("context");
                let full = if ctx.trim().is_empty() { text.clone() } else { format!("{text}\n\n{ctx}") };
                // a flow whose current step went to 0 agent yet: the step goes with this message
                let steps = self.flow_unsent(&t, &to, &full);
                self.later.extend(to.iter().map(|a| match steps.iter().find(|(sa, _)| sa == a) {
                    Some((_, m)) => (a.clone(), m.clone()),
                    None => (a.clone(), full.clone()),
                }));
                Ok(json!({"to": to}))
            }
            "send" => {
                let (from, text, to) = (s("from"), s("text"), list("to"));
                if to.is_empty() || text.trim().is_empty() {
                    return Err("send: `to` and `message` required".into());
                }
                if to.contains(&from) {
                    return Err("send: 0 messages to yourself".into());
                }
                for a in &to {
                    roster::get(a).filter(|a| !a.archived).ok_or_else(|| format!("no agent {a}"))?;
                }
                self.log(Some(&from), Kind::Prompt { from: from.clone(), to: to.clone(), text: text.clone() });
                for a in &to {
                    self.cancel_resume(a);
                }
                for a in &to {
                    self.deliver(a, &format!("[from @{from}] {text}"))?;
                }
                Ok(json!({"to": to}))
            }
            "answer" => {
                let id = s("id");
                let (agent, req) = self.asks.get(&id).cloned().ok_or_else(|| format!("no open ask {id}"))?;
                let Some(live) = self.sessions.get_mut(&agent) else {
                    self.asks.remove(&id);
                    return Err(format!("{agent}: session gone"));
                };
                let allow = c["allow"].as_bool().unwrap_or(false);
                // an ACP option picks itself, allow or reject
                if allow || c["option"].is_string() {
                    let mut input = req["input"].clone();
                    if let (Some(o), Some(opt)) = (input.as_object_mut(), c["option"].as_str()) {
                        o.insert("_option".into(), Value::String(opt.into()));
                    }
                    if let (Some(o), Some(ans)) = (input.as_object_mut(), c.get("answers").filter(|a| a.is_object())) {
                        o.insert("answers".into(), ans.clone());
                    }
                    live.session.allow(&id, &input)?;
                } else {
                    let why = c["note"].as_str().filter(|n| !n.is_empty()).unwrap_or("The owner said no.");
                    live.session.deny(&id, why)?;
                }
                // answered: the ask goes only now (a failed write keeps it open for a retry)
                self.asks.remove(&id);
                self.log(Some(&agent), Kind::Answer { id, allow, label: s("label") });
                Ok(json!({}))
            }
            // the owner stops subagent ID: a background one of Claude Code through its master
            // (TaskStop), a foreground one with its master's turn
            "sub_stop" => {
                let id = s("id");
                let r = self.subagents.stop(&id).ok_or_else(|| format!("subagent {id}: 0 running"))?;
                let master = r.agent.clone();
                let how = if r.background && !r.task.is_empty() {
                    let text = format!(
                        "[codehead] The owner stopped your background subagent “{}” (task id {}). Stop it now with TaskStop (task_id: {}) and go on without its result.",
                        r.description, r.task, r.task
                    );
                    self.deliver(&master, &text)?;
                    "its master stops it"
                } else {
                    // a foreground subagent ends with its master's turn: its other foreground ones too
                    if let Some(l) = self.sessions.get_mut(&master).filter(|l| l.busy) {
                        l.session.interrupt()?;
                        l.interrupted = true;
                    }
                    let rest: Vec<String> = self.subagents.running.iter().filter(|(_, x)| x.agent == master && !x.background).map(|(i, _)| i.clone()).collect();
                    for i in rest {
                        self.subagents.stop(&i);
                        self.log(Some(&master), Kind::SubEnd { id: i, state: "stopped".into(), result: "its master's turn was interrupted".into() });
                    }
                    "its master's turn interrupted"
                };
                self.log(Some(&master), Kind::SubEnd { id, state: "stopped".into(), result: how.into() });
                Ok(json!({"stopped": how}))
            }
            "interrupt" => {
                let (mut stopped, mut errs) = (Vec::new(), Vec::new());
                for (name, live) in self.sessions.iter_mut().filter(|(_, l)| l.busy) {
                    match live.session.interrupt() {
                        Ok(()) => {
                            live.interrupted = true;
                            stopped.push(name.clone());
                        }
                        Err(e) => errs.push(format!("{name}: {e}")),
                    }
                }
                // their open asks are moot: 0 answer comes (ACP: `cancel` answered them)
                self.asks.retain(|_, (a, _)| !stopped.contains(a));
                if !errs.is_empty() {
                    return Err(errs.join("; "));
                }
                Ok(json!({"interrupted": stopped.len()}))
            }
            "join" => {
                let agent = s("agent");
                self.ensure(&agent)?;
                let text = s("text");
                if !text.trim().is_empty() {
                    self.log(None, Kind::Prompt { from: "owner".into(), to: vec![agent.clone()], text: text.clone() });
                    self.deliver(&agent, &text)?;
                }
                Ok(json!({}))
            }
            // AGENT leaves; its flow goes, or (TO: the agent that replaces it) goes to TO: the
            // current step again with TO's 1st message
            "leave" => {
                let (agent, to) = (s("agent"), s("to"));
                if let Some(mut l) = self.sessions.remove(&agent) {
                    l.session.kill();
                }
                self.asks.retain(|_, (a, _)| *a != agent);
                let flow = self.thread()?.flow_of(&agent);
                if let Some(f) = &flow {
                    let line = if to.is_empty() { format!("flow {} of @{agent} ended: it left", f.title) } else { format!("flow {} of @{agent} → @{to}", f.title) };
                    self.flow_log(&agent, "clear", line);
                }
                store::update(&self.id, |t| {
                    t.set_flow(&agent, None);
                    t.agents.retain(|a| *a != agent);
                    if let Some(mut f) = flow.filter(|_| !to.is_empty()) {
                        f.sent = false;
                        f.pending = None;
                        t.set_flow(&to, Some(f));
                    }
                })?;
                self.log(Some(&agent), Kind::Leave);
                Ok(json!({}))
            }
            "reload" => {
                // the agent's profile changed: its idle session ends, the next message starts
                // the new one (fresh: the harness changed, the conversation does not carry over)
                let agent = s("agent");
                if self.sessions.get(&agent).is_some_and(|l| l.busy) {
                    return Err(format!("{agent} is mid-turn"));
                }
                if let Some(mut l) = self.sessions.remove(&agent) {
                    l.session.kill();
                }
                if c["fresh"].as_bool() == Some(true) {
                    store::update(&self.id, |t| {
                        t.sessions.remove(&agent);
                    })?;
                }
                Ok(json!({}))
            }
            "status" => {
                let state = s("state");
                if !["working", "blocked", "review", "done"].contains(&state.as_str()) {
                    return Err("status: working, blocked, review or done".into());
                }
                self.log(Some(&s("agent")), Kind::Status { state, note: s("note") });
                Ok(json!({}))
            }
            "stop" => {
                self.stopping = true;
                Ok(json!({}))
            }
            // a sub-thread of this thread started (the view made it): its agents' messages wait
            "sub_start" => {
                let id = s("id");
                if id.is_empty() {
                    return Err("sub_start: id required".into());
                }
                let k = id.clone();
                store::update(&self.id, move |t| {
                    if !t.kids.contains(&k) {
                        t.kids.push(k);
                    }
                })?;
                self.log(None, Kind::SubThread { id, title: s("title"), state: "running".into(), text: s("text") });
                Ok(json!({}))
            }
            // a sub-thread ended (its host reports, or the view found it gone): its result as a
            // message of AGENT (its agent; 0: unknown), the waiting messages go once 0 sub-thread
            // runs. An id not running: 0 effect.
            "sub_done" => {
                let id = s("id");
                let t = self.thread()?;
                if !t.kids.contains(&id) {
                    return Ok(json!({}));
                }
                let k = id.clone();
                let t = store::update(&self.id, move |t| t.kids.retain(|x| *x != k))?;
                let state = if s("state").is_empty() { "done".to_string() } else { s("state") };
                let agent = s("agent");
                self.log(Some(&agent).filter(|a| !a.is_empty()).map(String::as_str), Kind::SubThread { id, title: s("title"), state, text: s("text") });
                if t.kids.is_empty() {
                    let held = std::mem::take(&mut self.held);
                    self.later.extend(held);
                }
                Ok(json!({}))
            }
            op if op.starts_with("flow_") => self.flow_cmd(op, c),
            op => Err(format!("unknown op {op:?}")),
        }
    }

    fn harness(&mut self, agent: String, o: Out) {
        self.last = Instant::now();
        match o {
            Out::Event(mut kind) => {
                // a usage limit ended the turn: the resume goes in the log after the turn end
                let mut pending_resume: Option<String> = None;
                if matches!(kind, Kind::Text { .. } | Kind::Tool { .. } | Kind::TurnEnd { .. }) {
                    self.live.remove(&agent);
                }
                // an interrupted turn: Claude Code ends it with `error_during_execution`
                if let Kind::TurnEnd { error, .. } = &mut kind
                    && let Some(l) = self.sessions.get_mut(&agent)
                    && std::mem::take(&mut l.interrupted)
                    && error.is_some()
                {
                    *error = Some(INTERRUPTED.into());
                }
                match &kind {
                    Kind::Session { session, .. } => {
                        // stream-json repeats its init line every turn: 1 event per new session
                        if self.thread().is_ok_and(|t| t.sessions.get(&agent) == Some(session)) {
                            return;
                        }
                        let (a, s) = (agent.clone(), session.clone());
                        let _ = store::update(&self.id, move |t| {
                            t.fork.retain(|x| *x != a);
                            t.sessions.insert(a, s);
                        });
                    }
                    Kind::Text { .. } | Kind::Thought { .. } | Kind::Tool { .. } => {
                        // an edit in another repo / worktree: that checkout joins the thread
                        if let Kind::Tool { name, input, .. } = &kind
                            && let Ok(t) = self.thread()
                        {
                            super::checkouts::note(&t, name, input);
                        }
                        // a queued message runs as its own turn: it starts with output
                        if let Some(l) = self.sessions.get_mut(&agent)
                            && !l.busy
                        {
                            l.busy = true;
                            self.log(Some(&agent), Kind::TurnStart);
                        }
                    }
                    Kind::TurnEnd { cost, error, .. } => {
                        self.turn_use.remove(&agent);
                        if let Some(l) = self.sessions.get_mut(&agent) {
                            l.busy = false;
                        }
                        // a sub-thread: its turns count towards its end; an error fails it
                        self.kid.turns += 1;
                        if error.is_some() {
                            self.kid.failed = error.clone();
                        }
                        if let Some(e) = error.as_deref().filter(|e| crate::stream::usage_limit(e)) {
                            pending_resume = Some(e.to_string());
                        }
                        // Claude's $: the base of the $ per % of its plan windows
                        if roster::get(&agent).is_some_and(|a| a.harness.split('/').next() == Some("claude")) {
                            super::limits::add_cost(*cost);
                        }
                        // a turn ends: its open asks are moot (the view drops them too)
                        self.asks.retain(|_, (a, _)| *a != agent);
                        // its edits go in the log of edits (the next note of every other agent)
                        if let Ok(t) = self.thread() {
                            let idle = !self.sessions.values().any(|l| l.busy);
                            super::edits::turn_end(&self.id, &t.work_dir(), &agent, idle);
                        }
                    }
                    _ => {}
                }
                // a task tool call starts a subagent, its result ends it
                let subs = self.subagents.event(&agent, &kind);
                let boundary = matches!(kind, Kind::ToolResult { .. } | Kind::TurnEnd { .. });
                self.log(Some(&agent), kind);
                if boundary {
                    self.flow_boundary(&agent);
                }
                for (a, k) in subs {
                    self.log(Some(&a), k);
                }
                if let Some(e) = pending_resume {
                    self.schedule_resume(&agent, &e);
                }
            }
            Out::Delta(text) => {
                self.push(&json!({"op": "delta", "agent": agent, "text": text}));
                self.live.entry(agent).or_default().push_str(&text);
            }
            // the plan's windows are the account's: 1 file for every host
            Out::Limits(l) => super::limits::record("claude", &l),
            Out::Mcp(harness, rows) => super::servers::record(&harness, rows),
            Out::Usage(u) => {
                self.turn_use.insert(agent, u);
            }
            Out::Control(id, req) => {
                // ACP agents: the mode decides what runs without the owner, auto-allowed MCP
                // servers run always
                if let Some(l) = self.sessions.get_mut(&agent)
                    && l.session.host_allows()
                    && let Some(a) = roster::get(&agent)
                    && let tool = req["tool_name"].as_str().unwrap_or_default()
                    && (super::guard::auto_allow(&a.mode, tool, req["input"]["_kind"].as_str().unwrap_or_default(), &req["input"])
                        || super::servers::acp_auto_allowed(tool, &super::servers::for_agent(&a)))
                {
                    if let Err(e) = l.session.allow(&id, &json!({})) {
                        self.log(Some(&agent), Kind::Error { message: format!("auto-allow: {e}") });
                    }
                    return;
                }
                if req["subtype"] == "can_use_tool" {
                    // an ask of a subagent: its name and master go with it
                    let mut input = req["input"].clone();
                    if let Some((_, r)) = req["agent_id"].as_str().and_then(|t| self.subagents.by_task(t))
                        && input.is_object()
                    {
                        input["_subagent"] = json!(r.description);
                    }
                    self.log(Some(&agent), Kind::Ask { id: id.clone(), tool: req["tool_name"].as_str().unwrap_or_default().into(), input });
                    self.asks.insert(id, (agent, req));
                }
            }
            Out::Exit(err) => {
                self.live.remove(&agent);
                if let Some(mut l) = self.sessions.remove(&agent) {
                    // stdout closed or the setup failed: the process can still run. A short
                    // grace, then kill: a blocking wait here stops the host loop.
                    let at = Instant::now();
                    let mut status = None;
                    while at.elapsed() < Duration::from_secs(2) {
                        match l.session.child.try_wait() {
                            Ok(Some(s)) => {
                                status = Some(s);
                                break;
                            }
                            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
                            Err(_) => break,
                        }
                    }
                    let killed = status.is_none();
                    if killed {
                        l.session.kill();
                    }
                    let status = status.and_then(|s| s.code());
                    if l.busy || killed || status.is_some_and(|c| c != 0) {
                        let tail: Vec<&str> = err.lines().rev().take(6).collect::<Vec<_>>().into_iter().rev().collect();
                        let msg = if tail.is_empty() { format!("session ended (exit {status:?})") } else { tail.join("\n") };
                        self.log(Some(&agent), Kind::Error { message: msg });
                        if l.busy {
                            self.log(Some(&agent), session_ended());
                        }
                    }
                }
                for (a, k) in self.subagents.exit(&agent, &format!("the harness process of {agent} ended")) {
                    self.log(Some(&a), k);
                }
                self.asks.retain(|_, (a, _)| *a != agent);
            }
        }
    }

    /// Thread T's setup is pending: it runs on a thread (a `setup` tool run in the log); the
    /// messages wait until it ends.
    fn start_setup(&mut self, t: &store::Thread, tx: Sender<Input>) {
        let Some(wt) = t.worktree.as_deref() else { return };
        let Some((_, label, _)) = crate::setup::script(Path::new(wt), "setup") else {
            let _ = store::update(&self.id, |t| t.setup = None);
            return;
        };
        self.setup_running = true;
        self.log(Some("setup"), Kind::Tool { id: "setup".into(), name: "Bash".into(), input: json!({"command": label, "description": "worktree setup: the 1st turn waits for it"}) });
        let t = t.clone();
        std::thread::spawn(move || {
            let (label, ok, out) = crate::setup::run_setup(&t).unwrap_or_else(|| ("setup".into(), true, String::new()));
            let _ = tx.send(Input::Setup(label, ok, out));
        });
    }

    /// The setup ended: its result in the log, the waiting messages go out (a failed setup: with
    /// a note on it).
    fn setup_done(&mut self, label: String, ok: bool, out: String) {
        self.setup_running = false;
        self.log(Some("setup"), Kind::ToolResult { id: "setup".into(), ok, output: out });
        let _ = store::update(&self.id, |t| t.setup = Some(if ok { "done" } else { "failed" }.into()));
        let note = (!ok).then(|| format!("[codehead: the worktree setup (`{label}`) failed; its output is in the thread. Make the environment work first.]"));
        self.release_queue(note.as_deref());
    }

    /// A new thread (`kickoff`): its task to its agents, as the owner's `prompt` (the UI started
    /// the host and opened the view at once, 0 wait for a reply).
    fn kickoff(&mut self, t: &store::Thread) {
        if !t.kickoff {
            return;
        }
        // off first: a host that dies after this line never sends the task twice
        let _ = store::update(&self.id, |t| t.kickoff = false);
        if let Err(e) = self.cmd(&json!({"op": "prompt", "to": t.agents, "text": t.task})) {
            self.log(None, Kind::Error { message: format!("task not sent: {e}") });
        }
        self.deliver_later();
    }

    /// A new thread's worktree (`worktree_pending`): `git fetch` + `git worktree add`, the local
    /// files, a port, off the host loop; messages wait for its end.
    fn start_worktree(&mut self, t: &store::Thread, tx: Sender<Input>) {
        let Some(branch) = t.worktree_pending.clone() else { return };
        self.setup_running = true;
        self.log(Some("setup"), Kind::Tool { id: "worktree".into(), name: "Bash".into(), input: json!({"command": format!("git worktree add {branch}"), "description": "the thread's worktree: the 1st turn waits for it"}) });
        let repo = PathBuf::from(&t.dir);
        std::thread::spawn(move || {
            let r = crate::threads::add_worktree(&repo, &branch).map(|p| {
                // the worktree stays on the branch; local files (.env …), a port block
                pin_branch(&p, &branch);
                crate::setup::copy_includes(&repo, &p);
                (p, crate::setup::free_port(&store::all()))
            });
            let _ = tx.send(Input::Worktree(r));
        });
    }

    /// The worktree is there (or failed): in thread.json, then its setup script, else the waiting
    /// messages go out (a failed worktree: with a note, the agents are in the main checkout).
    fn worktree_done(&mut self, r: Result<(PathBuf, u16), String>, tx: Sender<Input>) {
        let branch = self.thread().ok().and_then(|t| t.worktree_pending).unwrap_or_default();
        match r {
            Ok((p, port)) => {
                self.log(Some("setup"), Kind::ToolResult { id: "worktree".into(), ok: true, output: format!("{} on {branch}", p.display()) });
                let setup = crate::setup::script(&p, "setup").is_some();
                let path = p.display().to_string();
                let t = store::update(&self.id, |t| {
                    t.worktree = Some(path);
                    t.branch = Some(branch.clone());
                    t.port = Some(port);
                    t.worktree_pending = None;
                    if setup {
                        t.setup = Some("pending".into());
                    }
                });
                match t {
                    Ok(t) if setup => {
                        self.setup_running = false;
                        self.start_setup(&t, tx);
                        if self.setup_running {
                            // the sessions boot during the setup (the worktree is there)
                            self.boot_queued();
                        } else {
                            self.release_queue(None);
                        }
                    }
                    _ => {
                        self.setup_running = false;
                        self.release_queue(None);
                    }
                }
            }
            Err(e) => {
                self.log(Some("setup"), Kind::ToolResult { id: "worktree".into(), ok: false, output: e.clone() });
                let dir = self.thread().map(|t| t.dir).unwrap_or_default();
                let _ = store::update(&self.id, |t| t.worktree_pending = None);
                self.setup_running = false;
                let note = format!("[codehead: the worktree for branch `{branch}` failed ({e}): you are in the main checkout {dir}. Ask the owner before you edit files.]");
                self.release_queue(Some(&note));
            }
        }
    }

    /// The agents of the waiting messages: their sessions start now (a harness boots for seconds),
    /// the messages still wait. 0 boot while the worktree is not there (the session's dir).
    fn boot_queued(&mut self) {
        if self.thread().is_ok_and(|t| t.worktree_pending.is_some()) {
            return;
        }
        let mut agents: Vec<String> = self.queue.iter().map(|(a, _)| a.clone()).collect();
        agents.sort();
        agents.dedup();
        for a in agents {
            // an error shows at the delivery (the message's own error event)
            let _ = self.ensure(&a);
        }
    }

    /// A thread on main: `git pull --ff-only` off the host loop; messages wait for its end.
    fn start_pull(&mut self, t: &store::Thread, tx: Sender<Input>) {
        self.setup_running = true;
        self.log(Some("setup"), Kind::Tool { id: "pull".into(), name: "Bash".into(), input: json!({"command": "git pull --ff-only", "description": "main up to date: the 1st turn waits for it"}) });
        let dir = t.work_dir();
        std::thread::spawn(move || {
            let (ok, out) = match crate::threads::update_default(&dir) {
                None => (true, "0 pull: not on main / master with an upstream".to_string()),
                Some(Ok(b)) => (true, format!("{b} up to date")),
                Some(Err(e)) => (false, e),
            };
            let _ = tx.send(Input::Pull(ok, out));
        });
    }

    /// The pull ended: its result in the log, the waiting messages go out (a failed pull: with a
    /// note on it, the agents work on the local main).
    fn pull_done(&mut self, ok: bool, out: String) {
        self.setup_running = false;
        self.log(Some("setup"), Kind::ToolResult { id: "pull".into(), ok, output: out });
        let _ = store::update(&self.id, |t| t.pull = false);
        let note = (!ok).then_some("[codehead: `git pull --ff-only` of main failed (output in the thread): main may be behind its upstream.]");
        self.release_queue(note);
    }

    /// A message to AGENT did not go (its harness did not start, …): the error in the log; a
    /// sub-thread fails with it (its end comes, 0 turn ever will).
    fn undelivered(&mut self, agent: &str, e: String) {
        self.kid.failed = Some(e.clone());
        self.kid.turns += 1;
        self.log(Some(agent), Kind::Error { message: e });
    }

    /// The waiting messages go out, each with NOTE on top.
    fn release_queue(&mut self, note: Option<&str>) {
        for (agent, text) in std::mem::take(&mut self.queue) {
            let text = match note {
                Some(n) => format!("{n}\n\n{text}"),
                None => text,
            };
            if let Err(e) = self.deliver(&agent, &text) {
                self.undelivered(&agent, e);
            }
        }
    }

    /// The messages of the last command go out (its reply went first).
    fn deliver_later(&mut self) {
        for (agent, text) in std::mem::take(&mut self.later) {
            if let Err(e) = self.deliver(&agent, &text) {
                self.undelivered(&agent, e);
            }
        }
    }

    fn stop(&mut self) {
        let sessions: Vec<(String, Live)> = self.sessions.drain().collect();
        for (agent, mut l) in sessions {
            l.session.kill();
            // a turn the stop cut: closed in the log (else the view shows it at work forever)
            if l.busy {
                self.log(Some(&agent), session_ended());
            }
        }
        let _ = std::fs::remove_file(socket(&self.id));
    }

    /// Turns a crashed host left open (a `turn_start` with 0 `turn_end`): closed at start.
    /// Claude Code agents: their subagent files (transcripts, nested ones, ends).
    fn scan_subs(&mut self) {
        self.sub_scan = Instant::now();
        let Ok(t) = self.thread() else { return };
        let sessions: Vec<(String, String)> = t
            .sessions
            .iter()
            .filter(|(a, _)| roster::get(a).is_some_and(|x| x.harness == "claude"))
            .map(|(a, s)| (a.clone(), s.clone()))
            .collect();
        let root = crate::config::home().join(".claude/projects");
        for (a, k) in self.subagents.scan(&root, &sessions) {
            self.log(Some(&a), k);
        }
    }

    fn close_open_turns(&self) {
        let mut open: Vec<String> = Vec::new();
        for e in store::events(&self.id) {
            let Some(a) = e.agent else { continue };
            match e.kind {
                Kind::TurnStart if !open.contains(&a) => open.push(a),
                Kind::TurnEnd { .. } | Kind::Leave => open.retain(|x| *x != a),
                _ => {}
            }
        }
        for a in open {
            self.log(Some(&a), session_ended());
        }
    }
}

// ---------- flows ----------

/// A flow move's result for the agent that reported (the `flow_step` tool's text).
fn moved_reply(text: Option<String>, handed: Option<String>, wait: Option<&str>) -> Value {
    json!({"text": text, "handed": handed, "wait": wait})
}

impl Host {
    /// AGENT's flow: the thread, the flow's state, the flow, its nodes.
    fn flow_ctx(&self, agent: &str) -> Result<(store::Thread, super::flows::run::FlowState, super::flows::Flow, Vec<super::flows::Resolved>), String> {
        let t = self.thread()?;
        let st = t.flow_of(agent).ok_or_else(|| format!("@{agent} has 0 flow"))?;
        let lib = super::flows::Library::load(Some(&t.work_dir()));
        let f = lib.flow(&st.flow).cloned().ok_or_else(|| format!("flow {} not found", st.flow))?;
        let nodes = super::flows::resolve(&f, &lib);
        Ok((t, st, f, nodes))
    }

    fn flow_save(&self, agent: &str, st: &super::flows::run::FlowState) -> Result<(), String> {
        let (agent, st) = (agent.to_string(), st.clone());
        store::update(&self.id, move |t| t.set_flow(&agent, Some(st))).map(|_| ())
    }

    /// The agent whose flow an owner op means: `agent`, else the only agent with a flow, else
    /// (0 flow) the lead. 2+ flows and 0 `agent`: an error that names them.
    fn flow_agent(&self, c: &Value) -> Result<String, String> {
        if let Some(a) = c["agent"].as_str().filter(|a| !a.is_empty()) {
            return Ok(a.to_string());
        }

        let t = self.thread()?;
        let flows = t.flow_map();
        match flows.len() {
            0 => t.lead().map(String::from).ok_or_else(|| "the thread has 0 agents".into()),
            1 => Ok(flows.into_keys().next().unwrap_or_default()),
            _ => Err(format!("2+ agents have a flow ({}): name 1 with `agent`", flows.keys().map(|a| format!("@{a}")).collect::<Vec<_>>().join(", "))),
        }
    }

    /// A sub-thread whose agents are done (1+ turns, 0 busy, 0 waiting message or ask, idle for
    /// KID_DONE): its result (the last agent text) to the thread it runs for, then it goes (thread
    /// deleted, host ends).
    fn kid_check(&mut self) {
        let Ok(t) = self.thread() else { return };
        let Some(main) = t.of.clone() else { return };
        let idle = self.kid.turns > 0 && !self.sessions.values().any(|l| l.busy) && self.later.is_empty() && self.queue.is_empty() && self.asks.is_empty() && !self.subagents.any() && !self.setup_running;
        if !idle {
            self.kid.idle_at = None;
            return;
        }
        let since = *self.kid.idle_at.get_or_insert_with(Instant::now);
        if since.elapsed() < KID_DONE {
            return;
        }
        // a monitor or background command of its agents runs: not done (its result comes later)
        if crate::monitors::watching(&self.id, &self.sessions.keys().cloned().collect::<Vec<_>>()) {
            self.kid.idle_at = None;
            return;
        }
        let events = store::events_tail(&self.id, 1 << 20);
        // its result: the last message of its agents (the agent that wrote it: the message's)
        let last = events.iter().rev().find_map(|e| match &e.kind {
            Kind::Text { text } if !text.trim().is_empty() => Some((e.agent.clone().unwrap_or_default(), text.trim().to_string())),
            _ => None,
        });
        let lead = t.agents.first().cloned().unwrap_or_default();
        let (state, agent, text) = match (&self.kid.failed, last) {
            (Some(e), _) => ("failed", lead, e.clone()),
            (None, Some((agent, text))) => ("done", agent, text),
            (None, None) => ("done", lead, "0 text from its agents".into()),
        };
        let text: String = text.chars().take(KID_TEXT).collect();
        let _ = super::client::call(&main, &json!({"op": "sub_done", "id": self.id, "title": t.title, "agent": agent, "state": state, "text": text}));
        let _ = store::delete(&self.id);
        self.stopping = true;
    }

    /// Every 5 s, while sub-threads of this thread run: one that is gone (its thread deleted
    /// without its end) or whose host is down KID_LOST after its start ends as failed (thread
    /// deleted); the held messages go once 0 runs. With or without an open view.
    fn kids_check(&mut self) {
        if self.kids_at.elapsed() < Duration::from_secs(5) {
            return;
        }
        self.kids_at = Instant::now();
        let Ok(t) = self.thread() else { return };
        for id in t.kids {
            let why = match store::load(&id) {
                None => Some((String::new(), String::new(), "the sub-thread is gone (deleted before its end)")),
                Some(k) if !super::client::alive(&id) && super::now_ms().saturating_sub(k.created) > KID_LOST => {
                    let _ = store::delete(&id);
                    Some((k.title, k.agents.first().cloned().unwrap_or_default(), "its host ended before its agents were done"))
                }
                Some(_) => None,
            };
            if let Some((title, agent, text)) = why {
                let _ = self.cmd(&json!({"op": "sub_done", "id": id, "title": title, "agent": agent, "state": "failed", "text": text}));
            }
        }
        self.deliver_later();
    }

    /// Sub-threads of this thread run: the host stays (it holds messages, it watches them).
    fn has_kids(&self) -> bool {
        self.thread().is_ok_and(|t| !t.kids.is_empty())
    }

    /// A flow line of AGENT's flow in the log (the event is the agent's). Steps and waits name
    /// their flow after the mark (`▶ chore · step 1/3 implement · @ADA`).
    fn flow_log(&self, agent: &str, op: &str, text: String) {
        let name = self.thread().ok().and_then(|t| t.flow_of(agent)).map(|f| f.title).filter(|n| !n.is_empty());
        let text = match (op, name, text.split_once(' ')) {
            ("step" | "wait", Some(n), Some((mark, rest))) => format!("{mark} {n} · {rest}"),
            _ => text,
        };
        self.log(Some(agent), Kind::Flow { op: op.into(), text });
    }

    /// The current node of AGENT's flow starts: the agent gets its text. CALLER: the agent whose
    /// `flow_step` led here (its text goes back in the tool result, not as a message). NOTE: the
    /// owner's words on top. Returns the reply for the caller.
    fn flow_enter(&mut self, agent: &str, caller: Option<&str>, note: Option<&str>) -> Result<Value, String> {
        let (t, mut st, f, nodes) = self.flow_ctx(agent)?;
        if st.finished {
            return self.flow_finished(agent, caller);
        }
        let Some(n) = nodes.get(st.node) else { return Ok(moved_reply(None, None, None)) };
        self.flow_log(agent, "step", format!("▶ step {}/{} {} · @{agent}", st.node + 1, nodes.len(), n.title));
        let mut msg = super::flows::run::step_message(&f.name, &nodes, st.node, &st, None);
        if let Some(n) = note.filter(|n| !n.trim().is_empty()) {
            msg = format!("[the owner] {}\n\n{msg}", n.trim());
        }
        // a thread with 0 message yet: the step goes with the task (the `prompt` op)
        let started = !t.sessions.is_empty() || store::events_tail(&self.id, 1 << 20).iter().any(|e| matches!(&e.kind, Kind::Prompt { .. }));
        if !started {
            return Ok(moved_reply(None, None, None));
        }
        st.sent = true;
        self.flow_save(agent, &st)?;
        if caller == Some(agent) {
            return Ok(moved_reply(Some(msg), None, None));
        }
        self.later.push((agent.to_string(), msg));
        Ok(moved_reply(None, Some(agent.to_string()), None))
    }

    /// AGENT's flow ended: the queued switch starts, else the end in the log.
    fn flow_finished(&mut self, agent: &str, caller: Option<&str>) -> Result<Value, String> {
        let t = self.thread()?;
        let st = t.flow_of(agent).unwrap_or_default();
        if let Some(sw) = st.queued.clone() {
            return self.flow_apply(agent, &sw, caller);
        }
        self.flow_log(agent, "finished", format!("✓ flow {} of @{agent} finished", st.title));
        Ok(json!({"text": null, "handed": null, "wait": null, "finished": true}))
    }

    /// AGENT's flow switches to SW: the new flow's state, its start node enters.
    fn flow_apply(&mut self, agent: &str, sw: &super::flows::run::Switch, caller: Option<&str>) -> Result<Value, String> {
        let t = self.thread()?;
        if !t.agents.iter().any(|a| a == agent) {
            return Err(format!("@{agent} is not on the thread"));
        }
        let lib = super::flows::Library::load(Some(&t.work_dir()));
        let f = lib.flow(&sw.flow).cloned().ok_or_else(|| format!("flow {} not found", sw.flow))?;
        let st = super::flows::run::FlowState::new(&f, &lib, sw.start, super::now_ms());
        let old = t.flow_of(agent);
        let from = old.as_ref().filter(|o| !o.finished || o.queued.is_some()).map(|o| o.title.clone());
        self.flow_save(agent, &st)?;
        let nodes = super::flows::resolve(&f, &lib);
        let at = nodes.get(st.node).map(|n| n.title.clone()).unwrap_or_default();
        match from {
            Some(o) if o != f.name || old.as_ref().is_some_and(|o| o.flow != f.slug) => self.flow_log(agent, "switch", format!("flow of @{agent}: {o} → {} (start: {at})", f.name)),
            _ => self.flow_log(agent, "set", format!("flow of @{agent}: {} (start: {at})", f.name)),
        }
        self.flow_enter(agent, caller, None)
    }

    /// AGENT reached a tool boundary (or its turn ended): a pending `now` switch of its flow stops
    /// its running turn and applies.
    fn flow_boundary(&mut self, agent: &str) {
        let Ok(t) = self.thread() else { return };
        let Some(sw) = t.flow_of(agent).and_then(|s| s.pending) else { return };
        if let Some(l) = self.sessions.get_mut(agent).filter(|l| l.busy)
            && l.session.interrupt().is_ok()
        {
            l.interrupted = true;
        }
        if self.sessions.get(agent).is_some_and(|l| l.interrupted) {
            self.asks.retain(|_, (a, _)| a != agent);
        }
        if let Err(e) = self.flow_apply(agent, &sw, None) {
            self.flow_log(agent, "warn", format!("switch to {}: {e}", sw.flow));
        }
        self.deliver_later();
    }

    /// The flows of thread T whose current step went to 0 agent yet (a flow set with the task):
    /// each step goes with TEXT under it (the agent's 1st message). Agents in TO: (agent, step)
    /// for the caller to send in place of TEXT; the others get theirs on their own. Marks the steps
    /// sent.
    fn flow_unsent(&mut self, t: &store::Thread, to: &[String], text: &str) -> Vec<(String, String)> {
        let mut out = Vec::new();
        for (agent, st) in t.flow_map() {
            if st.sent || st.finished || st.wait.is_some() {
                continue;
            }

            let Ok((_, mut st, f, nodes)) = self.flow_ctx(&agent) else { continue };
            let Some(n) = nodes.get(st.node) else { continue };
            let msg = super::flows::run::step_message(&f.name, &nodes, st.node, &st, Some(text));
            self.flow_log(&agent, "step", format!("▶ step {}/{} {} · @{agent}", st.node + 1, nodes.len(), n.title));
            st.sent = true;
            if self.flow_save(&agent, &st).is_err() {
                continue;
            }
            if to.contains(&agent) {
                out.push((agent, msg));
            } else {
                self.later.push((agent, msg));
            }
        }
        out
    }

    /// The `flow_*` ops. Owner ops name the flow's agent (`agent`; see `flow_agent`).
    fn flow_cmd(&mut self, op: &str, c: &Value) -> Result<Value, String> {
        let s = |k: &str| c[k].as_str().unwrap_or_default().to_string();
        let now = super::now_ms();
        match op {
            // the owner sets / switches an agent's flow: MODE `now` (at the agent's next tool
            // boundary), `after` (once its current flow ends); 0 flow running: at once
            "flow_set" => {
                let agent = self.flow_agent(c)?;
                let sw = super::flows::run::Switch { flow: s("flow"), start: c["start"].as_u64().unwrap_or(0) as usize };
                let t = self.thread()?;
                let lib = super::flows::Library::load(Some(&t.work_dir()));
                let f = lib.flow(&sw.flow).ok_or_else(|| format!("flow {} not found", sw.flow))?;
                let sw = super::flows::run::Switch { flow: f.slug.clone(), ..sw };
                let name = f.name.clone();
                let cur = t.flow_of(&agent);
                let running = cur.as_ref().is_some_and(|f| !f.finished);
                match (running, s("mode").as_str()) {
                    (true, "after") => {
                        let mut st = cur.unwrap_or_default();
                        st.queued = Some(sw);
                        self.flow_save(&agent, &st)?;
                        self.flow_log(&agent, "queued", format!("queued: flow {name} after {} (@{agent})", st.title));
                        Ok(json!({"queued": name}))
                    }
                    (true, _) if self.sessions.get(&agent).is_some_and(|l| l.busy) => {
                        let mut st = cur.unwrap_or_default();
                        st.pending = Some(sw);
                        self.flow_save(&agent, &st)?;
                        self.flow_log(&agent, "pending", format!("switch @{agent} to {name}: its running turn stops at its next tool boundary"));
                        Ok(json!({"pending": name}))
                    }
                    _ => self.flow_apply(&agent, &sw, None),
                }
            }
            "flow_cancel" => {
                let agent = self.flow_agent(c)?;
                let t = self.thread()?;
                let mut st = t.flow_of(&agent).ok_or_else(|| format!("@{agent} has 0 flow"))?;
                let what = st.pending.take().or(st.queued.take()).ok_or("0 switch to cancel")?;
                self.flow_save(&agent, &st)?;
                self.flow_log(&agent, "set", format!("switch of @{agent} to {} canceled", what.flow));
                Ok(json!({}))
            }
            // an agent reports the current step of its flow done (`flow_step` tool)
            "flow_step" => {
                let agent = s("agent");
                let (_, mut st, _, nodes) = self.flow_ctx(&agent).map_err(|_| format!("@{agent} has 0 flow: 0 step to report"))?;
                if st.finished {
                    return Err("the flow finished: 0 step left".into());
                }
                if st.pending.is_some() {
                    return Err("the owner switches the flow: stop and wait".into());
                }
                let n = nodes.get(st.node).cloned().ok_or("0 current step")?;
                let pass = c["pass_gate"].as_bool().unwrap_or(false);
                if pass && roster::get(&agent).is_none_or(|a| a.mode != "autopilot") {
                    return Err("pass_gate: only an agent on autopilot passes a gate; wait for the owner".into());
                }
                let note = s("note");
                let moved = st.step_done(&nodes, c["again"].as_bool().unwrap_or(false), pass, now);
                self.flow_save(&agent, &st)?;
                let tail = if note.is_empty() { String::new() } else { format!(" · {note}") };
                match moved {
                    super::flows::run::Moved::Wait(w) => {
                        let what = match w {
                            super::flows::run::Wait::Gate => {
                                let outs: Vec<&str> = std::iter::once("approve").chain(n.edges.iter().map(|(k, _)| k.as_str())).collect();
                                format!("⏸ gate {}: waits for the owner ({}){tail}", n.title, outs.join(" · "))
                            }
                            super::flows::run::Wait::LoopMax => format!("⏸ loop of {}: max rounds reached · 1 more round, or move on{tail}", n.title),
                        };
                        self.flow_log(&agent, "wait", what);
                        Ok(moved_reply(None, None, Some("owner")))
                    }
                    super::flows::run::Moved::Finished => {
                        self.flow_log(&agent, "step", format!("✓ step {}/{} {}{tail}", nodes.len(), nodes.len(), n.title));
                        self.flow_finished(&agent, Some(&agent))
                    }
                    super::flows::run::Moved::To(_) => {
                        if pass && n.gate {
                            self.flow_log(&agent, "warn", format!("gate {} passed by @{agent} (autopilot){tail}", n.title));
                        } else if !tail.is_empty() {
                            self.flow_log(&agent, "step", format!("✓ {}{tail}", n.title));
                        }
                        self.flow_enter(&agent, Some(&agent), None)
                    }
                }
            }
            // the owner answers the wait: CHOICE `approve` / an outcome (gate), `again` / `next`
            // (loop max); NOTE: the owner's words to the agent
            "flow_answer" => {
                let agent = self.flow_agent(c)?;
                let (_, mut st, _, nodes) = self.flow_ctx(&agent)?;
                let choice = s("choice");
                let title = nodes.get(st.node).map(|n| n.title.clone()).unwrap_or_default();
                st.answer(&nodes, &choice, now)?;
                self.flow_save(&agent, &st)?;
                self.flow_log(&agent, "answer", format!("owner: {choice} · {title}"));
                if st.finished {
                    return self.flow_finished(&agent, None);
                }
                self.flow_enter(&agent, None, Some(&s("note")))
            }
            "flow_skip" | "flow_jump" => {
                let agent = self.flow_agent(c)?;
                let (_, mut st, _, nodes) = self.flow_ctx(&agent)?;
                let from = nodes.get(st.node).map(|n| n.title.clone()).unwrap_or_default();
                if op == "flow_skip" {
                    st.skip(&nodes, now);
                    self.flow_save(&agent, &st)?;
                    self.flow_log(&agent, "skip", format!("owner skipped {from}"));
                } else {
                    let to = c["node"].as_u64().ok_or("flow_jump: `node` required")? as usize;
                    st.jump(&nodes, to, now)?;
                    self.flow_save(&agent, &st)?;
                    self.flow_log(&agent, "jump", format!("owner: {from} → {}", nodes[to].title));
                }
                if st.finished {
                    return self.flow_finished(&agent, None);
                }
                self.flow_enter(&agent, None, None)
            }
            "flow_clear" => {
                let agent = self.flow_agent(c)?;
                self.flow_log(&agent, "clear", format!("flow of @{agent} removed"));
                store::update(&self.id, |t| t.set_flow(&agent, None))?;
                Ok(json!({}))
            }
            // an agent proposes another flow for itself: a line for the owner
            "flow_suggest" => {
                let t = self.thread()?;
                let lib = super::flows::Library::load(Some(&t.work_dir()));
                let f = lib.flow(&s("flow")).ok_or_else(|| format!("flow {} not found. flows: {}", s("flow"), lib.flows.iter().map(|f| f.slug.as_str()).collect::<Vec<_>>().join(", ")))?;
                self.flow_log(&s("agent"), "suggest", format!("@{} suggests flow {} · {} · agents box (tab) → flow to switch", s("agent"), f.name, s("reason")));
                Ok(json!({}))
            }
            _ => Err(format!("unknown op {op:?}")),
        }
    }
}

/// The end of a turn whose session ended (stop, crash, exit): 0 cost known.
fn session_ended() -> Kind {
    Kind::TurnEnd { cost: 0.0, input: 0, output: 0, cache_read: 0, cache_write: 0, ctx: 0, window: 0, ms: 0, error: Some("session ended".into()) }
}

/// SIGTERM / SIGINT / SIGHUP came: the host loop ends its sessions and exits.
static SIGNALED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

extern "C" fn on_signal(_: libc::c_int) {
    SIGNALED.store(true, std::sync::atomic::Ordering::Relaxed);
}

fn catch_signals() {
    for s in [libc::SIGTERM, libc::SIGINT, libc::SIGHUP] {
        // SAFETY: the handler only stores to an atomic (async-signal-safe).
        unsafe { libc::signal(s, on_signal as *const () as libc::sighandler_t) };
    }
}

fn serve(conn: UnixStream, tx: Sender<Input>) {
    let Ok(mut w) = conn.try_clone() else { return };
    for line in BufReader::new(conn).lines().map_while(Result::ok) {
        let reply = match serde_json::from_str::<Value>(&line) {
            // a view subscribes: this connection carries the host's lines from now on
            Ok(c) if c["op"] == "subscribe" => {
                let (stx, srx) = mpsc::channel::<String>();
                if tx.send(Input::Subscribe(stx)).is_err() || writeln!(w, "{}", json!({"ok": true})).is_err() {
                    return;
                }
                for l in srx {
                    if writeln!(w, "{l}").is_err() {
                        return;
                    }
                }
                return;
            }
            Ok(c) => {
                let (rtx, rrx) = mpsc::channel();
                if tx.send(Input::Cmd(c, rtx)).is_err() {
                    return;
                }
                rrx.recv().unwrap_or_else(|_| json!({"ok": false, "error": "host stopped"}))
            }
            Err(e) => json!({"ok": false, "error": format!("bad json: {e}")}),
        };
        if writeln!(w, "{reply}").is_err() {
            return;
        }
    }
}

/// Bind the socket of thread ID. Err: another host serves it (or the bind failed).
/// The lock file (held for the host's life) closes the race of 2 hosts started at once.
fn bind(id: &str) -> Result<(UnixListener, std::fs::File), String> {
    let path = socket(id);
    std::fs::create_dir_all(super::run_dir()).map_err(|e| e.to_string())?;
    let lock = super::lock(&super::run_dir().join(format!("{id}.lock")), false).map_err(|_| "a host serves this thread".to_string())?;
    if UnixStream::connect(&path).is_ok() {
        return Err("a host serves this thread".into());
    }
    let _ = std::fs::remove_file(&path);
    let l = UnixListener::bind(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok((l, lock))
}

/// Run the host of thread ID until idle or `stop`.
/// Worktree WT stays on BRANCH (`crate::branchpin`); a failure: in the host's log.
fn pin_branch(wt: &Path, branch: &str) {
    if let Err(e) = crate::branchpin::pin(wt, branch) {
        eprintln!("codehead host: pin {} to {branch}: {e}", wt.display());
    }
}

pub fn run(id: &str) -> ! {
    let Some(t) = store::load(id) else {
        eprintln!("codehead host: no thread {id}");
        std::process::exit(2)
    };
    let (listener, _lock) = match bind(&t.id) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("codehead host {}: {e}", t.id);
            std::process::exit(0)
        }
    };
    // a plan window ends: its next one starts 1 min after (agents work without the screen too)
    super::warm::watch();
    let (tx, rx) = mpsc::channel::<Input>();
    let (htx, hrx) = mpsc::channel::<(String, Out)>();
    let fwd = tx.clone();
    std::thread::spawn(move || {
        for (a, o) in hrx {
            if fwd.send(Input::Harness(a, o)).is_err() {
                return;
            }
        }
    });
    let acc = tx.clone();
    std::thread::spawn(move || {
        for conn in listener.incoming().flatten() {
            let tx = acc.clone();
            std::thread::spawn(move || serve(conn, tx));
        }
    });
    let exe = super::exe();
    let mut h = Host { id: t.id.clone(), exe, sessions: HashMap::new(), asks: HashMap::new(), harness_tx: htx, last: Instant::now(), stopping: false, subs: Default::default(), live: HashMap::new(), turn_use: HashMap::new(), setup_running: false, queue: Vec::new(), later: Vec::new(), subagents: Default::default(), sub_scan: Instant::now(), held: Vec::new(), kid: Default::default(), kids_at: Instant::now(), resume: t.resume.clone().into_iter().collect() };
    catch_signals();
    h.close_open_turns();
    // subagents the log has open ran in a process that is gone
    let (subs, lost) = super::subagents::Tracker::from_log(&store::events(&t.id));
    h.subagents = subs;
    for (a, k) in lost {
        h.log(Some(&a), k);
    }
    // ended subagents resumed while 0 host ran (or an older one missed it)
    if h.subagents.resumable() {
        h.scan_subs();
    }
    // the hooks of the repo change: each host start pins the worktree again
    if let (Some(wt), Some(b), None) = (t.worktree.clone(), t.branch.clone(), &t.worktree_pending) {
        std::thread::spawn(move || pin_branch(Path::new(&wt), &b));
    }
    if t.worktree_pending.is_some() {
        h.start_worktree(&t, tx.clone());
    } else if t.setup.as_deref() == Some("pending") {
        h.start_setup(&t, tx.clone());
    } else if t.pull {
        h.start_pull(&t, tx.clone());
    }
    h.kickoff(&t);
    loop {
        // 1 s: a signal ends the host within a second
        match rx.recv_timeout(Duration::from_secs(1)) {
            Ok(Input::Cmd(c, reply)) => {
                let r = match h.cmd(&c) {
                    Ok(mut v) => {
                        v["ok"] = json!(true);
                        v
                    }
                    Err(e) => json!({"ok": false, "error": e}),
                };
                let _ = reply.send(r);
                h.deliver_later();
            }
            Ok(Input::Subscribe(s)) => h.subscribe(s),
            Ok(Input::Harness(a, o)) => h.harness(a, o),
            Ok(Input::Setup(label, ok, out)) => h.setup_done(label, ok, out),
            Ok(Input::Pull(ok, out)) => h.pull_done(ok, out),
            Ok(Input::Worktree(r)) => h.worktree_done(r, tx.clone()),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
        if h.stopping || SIGNALED.load(std::sync::atomic::Ordering::Relaxed) {
            break;
        }
        // running ones: every 2 s; ended ones a message can resume: every 10 s
        let every = Duration::from_secs(if h.subagents.any() { 2 } else { 10 });
        if (h.subagents.any() || h.subagents.resumable()) && h.sub_scan.elapsed() >= every {
            h.scan_subs();
        }
        h.run_resumes();
        h.kid_check();
        h.kids_check();
        let busy = h.sessions.values().any(|l| l.busy);
        // running subagents keep the host (and their harness process) alive; a resume waits too
        if !busy && !h.subagents.any() && !h.setup_running && h.asks.is_empty() && h.resume.is_empty() && h.last.elapsed() >= IDLE && !h.has_kids() {
            // a monitor or background command runs: the host (and its harness) stays
            if !crate::monitors::watching(&h.id, &h.sessions.keys().cloned().collect::<Vec<_>>()) {
                break;
            }
            h.last = Instant::now();
        }
        // the thread was deleted: end with it
        if !store::thread_dir(&h.id).join("thread.json").is_file() {
            break;
        }
    }
    h.stop();
    std::process::exit(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_flow_goes_with_the_task_moves_on_reports_and_waits_at_gates() {
        let _env = crate::engine::ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let t = store::Thread::new("flow test", "task", Path::new("/tmp"), vec!["ada".into()]);
        std::fs::create_dir_all(store::thread_dir(&t.id)).unwrap();
        t.save().unwrap();
        let (htx, _hrx) = mpsc::channel();
        let mut h = Host { id: t.id.clone(), exe: PathBuf::new(), sessions: HashMap::new(), asks: HashMap::new(), harness_tx: htx, last: Instant::now(), stopping: false, subs: Default::default(), live: HashMap::new(), turn_use: HashMap::new(), setup_running: false, queue: Vec::new(), later: Vec::new(), subagents: Default::default(), sub_scan: Instant::now(), held: Vec::new(), kid: Default::default(), kids_at: Instant::now(), resume: HashMap::new() };
        let flow = || store::load(&t.id).unwrap().flow_of("ada").unwrap();
        // set before the 1st message: the step waits for the task
        h.cmd(&json!({"op": "flow_set", "flow": "chore"})).unwrap();
        assert!(h.later.is_empty() && !flow().sent);
        h.cmd(&json!({"op": "prompt", "text": "do it"})).unwrap();
        let (to, m) = h.later.pop().unwrap();
        assert_eq!(to, "ada");
        assert!(m.starts_with("[codehead flow: chore · step 1/3: implement]") && m.ends_with("do it"), "{m}");
        assert!(flow().sent);
        // only the flow's agent reports it (bob has 0 flow); the reply holds the next step
        assert!(h.cmd(&json!({"op": "flow_step", "agent": "bob"})).is_err());
        let r = h.cmd(&json!({"op": "flow_step", "agent": "ada"})).unwrap();
        assert!(r["text"].as_str().unwrap().contains("step 2/3: verify"));
        // a switch after the flow: the queued flow starts when it ends
        h.cmd(&json!({"op": "flow_set", "flow": "new feature", "mode": "after"})).unwrap();
        assert_eq!(flow().queued.unwrap().flow, "new-feature");
        h.cmd(&json!({"op": "flow_step", "agent": "ada"})).unwrap();
        let r = h.cmd(&json!({"op": "flow_step", "agent": "ada"})).unwrap();
        assert!(r["text"].as_str().unwrap().contains("[codehead flow: new feature · step 1/9: scrutinize-task]"), "{r}");
        assert_eq!(flow().flow, "new-feature");
        // a gate: the agent waits, the owner's outcome picks the edge
        h.cmd(&json!({"op": "flow_jump", "node": 3})).unwrap();
        let (_, m) = h.later.pop().unwrap();
        assert!(m.contains("plan approval") && m.contains("Gate:"));
        let r = h.cmd(&json!({"op": "flow_step", "agent": "ada"})).unwrap();
        assert_eq!(r["wait"], "owner");
        assert!(h.cmd(&json!({"op": "flow_step", "agent": "ada", "pass_gate": true})).is_err(), "ada is 0 autopilot agent of the roster");
        h.cmd(&json!({"op": "flow_answer", "choice": "changes", "note": "split it"})).unwrap();
        let (_, m) = h.later.pop().unwrap();
        assert!(m.starts_with("[the owner] split it") && m.contains("step 3/9: plan"), "{m}");
        // peer review: the flow's own agent asks another agent (roles are gone)
        h.cmd(&json!({"op": "flow_jump", "node": 6})).unwrap();
        let (to, m) = h.later.pop().unwrap();
        assert!(to == "ada" && m.contains("peer-review") && m.contains("list_agents"), "{m}");
        let log: Vec<String> = store::events(&t.id).into_iter().filter_map(|e| match e.kind { Kind::Flow { text, .. } => Some(text), _ => None }).collect();
        assert!(log.iter().any(|l| l.starts_with("⏸ new feature · gate plan approval: waits for the owner (approve · changes)")), "{log:?}");
        assert!(log.iter().any(|l| l == "▶ chore · step 1/3 implement · @ada"), "steps name their flow: {log:?}");
        let _ = std::fs::remove_dir_all(store::thread_dir(&t.id));
    }

    /// 2 agents, 2 flows: each step goes to its flow's agent (with the task at the start), each
    /// `flow_step` moves only its own flow, owner ops name the agent once 2+ flows run.
    #[test]
    fn each_agent_runs_its_own_flow() {
        let _env = crate::engine::ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let t = store::Thread::new("2 flows", "task", Path::new("/tmp"), vec!["ada".into(), "zed".into()]);
        std::fs::create_dir_all(store::thread_dir(&t.id)).unwrap();
        t.save().unwrap();
        let (htx, _hrx) = mpsc::channel();
        let mut h = Host { id: t.id.clone(), exe: PathBuf::new(), sessions: HashMap::new(), asks: HashMap::new(), harness_tx: htx, last: Instant::now(), stopping: false, subs: Default::default(), live: HashMap::new(), turn_use: HashMap::new(), setup_running: false, queue: Vec::new(), later: Vec::new(), subagents: Default::default(), sub_scan: Instant::now(), held: Vec::new(), kid: Default::default(), kids_at: Instant::now(), resume: HashMap::new() };
        let flow = |a: &str| store::load(&t.id).unwrap().flow_of(a);
        h.cmd(&json!({"op": "flow_set", "flow": "chore", "agent": "ada"})).unwrap();
        h.cmd(&json!({"op": "flow_set", "flow": "code review", "agent": "zed"})).unwrap();
        assert!(h.cmd(&json!({"op": "flow_set", "flow": "chore", "agent": "bob"})).is_err(), "bob is not on the thread");

        // the task goes to the lead; each agent gets its step 1 with the task under it
        h.cmd(&json!({"op": "prompt", "text": "do it"})).unwrap();
        let ada = h.later.iter().find(|(a, _)| a == "ada").map(|(_, m)| m.clone()).unwrap();
        let zed = h.later.iter().find(|(a, _)| a == "zed").map(|(_, m)| m.clone()).unwrap();
        assert!(ada.starts_with("[codehead flow: chore · step 1/3: implement]") && ada.ends_with("do it"), "{ada}");
        assert!(zed.starts_with("[codehead flow: code review · step 1/2: review-diff]") && zed.ends_with("do it"), "{zed}");
        assert_eq!(h.later.len(), 2);

        // zed reports its step: only its flow moves
        let r = h.cmd(&json!({"op": "flow_step", "agent": "zed"})).unwrap();
        assert!(r["text"].as_str().unwrap().contains("step 2/2: report"), "{r}");
        assert_eq!((flow("ada").unwrap().node, flow("zed").unwrap().node), (0, 1));

        // owner ops: 2 flows → the agent is named
        let e = h.cmd(&json!({"op": "flow_skip"})).unwrap_err();
        assert!(e.contains("@ada") && e.contains("@zed"), "{e}");
        h.cmd(&json!({"op": "flow_skip", "agent": "ada"})).unwrap();
        assert_eq!(flow("ada").unwrap().node, 1);

        // the flow lines are the agent's events
        let flows: Vec<(Option<String>, String)> = store::events(&t.id).into_iter().filter_map(|e| match e.kind { Kind::Flow { text, .. } => Some((e.agent, text)), _ => None }).collect();
        assert!(flows.iter().any(|(a, l)| a.as_deref() == Some("zed") && l.starts_with("▶ code review · step 1/2")), "{flows:?}");

        // zed's flow goes, ada's stays
        h.cmd(&json!({"op": "flow_clear", "agent": "zed"})).unwrap();
        assert!(flow("zed").is_none() && flow("ada").is_some());

        // bob replaces ada: ada's flow goes to bob, its step again with bob's 1st message
        h.later.clear();
        h.cmd(&json!({"op": "leave", "agent": "ada", "to": "bob"})).unwrap();
        assert!(flow("ada").is_none() && flow("bob").is_some_and(|f| !f.sent && f.node == 1));
        h.cmd(&json!({"op": "prompt", "to": ["bob"], "text": "take over"})).unwrap();
        let bob = h.later.iter().find(|(a, _)| a == "bob").map(|(_, m)| m.clone()).unwrap();
        assert!(bob.starts_with("[codehead flow: chore · step 2/3") && bob.ends_with("take over"), "{bob}");

        // zed leaves (0 replacement): 0 flow left
        h.cmd(&json!({"op": "leave", "agent": "zed"})).unwrap();
        assert!(flow("zed").is_none());
        let _ = std::fs::remove_dir_all(store::thread_dir(&t.id));
    }

    /// An older version's thread: its 1 flow is the lead's; a save keeps `flow` = the lead's.
    /// A rename keeps the flow with its agent: the lead of an older thread (its 1 flow) and a
    /// non-lead; 0 copy under the old name.
    #[test]
    fn a_renamed_agent_keeps_its_flow() {
        let mut t = store::Thread::new("rename", "task", Path::new("/tmp"), vec!["ada".into(), "zed".into()]);
        t.flow = Some(crate::engine::flows::run::FlowState { flow: "chore".into(), ..Default::default() });
        t.set_flow("zed", Some(crate::engine::flows::run::FlowState { flow: "code-review".into(), ..Default::default() }));
        t.rename_agent("ada", "nova");
        assert_eq!(t.agents, ["nova", "zed"]);
        assert_eq!(t.flow_map().keys().cloned().collect::<Vec<_>>(), ["nova", "zed"]);
        assert_eq!(t.flow_of("nova").map(|f| f.flow), Some("chore".into()));
        t.rename_agent("zed", "rex");
        assert_eq!(t.flow_of("rex").map(|f| f.flow), Some("code-review".into()));
        assert!(t.flow_of("zed").is_none() && t.flow_of("ada").is_none());
    }

    #[test]
    fn an_old_thread_flow_belongs_to_the_lead() {
        let mut t = store::Thread::new("old", "task", Path::new("/tmp"), vec!["ada".into(), "zed".into()]);
        t.flow = Some(crate::engine::flows::run::FlowState { flow: "chore".into(), node: 2, ..Default::default() });
        assert_eq!(t.flow_of("ada").map(|f| f.node), Some(2));
        assert!(t.flow_of("zed").is_none());
        t.set_flow("zed", Some(crate::engine::flows::run::FlowState { flow: "code-review".into(), ..Default::default() }));
        assert_eq!((t.flows.len(), t.flow.as_ref().map(|f| f.flow.as_str())), (2, Some("chore")));
        t.set_flow("ada", None);
        assert!(t.flow.is_none() && t.flow_of("ada").is_none() && t.flow_of("zed").is_some());
    }

    /// The thread's host watches its sub-threads (0 view needed): one deleted without its end and
    /// one whose host is down 20 s after its start end as failed; the held messages go.
    #[test]
    fn the_host_ends_gone_and_lost_sub_threads() {
        let _env = crate::engine::ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let mut lost = store::Thread::new("lost one", "x", Path::new("/tmp"), vec!["ada".into()]);
        lost.created = super::super::now_ms() - 60_000;
        std::fs::create_dir_all(store::thread_dir(&lost.id)).unwrap();
        lost.save().unwrap();
        let mut young = store::Thread::new("young one", "x", Path::new("/tmp"), vec!["ada".into()]);
        young.of = Some("x".into());
        std::fs::create_dir_all(store::thread_dir(&young.id)).unwrap();
        young.save().unwrap();
        let mut t = store::Thread::new("main", "task", Path::new("/tmp"), vec!["ada".into()]);
        t.kids = vec!["gone-kid".into(), lost.id.clone(), young.id.clone()];
        std::fs::create_dir_all(store::thread_dir(&t.id)).unwrap();
        t.save().unwrap();
        let (htx, _hrx) = mpsc::channel();
        let mut h = Host { id: t.id.clone(), exe: PathBuf::new(), sessions: HashMap::new(), asks: HashMap::new(), harness_tx: htx, last: Instant::now(), stopping: false, subs: Default::default(), live: HashMap::new(), turn_use: HashMap::new(), setup_running: false, queue: Vec::new(), later: Vec::new(), subagents: Default::default(), sub_scan: Instant::now(), held: Vec::new(), kid: Default::default(), kids_at: Instant::now(), resume: HashMap::new() };
        assert!(h.has_kids());

        // 5 s between looks
        h.kids_check();
        assert_eq!(store::load(&t.id).unwrap().kids.len(), 3);
        h.kids_at = Instant::now() - Duration::from_secs(6);
        h.kids_check();

        // the gone one and the lost one end; the young one (host not up yet) runs on
        assert_eq!(store::load(&t.id).unwrap().kids, [young.id.clone()]);
        assert!(store::load(&lost.id).is_none(), "the lost one's thread goes");
        let ends: Vec<(String, String)> = store::events(&t.id).into_iter().filter_map(|e| match e.kind { Kind::SubThread { state, text, .. } => Some((state, text)), _ => None }).collect();
        assert_eq!(ends.len(), 2, "{ends:?}");
        assert!(ends.iter().all(|(s, _)| s == "failed") && ends[1].1.contains("host ended"), "{ends:?}");
        for id in [&t.id, &young.id] {
            let _ = std::fs::remove_dir_all(store::thread_dir(id));
        }
    }

    /// A running sub-thread holds the thread's messages (same checkout); its end logs its result
    /// and lets them go; an end of a sub-thread not running: 0 effect.
    #[test]
    fn a_sub_thread_holds_messages_until_it_reports_its_end() {
        let _env = crate::engine::ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let t = store::Thread::new("main", "task", Path::new("/tmp"), vec!["ada".into()]);
        std::fs::create_dir_all(store::thread_dir(&t.id)).unwrap();
        t.save().unwrap();
        let (htx, _hrx) = mpsc::channel();
        let mut h = Host { id: t.id.clone(), exe: PathBuf::new(), sessions: HashMap::new(), asks: HashMap::new(), harness_tx: htx, last: Instant::now(), stopping: false, subs: Default::default(), live: HashMap::new(), turn_use: HashMap::new(), setup_running: false, queue: Vec::new(), later: Vec::new(), subagents: Default::default(), sub_scan: Instant::now(), held: Vec::new(), kid: Default::default(), kids_at: Instant::now(), resume: HashMap::new() };

        h.cmd(&json!({"op": "sub_start", "id": "kid1", "title": "commit it", "text": "@ada · commit it"})).unwrap();
        assert_eq!(store::load(&t.id).unwrap().kids, ["kid1"]);

        // 0 harness spawned: the message waits
        h.deliver("ada", "next task").unwrap();
        h.deliver("ada", "and more").unwrap();
        assert_eq!(h.held.len(), 2);

        // an unknown end: 0 log, still held
        h.cmd(&json!({"op": "sub_done", "id": "other", "state": "done", "text": "x"})).unwrap();
        assert_eq!(h.held.len(), 2);

        h.cmd(&json!({"op": "sub_done", "id": "kid1", "title": "commit it", "agent": "ada", "state": "done", "text": "3 commits"})).unwrap();
        // its end: a message of its agent
        assert_eq!(store::events(&t.id).last().and_then(|e| e.agent.clone()).as_deref(), Some("ada"));
        assert!(store::load(&t.id).unwrap().kids.is_empty() && h.held.is_empty());
        assert_eq!(h.later, [("ada".to_string(), "next task".to_string()), ("ada".to_string(), "and more".to_string())]);
        let subs: Vec<(String, String)> = store::events(&t.id).into_iter().filter_map(|e| match e.kind { Kind::SubThread { state, text, .. } => Some((state, text)), _ => None }).collect();
        assert_eq!(subs.iter().map(|s| s.0.as_str()).collect::<Vec<_>>(), ["running", "held", "done"], "1 held line, not 1 per message: {subs:?}");
        assert_eq!(subs[2].1, "3 commits");
        let _ = std::fs::remove_dir_all(store::thread_dir(&t.id));
    }

    #[test]
    fn a_prompt_replies_first_and_a_pending_pull_holds_the_first_turn() {
        let _env = crate::engine::ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let base = std::env::temp_dir().join(format!("codehead-host-pull-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let sh = |c: &str| assert!(std::process::Command::new("sh").arg("-c").arg(c).current_dir(&base).output().unwrap().status.success(), "{c}");
        let cfg = "git config user.email t@t && git config user.name t";
        sh(&format!("git init -q --bare -b main origin.git && git clone -q origin.git a && cd a && {cfg} && git commit -q --allow-empty -m 1 && git push -q origin main"));
        sh(&format!("git clone -q origin.git b && cd b && {cfg} && git commit -q --allow-empty -m 2 && git push -q origin main"));
        let a = base.join("a");
        let mut t = store::Thread::new("pull test", "task", &a, vec!["nobody-agent".into()]);
        t.pull = true;
        std::fs::create_dir_all(store::thread_dir(&t.id)).unwrap();
        t.save().unwrap();
        let (htx, _hrx) = mpsc::channel();
        let mut h = Host { id: t.id.clone(), exe: PathBuf::new(), sessions: HashMap::new(), asks: HashMap::new(), harness_tx: htx, last: Instant::now(), stopping: false, subs: Default::default(), live: HashMap::new(), turn_use: HashMap::new(), setup_running: false, queue: Vec::new(), later: Vec::new(), subagents: Default::default(), sub_scan: Instant::now(), held: Vec::new(), kid: Default::default(), kids_at: Instant::now(), resume: HashMap::new() };
        let (tx, rx) = mpsc::channel();
        h.start_pull(&t, tx);
        // the reply comes before any delivery: the message waits in LATER
        let r = h.cmd(&json!({"op": "prompt", "to": ["nobody-agent"], "text": "go"})).unwrap();
        assert_eq!(r["to"], json!(["nobody-agent"]));
        assert_eq!((h.later.len(), h.queue.len()), (1, 0));
        // after the reply: the pull still runs, so the message waits for it
        h.deliver_later();
        assert_eq!((h.later.len(), h.queue.len()), (0, 1));
        let Ok(Input::Pull(ok, out)) = rx.recv_timeout(Duration::from_secs(10)) else { panic!("0 pull end") };
        assert_eq!((ok, out.as_str()), (true, "main up to date"));
        h.pull_done(ok, out);
        assert!(!h.setup_running && h.queue.is_empty());
        assert!(!store::load(&t.id).unwrap().pull);
        let rev = |d: &str| String::from_utf8(std::process::Command::new("git").args(["-C", d, "rev-parse", "HEAD"]).current_dir(&base).output().unwrap().stdout).unwrap();
        assert_eq!(rev("a"), rev("b"), "main pulled");
        let _ = std::fs::remove_dir_all(store::thread_dir(&t.id));
        let _ = std::fs::remove_dir_all(base);
    }

    /// A new thread (`kickoff`, `worktree_pending`): the host logs the task at once, the task waits
    /// for the worktree; the worktree lands in thread.json (path, branch, port), then the task goes
    /// out. A failed worktree: the task goes with a note on it.
    #[test]
    fn a_kickoff_logs_the_task_and_a_pending_worktree_holds_it() {
        let _env = crate::engine::ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let base = std::env::temp_dir().join(format!("codehead-host-kickoff-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let sh = |c: &str| assert!(std::process::Command::new("sh").arg("-c").arg(c).current_dir(&base).output().unwrap().status.success(), "{c}");
        sh("git init -q -b main r && cd r && git -c user.email=t@t -c user.name=t commit -q --allow-empty -m 1");
        let repo = base.join("r");
        let branch = format!("kick-{}", std::process::id());
        let mut t = store::Thread::new("kickoff test", "do the thing", &repo, vec!["nobody-agent".into()]);
        t.kickoff = true;
        t.worktree_pending = Some(branch.clone());
        std::fs::create_dir_all(store::thread_dir(&t.id)).unwrap();
        t.save().unwrap();
        let (htx, _hrx) = mpsc::channel();
        let mut h = Host { id: t.id.clone(), exe: PathBuf::new(), sessions: HashMap::new(), asks: HashMap::new(), harness_tx: htx, last: Instant::now(), stopping: false, subs: Default::default(), live: HashMap::new(), turn_use: HashMap::new(), setup_running: false, queue: Vec::new(), later: Vec::new(), subagents: Default::default(), sub_scan: Instant::now(), held: Vec::new(), kid: Default::default(), kids_at: Instant::now(), resume: HashMap::new() };
        let (tx, rx) = mpsc::channel();
        h.start_worktree(&t, tx.clone());
        h.kickoff(&t);
        let prompts = |id: &str| store::events(id).into_iter().filter(|e| matches!(&e.kind, Kind::Prompt { text, .. } if text == "do the thing")).count();
        assert_eq!(prompts(&t.id), 1, "the task in the log at once");
        assert!(!store::load(&t.id).unwrap().kickoff);
        assert_eq!((h.later.len(), h.queue.len()), (0, 1), "the task waits for the worktree");
        // a 2nd host start (a crash, a restart): 0 2nd copy
        h.kickoff(&store::load(&t.id).unwrap());
        assert_eq!(prompts(&t.id), 1);
        let Ok(Input::Worktree(r)) = rx.recv_timeout(Duration::from_secs(20)) else { panic!("0 worktree end") };
        h.worktree_done(r, tx.clone());
        let after = store::load(&t.id).unwrap();
        let wt = PathBuf::from(after.worktree.clone().expect("worktree in thread.json"));
        assert!(wt.join(".git").is_file());
        assert_eq!((after.branch.as_deref(), after.worktree_pending.as_deref(), after.port.is_some()), (Some(branch.as_str()), None, true));
        assert!(!h.setup_running && h.queue.is_empty(), "the task went out (its delivery failed: 0 such agent)");
        // the branch's worktree is there: a 2nd one fails, the task goes with a note
        let mut t2 = store::Thread::new("kickoff test 2", "again", &repo, vec!["nobody-agent".into()]);
        t2.worktree_pending = Some(branch.clone());
        std::fs::create_dir_all(store::thread_dir(&t2.id)).unwrap();
        t2.save().unwrap();
        h.id = t2.id.clone();
        h.start_worktree(&t2, tx.clone());
        h.queue.push(("nobody-agent".into(), "again".into()));
        let Ok(Input::Worktree(r)) = rx.recv_timeout(Duration::from_secs(20)) else { panic!("0 worktree end") };
        assert!(r.is_err());
        h.worktree_done(r, tx);
        assert!(!h.setup_running && h.queue.is_empty() && store::load(&t2.id).unwrap().worktree_pending.is_none());
        let failed = store::events(&t2.id).into_iter().any(|e| matches!(&e.kind, Kind::ToolResult { id, ok: false, .. } if id == "worktree"));
        assert!(failed);
        let _ = std::process::Command::new("git").args(["-C", &repo.display().to_string(), "worktree", "remove", "--force", &wt.display().to_string()]).output();
        let _ = std::fs::remove_dir_all(store::thread_dir(&t.id));
        let _ = std::fs::remove_dir_all(store::thread_dir(&t2.id));
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn a_pending_setup_runs_first_and_messages_wait_for_it() {
        // the data dir stays put while this runs
        let _env = crate::engine::ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let wt = std::env::temp_dir().join(format!("codehead-host-setup-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&wt);
        std::fs::create_dir_all(wt.join(".codehead")).unwrap();
        std::fs::write(wt.join(".codehead/setup"), "echo installed on $CODEHEAD_PORT\n").unwrap();
        let mut t = store::Thread::new("setup test", "task", &wt, vec!["nobody-agent".into()]);
        t.worktree = Some(wt.display().to_string());
        t.port = Some(40090);
        t.setup = Some("pending".into());
        std::fs::create_dir_all(store::thread_dir(&t.id)).unwrap();
        t.save().unwrap();
        let (htx, _hrx) = mpsc::channel();
        let mut h = Host { id: t.id.clone(), exe: PathBuf::new(), sessions: HashMap::new(), asks: HashMap::new(), harness_tx: htx, last: Instant::now(), stopping: false, subs: Default::default(), live: HashMap::new(), turn_use: HashMap::new(), setup_running: false, queue: Vec::new(), later: Vec::new(), subagents: Default::default(), sub_scan: Instant::now(), held: Vec::new(), kid: Default::default(), kids_at: Instant::now(), resume: t.resume.clone().into_iter().collect() };
        let (tx, rx) = mpsc::channel();
        h.start_setup(&t, tx);
        assert!(h.setup_running);
        // a message now: it waits
        h.deliver("nobody-agent", "go").unwrap();
        assert_eq!(h.queue.len(), 1);
        let Ok(Input::Setup(label, ok, out)) = rx.recv_timeout(Duration::from_secs(10)) else { panic!("0 setup end") };
        assert_eq!((label.as_str(), ok, out.trim()), (".codehead/setup", true, "installed on 40090"));
        h.setup_done(label, ok, out);
        assert!(!h.setup_running && h.queue.is_empty());
        assert_eq!(store::load(&t.id).unwrap().setup.as_deref(), Some("done"));
        let kinds: Vec<String> = store::events(&t.id).iter().map(|e| format!("{:?}", e.kind).split([' ', '{', '(']).next().unwrap().to_string()).collect();
        assert!(kinds.starts_with(&["Tool".to_string(), "ToolResult".to_string()]), "{kinds:?}");
        let _ = std::fs::remove_dir_all(store::thread_dir(&t.id));
        let _ = std::fs::remove_dir_all(wt);
    }

    #[test]
    fn subagents_go_into_the_log_and_end_lost_with_their_process() {
        let _env = crate::engine::ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let t = store::Thread::new("subs", "task", Path::new("/tmp"), vec!["ada".into()]);
        std::fs::create_dir_all(store::thread_dir(&t.id)).unwrap();
        t.save().unwrap();
        let (htx, _hrx) = mpsc::channel();
        let mut h = Host { id: t.id.clone(), exe: PathBuf::new(), sessions: HashMap::new(), asks: HashMap::new(), harness_tx: htx, last: Instant::now(), stopping: false, subs: Default::default(), live: HashMap::new(), turn_use: HashMap::new(), setup_running: false, queue: Vec::new(), later: Vec::new(), subagents: Default::default(), sub_scan: Instant::now(), held: Vec::new(), kid: Default::default(), kids_at: Instant::now(), resume: t.resume.clone().into_iter().collect() };
        h.harness("ada".into(), Out::Event(Kind::Tool { id: "t1".into(), name: "Agent".into(), input: json!({"description": "scan", "run_in_background": true, "prompt": "scan it"}) }));
        h.harness("ada".into(), Out::Event(Kind::ToolResult { id: "t1".into(), ok: true, output: "Async agent launched\nagentId: a1".into() }));
        // the master's turn ended: the subagent keeps the host alive, the master counts as busy
        assert!(h.subagents.any());
        let ping = h.cmd(&json!({"op": "ping"})).unwrap();
        assert!(ping["busy"].as_array().unwrap().is_empty(), "0 session: 0 busy entry");
        h.harness("ada".into(), Out::Exit(String::new()));
        let kinds: Vec<Kind> = store::events(&t.id).into_iter().map(|e| e.kind).collect();
        assert!(matches!(&kinds[1], Kind::SubStart { id, background: true, prompt, .. } if id == "t1" && prompt == "scan it"), "{kinds:?}");
        assert!(kinds.iter().any(|k| matches!(k, Kind::SubInfo { task, .. } if task == "a1")), "{kinds:?}");
        assert!(matches!(kinds.last(), Some(Kind::SubEnd { state, .. }) if state == "lost"), "{kinds:?}");
        assert!(!h.subagents.any());
        let _ = std::fs::remove_dir_all(store::thread_dir(&t.id));
    }

    #[test]
    fn subscribers_get_live_text_then_chunks_and_wakes() {
        let (a, b) = UnixStream::pair().unwrap();
        let (tx, rx) = mpsc::channel::<Input>();
        std::thread::spawn(move || serve(a, tx));
        let mut w = b.try_clone().unwrap();
        writeln!(w, "{}", json!({"op": "subscribe"})).unwrap();
        let Ok(Input::Subscribe(s)) = rx.recv() else { panic!("0 subscribe") };
        let (htx, _hrx) = mpsc::channel();
        let mut h = Host { id: "t".into(), exe: PathBuf::new(), sessions: HashMap::new(), asks: HashMap::new(), harness_tx: htx, last: Instant::now(), stopping: false, subs: Default::default(), live: HashMap::new(), turn_use: HashMap::new(), setup_running: false, queue: Vec::new(), later: Vec::new(), subagents: Default::default(), sub_scan: Instant::now(), held: Vec::new(), kid: Default::default(), kids_at: Instant::now(), resume: HashMap::new() };
        h.live.insert("ada".into(), "Hel".into());
        h.subscribe(s);
        h.harness("ada".into(), Out::Delta("lo".into()));
        h.push(&json!({"op": "wake"}));
        let mut r = BufReader::new(b);
        let mut line = || {
            let mut l = String::new();
            r.read_line(&mut l).unwrap();
            serde_json::from_str::<Value>(&l).unwrap()
        };
        assert_eq!(line()["ok"], true);
        assert_eq!(line(), json!({"op": "live", "agent": "ada", "text": "Hel"}));
        assert_eq!(line(), json!({"op": "delta", "agent": "ada", "text": "lo"}));
        assert_eq!(line(), json!({"op": "wake"}));
        assert_eq!(h.live["ada"], "Hello");
    }

    #[test]
    fn mentions_match_names_once_in_order() {
        let names = vec!["ada".to_string(), "bob".to_string(), "r2-d2".to_string()];
        assert_eq!(mentions("@bob look, then @Ada. @bob again", &names), vec!["bob", "ada"]);
        assert_eq!(mentions("mail me@ada.dev", &names), Vec::<String>::new());
        assert_eq!(mentions("ask @r2-d2!", &names), vec!["r2-d2"]);
        assert!(mentions("@carol", &names).is_empty());
    }
}

#[cfg(test)]
mod resume_tests {
    use super::*;

    #[test]
    fn the_resume_time_comes_from_the_reset_in_the_error() {
        let _env = crate::engine::ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        // 0 limits file in the test data dir: the error's `resets 1:50pm`
        let now = crate::engine::now_ms();
        let at = resume_at("You've hit your 5-hour limit · resets 1:50pm (Europe/Berlin)", now);
        assert!(at > now && at <= now + 86_400_000 + 60_000, "{at} {now}");
        assert_eq!(crate::stream::clock(at - 60_000), "13:50");
        // 0 reset in the text: 30 min (+ 1)
        assert_eq!(resume_at("usage limit reached", now), now + 31 * 60_000);
    }
}
