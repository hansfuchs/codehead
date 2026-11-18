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
}

struct Live {
    session: Session,
    busy: bool,
    /// The owner interrupted the running turn: its end is no error.
    interrupted: bool,
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
    /// The worktree setup runs: messages wait in QUEUE (agent, text) until it ends.
    setup_running: bool,
    queue: Vec<(String, String)>,
    /// Subagents of the agents (their start, steps, end into the log).
    subagents: super::subagents::Tracker,
    sub_scan: Instant,
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
        if self.sessions.contains_key(agent) {
            return Ok(());
        }
        let a = roster::get(agent).ok_or_else(|| format!("no agent {agent}"))?;
        let roster = roster::active();
        let system = harness::system_prompt(&a, &t, &roster);
        let dir = t.work_dir();
        let spec = harness::Spec { agent: &a, thread: &self.id, dir: &dir, resume: t.sessions.get(agent).map(String::as_str), fork: t.fork.iter().any(|a| a == agent), system: &system, exe: &self.exe };
        let session = harness::spawn(&spec, self.harness_tx.clone())?;
        self.sessions.insert(agent.to_string(), Live { session, busy: false, interrupted: false });
        Ok(())
    }

    fn deliver(&mut self, agent: &str, text: &str) -> Result<(), String> {
        // the worktree setup runs: the message waits for its end
        if self.setup_running {
            self.queue.push((agent.to_string(), text.to_string()));
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
                if t.state != "open" {
                    store::update(&self.id, |t| t.state = "open".into())?;
                }
                for a in &to {
                    self.deliver(a, &text)?;
                }
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
            "leave" => {
                let agent = s("agent");
                if let Some(mut l) = self.sessions.remove(&agent) {
                    l.session.kill();
                }
                self.asks.retain(|_, (a, _)| *a != agent);
                store::update(&self.id, |t| t.agents.retain(|a| *a != agent))?;
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
                if !["working", "blocked", "done"].contains(&state.as_str()) {
                    return Err("status: working, blocked or done".into());
                }
                self.log(Some(&s("agent")), Kind::Status { state, note: s("note") });
                Ok(json!({}))
            }
            "stop" => {
                self.stopping = true;
                Ok(json!({}))
            }
            op => Err(format!("unknown op {op:?}")),
        }
    }

    fn harness(&mut self, agent: String, o: Out) {
        self.last = Instant::now();
        match o {
            Out::Event(mut kind) => {
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
                        // a queued message runs as its own turn: it starts with output
                        if let Some(l) = self.sessions.get_mut(&agent)
                            && !l.busy
                        {
                            l.busy = true;
                            self.log(Some(&agent), Kind::TurnStart);
                        }
                    }
                    Kind::TurnEnd { .. } => {
                        if let Some(l) = self.sessions.get_mut(&agent) {
                            l.busy = false;
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
                self.log(Some(&agent), kind);
                for (a, k) in subs {
                    self.log(Some(&a), k);
                }
            }
            Out::Delta(text) => {
                self.push(&json!({"op": "delta", "agent": agent, "text": text}));
                self.live.entry(agent).or_default().push_str(&text);
            }
            Out::Limits(l) => {
                // 1 file for every host: the plan's windows are the account's, not the thread's
                // hosts of other threads write it too: under its lock
                let path = super::data_dir().join("limits.json");
                let _ = super::locked(&super::data_dir().join("limits.lock"), || {
                    let mut v: Value = std::fs::read(&path).ok().and_then(|t| serde_json::from_slice(&t).ok()).unwrap_or_else(|| json!({}));
                    for (w, pct, resets) in l {
                        v["claude"][w] = json!({"pct": pct, "resets": resets});
                    }
                    super::write_atomic(&path, v.to_string().as_bytes())
                });
            }
            Out::Control(id, req) => {
                // ACP agents: the mode decides what runs without the owner
                if let Some(l) = self.sessions.get_mut(&agent)
                    && l.session.host_allows()
                    && let Some(a) = roster::get(&agent)
                    && super::guard::auto_allow(&a.mode, req["tool_name"].as_str().unwrap_or_default(), req["input"]["_kind"].as_str().unwrap_or_default(), &req["input"])
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
        for (agent, text) in std::mem::take(&mut self.queue) {
            let text = if ok { text } else { format!("[codehead: the worktree setup (`{label}`) failed; its output is in the thread. Make the environment work first.]\n\n{text}") };
            if let Err(e) = self.deliver(&agent, &text) {
                self.log(Some(&agent), Kind::Error { message: e });
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
    let exe = std::env::current_exe().unwrap_or_else(|_| Path::new("codehead").to_path_buf());
    let mut h = Host { id: t.id.clone(), exe, sessions: HashMap::new(), asks: HashMap::new(), harness_tx: htx, last: Instant::now(), stopping: false, subs: Default::default(), live: HashMap::new(), setup_running: false, queue: Vec::new(), subagents: Default::default(), sub_scan: Instant::now() };
    catch_signals();
    h.close_open_turns();
    // subagents the log has open ran in a process that is gone
    let (subs, lost) = super::subagents::Tracker::from_log(&store::events(&t.id));
    h.subagents = subs;
    for (a, k) in lost {
        h.log(Some(&a), k);
    }
    if t.setup.as_deref() == Some("pending") {
        h.start_setup(&t, tx.clone());
    }
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
            }
            Ok(Input::Subscribe(s)) => h.subscribe(s),
            Ok(Input::Harness(a, o)) => h.harness(a, o),
            Ok(Input::Setup(label, ok, out)) => h.setup_done(label, ok, out),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
        if h.stopping || SIGNALED.load(std::sync::atomic::Ordering::Relaxed) {
            break;
        }
        if h.subagents.any() && h.sub_scan.elapsed() >= Duration::from_secs(2) {
            h.scan_subs();
        }
        let busy = h.sessions.values().any(|l| l.busy);
        // running subagents keep the host (and their harness process) alive
        if !busy && !h.subagents.any() && !h.setup_running && h.asks.is_empty() && h.last.elapsed() >= IDLE {
            break;
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
        let mut h = Host { id: t.id.clone(), exe: PathBuf::new(), sessions: HashMap::new(), asks: HashMap::new(), harness_tx: htx, last: Instant::now(), stopping: false, subs: Default::default(), live: HashMap::new(), setup_running: false, queue: Vec::new(), subagents: Default::default(), sub_scan: Instant::now() };
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
        let mut h = Host { id: t.id.clone(), exe: PathBuf::new(), sessions: HashMap::new(), asks: HashMap::new(), harness_tx: htx, last: Instant::now(), stopping: false, subs: Default::default(), live: HashMap::new(), setup_running: false, queue: Vec::new(), subagents: Default::default(), sub_scan: Instant::now() };
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
        let mut h = Host { id: "t".into(), exe: PathBuf::new(), sessions: HashMap::new(), asks: HashMap::new(), harness_tx: htx, last: Instant::now(), stopping: false, subs: Default::default(), live: HashMap::new(), setup_running: false, queue: Vec::new(), subagents: Default::default(), sub_scan: Instant::now() };
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
