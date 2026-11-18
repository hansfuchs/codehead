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

pub fn socket(id: &str) -> PathBuf {
    super::run_dir().join(format!("{id}.sock"))
}

enum Input {
    Cmd(Value, Sender<Value>),
    Harness(String, Out),
}

struct Live {
    session: Session,
    busy: bool,
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
        self.sessions.insert(agent.to_string(), Live { session, busy: false });
        Ok(())
    }

    fn deliver(&mut self, agent: &str, text: &str) -> Result<(), String> {
        self.ensure(agent)?;
        let live = self.sessions.get_mut(agent).ok_or("0 session")?;
        live.session.prompt(text)?;
        if !live.busy {
            live.busy = true;
            self.log(Some(agent), Kind::TurnStart);
        }
        Ok(())
    }

    fn cmd(&mut self, c: &Value) -> Result<Value, String> {
        self.last = Instant::now();
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
                "busy": self.sessions.iter().filter(|(_, l)| l.busy).map(|(n, _)| n).collect::<Vec<_>>(),
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
                let (agent, req) = self.asks.remove(&id).ok_or_else(|| format!("no open ask {id}"))?;
                let live = self.sessions.get_mut(&agent).ok_or_else(|| format!("{agent}: session gone"))?;
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
                self.log(Some(&agent), Kind::Answer { id, allow, label: s("label") });
                Ok(json!({}))
            }
            "interrupt" => {
                let mut n = 0;
                for live in self.sessions.values_mut().filter(|l| l.busy) {
                    live.session.interrupt()?;
                    n += 1;
                }
                Ok(json!({"interrupted": n}))
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
            Out::Event(kind) => {
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
                    Kind::Text { .. } | Kind::Tool { .. } => {
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
                    }
                    _ => {}
                }
                self.log(Some(&agent), kind);
            }
            Out::Limits(l) => {
                // 1 file for every host: the plan's windows are the account's, not the thread's
                let path = super::data_dir().join("limits.json");
                let mut v: Value = std::fs::read(&path).ok().and_then(|t| serde_json::from_slice(&t).ok()).unwrap_or_else(|| json!({}));
                for (w, pct, resets) in l {
                    v["claude"][w] = json!({"pct": pct, "resets": resets});
                }
                let _ = super::write_atomic(&path, v.to_string().as_bytes());
            }
            Out::Control(id, req) => {
                // ACP agents: the mode decides what runs without the owner
                if let Some(l) = self.sessions.get_mut(&agent)
                    && l.session.is_acp()
                    && let Some(a) = roster::get(&agent)
                    && super::guard::auto_allow(&a.mode, req["tool_name"].as_str().unwrap_or_default(), req["input"]["_kind"].as_str().unwrap_or_default(), &req["input"])
                {
                    if let Err(e) = l.session.allow(&id, &json!({})) {
                        self.log(Some(&agent), Kind::Error { message: format!("auto-allow: {e}") });
                    }
                    return;
                }
                if req["subtype"] == "can_use_tool" {
                    self.log(Some(&agent), Kind::Ask { id: id.clone(), tool: req["tool_name"].as_str().unwrap_or_default().into(), input: req["input"].clone() });
                    self.asks.insert(id, (agent, req));
                }
            }
            Out::Exit(err) => {
                if let Some(mut l) = self.sessions.remove(&agent) {
                    let status = l.session.child.wait().ok().and_then(|s| s.code());
                    if l.busy || status.is_some_and(|c| c != 0) {
                        let tail: Vec<&str> = err.lines().rev().take(6).collect::<Vec<_>>().into_iter().rev().collect();
                        let msg = if tail.is_empty() { format!("session ended (exit {status:?})") } else { tail.join("\n") };
                        self.log(Some(&agent), Kind::Error { message: msg });
                        if l.busy {
                            self.log(Some(&agent), Kind::TurnEnd { cost: 0.0, input: 0, output: 0, cache_read: 0, cache_write: 0, ctx: 0, ms: 0, error: Some("session ended".into()) });
                        }
                    }
                }
                self.asks.retain(|_, (a, _)| *a != agent);
            }
        }
    }

    fn stop(&mut self) {
        for (_, mut l) in self.sessions.drain() {
            l.session.kill();
        }
        let _ = std::fs::remove_file(socket(&self.id));
    }
}

fn serve(conn: UnixStream, tx: Sender<Input>) {
    let Ok(mut w) = conn.try_clone() else { return };
    for line in BufReader::new(conn).lines().map_while(Result::ok) {
        let reply = match serde_json::from_str::<Value>(&line) {
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
fn bind(id: &str) -> Result<UnixListener, String> {
    let path = socket(id);
    std::fs::create_dir_all(super::run_dir()).map_err(|e| e.to_string())?;
    if UnixStream::connect(&path).is_ok() {
        return Err("a host serves this thread".into());
    }
    let _ = std::fs::remove_file(&path);
    UnixListener::bind(&path).map_err(|e| format!("{}: {e}", path.display()))
}

/// Run the host of thread ID until idle or `stop`.
pub fn run(id: &str) -> ! {
    let Some(t) = store::load(id) else {
        eprintln!("codehead host: no thread {id}");
        std::process::exit(2)
    };
    let listener = match bind(&t.id) {
        Ok(l) => l,
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
    let mut h = Host { id: t.id.clone(), exe, sessions: HashMap::new(), asks: HashMap::new(), harness_tx: htx, last: Instant::now(), stopping: false };
    loop {
        match rx.recv_timeout(Duration::from_secs(5)) {
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
            Ok(Input::Harness(a, o)) => h.harness(a, o),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
        if h.stopping {
            break;
        }
        let busy = h.sessions.values().any(|l| l.busy);
        if !busy && h.asks.is_empty() && h.last.elapsed() >= IDLE {
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
    fn mentions_match_names_once_in_order() {
        let names = vec!["ada".to_string(), "bob".to_string(), "r2-d2".to_string()];
        assert_eq!(mentions("@bob look, then @Ada. @bob again", &names), vec!["bob", "ada"]);
        assert_eq!(mentions("mail me@ada.dev", &names), Vec::<String>::new());
        assert_eq!(mentions("ask @r2-d2!", &names), vec!["r2-d2"]);
        assert!(mentions("@carol", &names).is_empty());
    }
}
