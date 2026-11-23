//! Codex: `codex app-server`, the CLI's own JSON-RPC protocol on stdio, 1 JSON message per line.
//! codex-acp (last release 0.16.0) bundles an old codex core: the API refuses every current model
//! with it ("requires a newer version of Codex", "not supported when using Codex with a ChatGPT
//! account"). The app server is the installed CLI itself: its models, its login.
//!
//! Flow: `initialize` → `initialized` → `thread/start` (or `thread/resume`, `thread/fork`) → 1
//! `turn/start` at a time. A turn streams `item/*` notifications (messages, reasoning, commands,
//! file changes, MCP calls) and ends with `turn/completed`. The app server asks
//! `item/commandExecution/requestApproval`, `item/fileChange/requestApproval`,
//! `mcpServer/elicitation/request` and `item/tool/requestUserInput`: codehead answers once the
//! owner (or the agent's mode) picks. `turn/interrupt` stops the running turn.

use std::collections::{HashMap, VecDeque};
use std::io::Write;
use std::process::ChildStdin;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};

use super::harness::{Out, TurnUse};
use super::store::Kind;

/// Why a request went out: the reply goes to its handler.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Pending {
    Init,
    Start,
    Resume,
    Turn,
    Interrupt,
}

pub struct Setup {
    pub cwd: String,
    /// The thread to resume, and whether to fork it (a new thread id).
    pub resume: Option<String>,
    pub fork: bool,
    pub model: Option<String>,
    pub effort: Option<String>,
    /// Developer instructions: who the agent is, the thread.
    pub instructions: String,
    /// Config overrides of the thread: `mcp_servers` (codehead's own + the agent's).
    pub config: Value,
    /// `read-only` (mode plan) or `danger-full-access`: the host decides per call (approval
    /// policy `untrusted`: every non-trusted call asks).
    pub sandbox: &'static str,
}

/// What an open ask answers: a command, a file change, an MCP elicitation, a question.
#[derive(Clone, Debug, PartialEq)]
enum Ask {
    Command,
    FileChange,
    Elicitation,
    /// Question ids by question text (codehead answers by text).
    Questions(Vec<(String, String)>),
}

struct State {
    stdin: Sender<String>,
    next: u64,
    pending: HashMap<u64, Pending>,
    thread: Option<String>,
    /// The running turn's id (`turn/interrupt` needs it).
    turn: Option<String>,
    busy: bool,
    queue: VecDeque<String>,
    /// Open asks: our id → the app server's JSON-RPC id, the kind.
    asks: HashMap<String, (Value, Ask)>,
    /// Tool items of the running turn: item id → (name, input), for asks on them.
    items: HashMap<String, (String, Value)>,
    /// Token totals of the thread: at the running turn's start, the last seen.
    base: TurnUse,
    total: TurnUse,
    ctx: u64,
    window: u64,
    setup: Setup,
    tx: Sender<(String, Out)>,
    name: String,
    /// `Out::Exit` went out: 1 per session.
    dead: bool,
}

#[derive(Clone)]
pub struct Conn(Arc<Mutex<State>>);

/// A JSON-RPC error E as text: its message, the API's own message after it.
fn error_text(e: &Value) -> String {
    let msg = e["message"].as_str().unwrap_or("error").to_string();
    let data = e["data"]["message"].as_str().or_else(|| e["data"].as_str()).unwrap_or_default();
    let inner = serde_json::from_str::<Value>(data).ok().and_then(|v| v["error"]["message"].as_str().map(String::from)).unwrap_or_else(|| data.to_string());
    if inner.is_empty() || inner == msg { msg } else { format!("{msg}: {inner}") }
}

/// The `mcp_servers` config of codehead's MCP server (EXE `mcp`, ENV) and the agent's servers
/// (ACP entries of `servers::acp_config`) → codex config entries.
pub fn mcp_config(exe: &str, env: &[(String, String)], acp: &[Value]) -> Value {
    let mut all = serde_json::Map::new();
    let pairs = |a: &Value| -> serde_json::Map<String, Value> { a.as_array().into_iter().flatten().filter_map(|p| Some((p["name"].as_str()?.to_string(), p["value"].clone()))).collect() };
    all.insert("codehead".into(), json!({"command": exe, "args": ["mcp"], "env": env.iter().map(|(k, v)| (k.clone(), json!(v))).collect::<serde_json::Map<_, _>>()}));

    for s in acp {
        let Some(name) = s["name"].as_str().filter(|n| *n != "codehead") else { continue };
        let entry = match s["url"].as_str() {
            Some(url) => json!({"url": url, "http_headers": pairs(&s["headers"])}),
            None => json!({"command": s["command"], "args": s["args"], "env": pairs(&s["env"])}),
        };

        all.insert(name.to_string(), entry);
    }

    Value::Object(all)
}

/// A tool item of the app server → (codehead tool name, input). None: not a tool (messages,
/// reasoning, the user's own message).
fn tool_of(item: &Value) -> Option<(String, Value)> {
    let s = |k: &str| item[k].as_str().unwrap_or_default().to_string();
    Some(match item["type"].as_str()? {
        "commandExecution" => ("Bash".into(), json!({"command": s("command"), "cwd": item["cwd"], "_kind": "execute"})),
        "fileChange" => {
            let paths: Vec<String> = item["changes"].as_array().into_iter().flatten().filter_map(|c| c["path"].as_str().map(String::from)).collect();
            let mut input = json!({"_kind": "edit", "description": paths.join(", "), "locations": paths.iter().map(|p| json!({"path": p})).collect::<Vec<_>>()});
            if let Some(p) = paths.first() {
                input["file_path"] = json!(p);
            }

            ("Edit".into(), input)
        }
        "mcpToolCall" => (format!("mcp__{}__{}", s("server"), s("tool")), if item["arguments"].is_object() { item["arguments"].clone() } else { json!({}) }),
        "dynamicToolCall" => (s("tool"), if item["arguments"].is_object() { item["arguments"].clone() } else { json!({}) }),
        "webSearch" => ("WebSearch".into(), json!({"query": s("query"), "_kind": "fetch"})),
        "imageView" => ("Read".into(), json!({"file_path": s("path"), "_kind": "read"})),
        "collabAgentToolCall" => ("Agent".into(), json!({"description": s("tool"), "prompt": item["prompt"]})),
        _ => return None,
    })
}

/// A finished tool item → (ok, output).
fn result_of(item: &Value) -> (bool, String) {
    let status = item["status"].as_str().unwrap_or_default();
    let mut ok = !matches!(status, "failed" | "declined");
    let text = match item["type"].as_str().unwrap_or_default() {
        "commandExecution" => {
            ok &= item["exitCode"].as_i64().is_none_or(|c| c == 0);
            item["aggregatedOutput"].as_str().unwrap_or_default().to_string()
        }
        "fileChange" => item["changes"].as_array().into_iter().flatten().map(|c| c["diff"].as_str().unwrap_or_default()).collect::<Vec<_>>().join("\n"),
        "mcpToolCall" => match item["error"]["message"].as_str() {
            Some(e) => {
                ok = false;
                e.to_string()
            }
            None => item["result"]["content"].as_array().into_iter().flatten().filter_map(|c| c["text"].as_str()).collect::<Vec<_>>().join("\n"),
        },
        "dynamicToolCall" => {
            ok &= item["success"].as_bool().unwrap_or(true);
            item["contentItems"].as_array().into_iter().flatten().filter_map(|c| c["text"].as_str()).collect::<Vec<_>>().join("\n")
        }
        "webSearch" => item["query"].as_str().unwrap_or_default().to_string(),
        _ => String::new(),
    };

    if status == "declined" && text.is_empty() {
        return (false, "declined".into());
    }

    (ok, cap(text))
}

fn cap(mut s: String) -> String {
    if s.len() > 4096 {
        let mut end = 4096;
        while !s.is_char_boundary(end) {
            end -= 1;
        }

        s.truncate(end);
        s.push_str("\n…");
    }

    s
}

/// A token breakdown of `thread/tokenUsage/updated` → codehead's numbers: input without the
/// cached part (OpenAI counts it in `inputTokens`).
fn usage_of(b: &Value) -> TurnUse {
    let n = |k: &str| b[k].as_u64().unwrap_or(0);
    TurnUse {
        input: n("inputTokens").saturating_sub(n("cachedInputTokens")),
        output: n("outputTokens"),
        cache_read: n("cachedInputTokens"),
        cache_write: n("cacheWriteInputTokens"),
        ctx: n("totalTokens"),
    }
}

impl State {
    fn send(&mut self, v: &Value) -> Result<(), String> {
        self.stdin.send(v.to_string()).map_err(|_| "codex stdin closed".to_string())
    }

    fn request(&mut self, method: &str, params: Value, why: Pending) -> Result<(), String> {
        self.next += 1;
        let id = self.next;
        self.pending.insert(id, why);
        self.send(&json!({"id": id, "method": method, "params": params}))
    }

    fn out(&self, o: Out) {
        let _ = self.tx.send((self.name.clone(), o));
    }

    /// The running turn's tokens: the thread total since the turn's start.
    fn turn_use(&self) -> TurnUse {
        TurnUse {
            input: self.total.input.saturating_sub(self.base.input),
            output: self.total.output.saturating_sub(self.base.output),
            cache_read: self.total.cache_read.saturating_sub(self.base.cache_read),
            cache_write: self.total.cache_write.saturating_sub(self.base.cache_write),
            ctx: self.ctx,
        }
    }

    /// Params of `thread/start`, `thread/resume`, `thread/fork`.
    fn thread_params(&self) -> Value {
        let mut p = json!({
            "cwd": self.setup.cwd,
            "approvalPolicy": "untrusted",
            "sandbox": self.setup.sandbox,
            "developerInstructions": self.setup.instructions,
            "config": self.setup.config,
        });
        if let Some(m) = &self.setup.model {
            p["model"] = json!(m);
        }

        p
    }

    /// The next queued prompt, when the thread is up and idle.
    fn pump(&mut self) -> Result<(), String> {
        if self.busy {
            return Ok(());
        }

        let Some(tid) = self.thread.clone() else { return Ok(()) };
        let Some(text) = self.queue.pop_front() else { return Ok(()) };

        self.busy = true;
        self.base = self.total;

        let mut p = json!({"threadId": tid, "input": [{"type": "text", "text": text, "text_elements": []}]});
        if let Some(e) = &self.setup.effort {
            p["effort"] = json!(e);
        }

        self.request("turn/start", p, Pending::Turn)
    }

    fn reply(&mut self, why: Pending, v: &Value) -> Result<(), String> {
        let err = v.get("error").map(error_text);
        let r = &v["result"];
        match why {
            Pending::Init => {
                if let Some(e) = err {
                    return Err(format!("initialize: {e}"));
                }

                self.send(&json!({"method": "initialized", "params": {}}))?;

                let mut p = self.thread_params();
                match self.setup.resume.clone() {
                    Some(tid) => {
                        p["threadId"] = json!(tid);
                        let method = if self.setup.fork { "thread/fork" } else { "thread/resume" };
                        self.request(method, p, Pending::Resume)?;
                    }
                    None => self.request("thread/start", p, Pending::Start)?,
                }
            }
            Pending::Start | Pending::Resume => {
                if let Some(e) = err {
                    // the old thread is gone: a new one
                    if why == Pending::Resume {
                        self.setup.resume = None;
                        let p = self.thread_params();
                        return self.request("thread/start", p, Pending::Start);
                    }
                    return Err(format!("thread/start: {e}"));
                }

                let Some(tid) = r["thread"]["id"].as_str().map(String::from) else { return Err("thread/start: 0 thread id".into()) };
                let model = r["model"].as_str().or(r["thread"]["model"].as_str()).unwrap_or_default().to_string();
                self.out(Out::Event(Kind::Session { session: tid.clone(), model }));
                self.thread = Some(tid);
                self.pump()?;
            }
            Pending::Turn => {
                if let Some(e) = err {
                    // 0 turn: the turn end comes from here
                    self.busy = false;
                    self.turn = None;
                    self.out(Out::Event(Kind::TurnEnd { cost: 0.0, input: 0, output: 0, cache_read: 0, cache_write: 0, ctx: self.ctx, window: self.window, ms: 0, error: Some(e) }));

                    self.pump()?;
                } else if let Some(id) = r["turn"]["id"].as_str() {
                    self.turn = Some(id.to_string());
                }
            }
            Pending::Interrupt => {
                if let Some(e) = err {
                    self.out(Out::Event(Kind::Error { message: format!("interrupt: {e}") }));
                }
            }
        }
        Ok(())
    }

    fn notify(&mut self, method: &str, p: &Value) {
        match method {
            "turn/started" => {
                if let Some(id) = p["turn"]["id"].as_str() {
                    self.turn = Some(id.to_string());
                }
            }
            "item/agentMessage/delta" => {
                if let Some(t) = p["delta"].as_str().filter(|t| !t.is_empty()) {
                    self.out(Out::Delta(t.into()));
                }
            }
            "item/started" => {
                let item = &p["item"];
                if let Some((name, input)) = tool_of(item) {
                    let id = item["id"].as_str().unwrap_or_default().to_string();
                    self.items.insert(id.clone(), (name.clone(), input.clone()));
                    self.out(Out::Event(Kind::Tool { id, name, input }));
                }
            }
            "item/completed" => self.completed(&p["item"]),
            "thread/tokenUsage/updated" => {
                let u = &p["tokenUsage"];
                self.total = usage_of(&u["total"]);
                let last = usage_of(&u["last"]);

                if last.ctx > 0 {
                    self.ctx = last.ctx;
                }

                if let Some(w) = u["modelContextWindow"].as_u64() {
                    self.window = w;
                }

                if self.busy {
                    self.out(Out::Usage(self.turn_use()));
                }
            }
            "turn/completed" => {
                let t = &p["turn"];
                self.busy = false;
                self.turn = None;
                self.items.clear();

                let error = match t["status"].as_str().unwrap_or_default() {
                    "failed" => Some(t["error"]["message"].as_str().unwrap_or("failed").to_string()),
                    _ => None,
                };

                let u = self.turn_use();
                self.out(Out::Event(Kind::TurnEnd {
                    cost: 0.0,
                    input: u.input,
                    output: u.output,
                    cache_read: u.cache_read,
                    cache_write: u.cache_write,
                    ctx: self.ctx,
                    window: self.window,
                    ms: t["durationMs"].as_u64().unwrap_or(0),
                    error,
                }));

                if let Err(e) = self.pump() {
                    self.out(Out::Event(Kind::Error { message: format!("codex: {e}") }));
                }
            }
            "mcpServer/startupStatus/updated" => {
                let state = match p["status"].as_str().unwrap_or_default() {
                    "ready" => "connected",
                    "starting" => return,
                    s => s,
                };

                let error = p["error"].as_str().or(p["failureReason"].as_str()).unwrap_or_default().to_string();
                let name = p["name"].as_str().unwrap_or_default().to_string();
                // codex_apps: the app server's own
                if name == "codex_apps" {
                    return;
                }

                let source = if self.setup.config["mcp_servers"].get(&name).is_some() { "codehead" } else { "user" };
                let row = super::servers::Status { state: state.into(), error, source: source.into(), at: super::now_ms(), ..Default::default() };
                self.out(Out::Mcp("codex".into(), vec![(name, row)]));
            }
            _ => {}
        }
    }

    /// A finished item: the agent's text, a thought, a tool result.
    fn completed(&mut self, item: &Value) {
        match item["type"].as_str().unwrap_or_default() {
            "agentMessage" => {
                let t = item["text"].as_str().unwrap_or_default();
                if !t.trim().is_empty() {
                    self.out(Out::Event(Kind::Text { text: t.into() }));
                }
            }
            "reasoning" => {
                let t = item["summary"].as_array().into_iter().flatten().filter_map(|s| s.as_str()).collect::<Vec<_>>().join("\n\n");
                if !t.trim().is_empty() {
                    self.out(Out::Event(Kind::Thought { text: t.trim().to_string() }));
                }
            }
            _ => {
                let id = item["id"].as_str().unwrap_or_default().to_string();
                if self.items.remove(&id).is_some() {
                    let (ok, output) = result_of(item);
                    self.out(Out::Event(Kind::ToolResult { id, ok, output }));
                }
            }
        }
    }

    /// A request of the app server: an ask, else an error reply.
    fn ask(&mut self, method: &str, rpc: Value, p: &Value) -> Result<(), String> {
        let id = super::new_id();
        let item = p["itemId"].as_str().and_then(|i| self.items.get(i)).cloned();
        let (kind, name, mut input) = match method {
            "item/commandExecution/requestApproval" => {
                let input = json!({"command": p["command"], "cwd": p["cwd"], "description": p["reason"], "_kind": "execute"});
                (Ask::Command, "Bash".to_string(), input)
            }
            "item/fileChange/requestApproval" => {
                let (_, mut input) = item.unwrap_or_else(|| ("Edit".into(), json!({"_kind": "edit"})));
                if let Some(r) = p["reason"].as_str() {
                    input["description"] = json!(r);
                }

                (Ask::FileChange, "Edit".to_string(), input)
            }
            "mcpServer/elicitation/request" => {
                let server = p["serverName"].as_str().unwrap_or_default();
                (Ask::Elicitation, format!("mcp__{server}"), json!({"description": p["message"], "_kind": "other"}))
            }
            "item/tool/requestUserInput" => {
                let qs = p["questions"].as_array().cloned().unwrap_or_default();
                let ids = qs.iter().map(|q| (q["question"].as_str().unwrap_or_default().to_string(), q["id"].as_str().unwrap_or_default().to_string())).collect();
                let questions: Vec<Value> = qs
                    .iter()
                    .map(|q| {
                        let options: Vec<Value> = q["options"].as_array().into_iter().flatten().map(|o| json!({"label": o["label"], "description": o["description"]})).collect();
                        json!({"question": q["question"], "header": q["header"], "options": options, "multiSelect": false})
                    })
                    .collect();

                (Ask::Questions(ids), "AskUserQuestion".to_string(), json!({"questions": questions}))
            }
            _ => {
                return self.send(&json!({"id": rpc, "error": {"code": -32601, "message": format!("{method}: not supported by codehead")}}));
            }
        };

        if !input.is_object() {
            input = json!({});
        }

        self.asks.insert(id.clone(), (rpc, kind));
        self.out(Out::Control(id, json!({"subtype": "can_use_tool", "tool_name": name, "input": input})));
        Ok(())
    }

    fn incoming(&mut self, v: &Value) -> Result<(), String> {
        let method = v["method"].as_str();
        let Some(method) = method else {
            // a reply to 1 of our requests
            if let Some(id) = v["id"].as_u64()
                && let Some(why) = self.pending.remove(&id)
            {
                return self.reply(why, v);
            }

            return Ok(());
        };

        match v.get("id") {
            Some(rpc) => self.ask(method, rpc.clone(), &v["params"]),
            None => {
                self.notify(method, &v["params"]);
                Ok(())
            }
        }
    }

    /// The answer to ask ID: ALLOW, with INPUT (a question's `answers`: text → answer).
    fn answer(&mut self, id: &str, allow: bool, input: &Value) -> Result<(), String> {
        let (rpc, kind) = self.asks.remove(id).ok_or_else(|| format!("no open ask {id}"))?;
        let result = match kind {
            Ask::Command | Ask::FileChange => json!({"decision": if allow { "accept" } else { "decline" }}),
            Ask::Elicitation if allow => json!({"action": "accept", "content": {}}),
            Ask::Elicitation => json!({"action": "decline"}),
            Ask::Questions(ids) => {
                let mut answers = serde_json::Map::new();
                for (text, qid) in ids {
                    if let Some(a) = input["answers"][&text].as_str().filter(|_| allow) {
                        answers.insert(qid, json!({"answers": [a]}));
                    }
                }

                json!({"answers": answers})
            }
        };

        self.send(&json!({"id": rpc, "result": result}))
    }

    /// End the session once: the stderr tail, then WHY. The host kills the process.
    fn exit(&mut self, stderr: &Mutex<Vec<String>>, why: &str) {
        if std::mem::replace(&mut self.dead, true) {
            return;
        }

        let mut err = stderr.lock().map(|t| t.join("\n")).unwrap_or_default();
        if !why.is_empty() {
            if !err.is_empty() {
                err.push('\n');
            }

            err.push_str(why);
        }

        self.out(Out::Exit(err));
    }
}

/// Time for `initialize` + `thread/start` to give a thread.
const SETUP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);

fn state(stdin: Sender<String>, setup: Setup, name: String, tx: Sender<(String, Out)>) -> State {
    State {
        stdin,
        next: 0,
        pending: HashMap::new(),
        thread: None,
        turn: None,
        busy: false,
        queue: VecDeque::new(),
        asks: HashMap::new(),
        items: HashMap::new(),
        base: TurnUse::default(),
        total: TurnUse::default(),
        ctx: 0,
        window: 0,
        setup,
        tx,
        name,
        dead: false,
    }
}

/// Start the protocol on a spawned `codex app-server`: stdout is read here, events go to TX.
pub fn start(stdin: ChildStdin, stdout: std::process::ChildStdout, stderr: Arc<Mutex<Vec<String>>>, setup: Setup, name: String, tx: Sender<(String, Out)>) -> Conn {
    let (wtx, wrx) = std::sync::mpsc::channel::<String>();
    std::thread::spawn(move || {
        let mut stdin = stdin;
        for mut line in wrx {
            line.push('\n');
            if stdin.write_all(line.as_bytes()).and_then(|_| stdin.flush()).is_err() {
                return;
            }
        }
    });

    let conn = Conn(Arc::new(Mutex::new(state(wtx, setup, name, tx))));
    {
        let mut s = conn.0.lock().expect("fresh lock");
        let init = json!({"clientInfo": {"name": "codehead", "title": "codehead", "version": crate::update::VERSION}, "capabilities": {"experimentalApi": true}});
        if let Err(e) = s.request("initialize", init, Pending::Init) {
            s.out(Out::Event(Kind::Error { message: format!("codex: {e}") }));
        }
    }

    let c = conn.clone();
    let tail = stderr.clone();
    std::thread::spawn(move || {
        super::harness::json_lines(stdout, |v| {
            let Ok(mut s) = c.0.lock() else { return false };
            if let Err(e) = s.incoming(&v) {
                // 0 thread: the setup failed, the queue never runs → end the session
                if s.thread.is_none() {
                    s.exit(&tail, &format!("codex: {e}"));
                    return false;
                }

                s.out(Out::Event(Kind::Error { message: format!("codex: {e}") }));
            }

            true
        });

        if let Ok(mut s) = c.0.lock() {
            s.exit(&tail, "");
        }
    });

    let c = conn.clone();
    std::thread::spawn(move || {
        let at = std::time::Instant::now();
        while at.elapsed() < SETUP_TIMEOUT {
            std::thread::sleep(std::time::Duration::from_millis(250));
            match c.0.lock() {
                Ok(s) if s.thread.is_some() || s.dead => return,
                Ok(_) => {}
                Err(_) => return,
            }
        }

        if let Ok(mut s) = c.0.lock()
            && s.thread.is_none()
        {
            s.exit(&stderr, &format!("codex: 0 thread after {}s", SETUP_TIMEOUT.as_secs()));
        }
    });

    conn
}

impl super::harness::Driver for Conn {
    fn prompt(&mut self, text: &str) -> Result<(), String> {
        let mut s = self.0.lock().map_err(|e| e.to_string())?;
        s.queue.push_back(text.to_string());
        s.pump()
    }

    fn allow(&mut self, id: &str, input: &Value) -> Result<(), String> {
        self.0.lock().map_err(|e| e.to_string())?.answer(id, true, input)
    }

    fn deny(&mut self, id: &str, _why: &str) -> Result<(), String> {
        self.0.lock().map_err(|e| e.to_string())?.answer(id, false, &Value::Null)
    }

    fn interrupt(&mut self) -> Result<(), String> {
        let mut s = self.0.lock().map_err(|e| e.to_string())?;
        // open asks first: cancelled
        let asks: Vec<(Value, Ask)> = s.asks.drain().map(|(_, a)| a).collect();
        for (rpc, kind) in asks {
            let result = match kind {
                Ask::Command | Ask::FileChange => json!({"decision": "cancel"}),
                Ask::Elicitation => json!({"action": "cancel"}),
                Ask::Questions(_) => json!({"answers": {}}),
            };

            s.send(&json!({"id": rpc, "result": result}))?;
        }

        s.queue.clear();

        match (s.thread.clone(), s.turn.clone()) {
            (Some(t), Some(turn)) => s.request("turn/interrupt", json!({"threadId": t, "turnId": turn}), Pending::Interrupt),
            _ => Ok(()),
        }
    }

    fn host_allows(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_state() -> (State, std::sync::mpsc::Receiver<String>, std::sync::mpsc::Receiver<(String, Out)>) {
        let (tx, rx) = std::sync::mpsc::channel();
        let (stdin, lines) = std::sync::mpsc::channel::<String>();
        let setup = Setup { cwd: "/w".into(), resume: None, fork: false, model: Some("gpt-6.1-sol".into()), effort: Some("high".into()), instructions: "You are ada.".into(), config: json!({}), sandbox: "danger-full-access" };
        (state(stdin, setup, "ada".into(), tx), lines, rx)
    }

    fn sent(lines: &std::sync::mpsc::Receiver<String>) -> Vec<Value> {
        lines.try_iter().map(|l| serde_json::from_str(&l).unwrap()).collect()
    }

    fn events(rx: &std::sync::mpsc::Receiver<(String, Out)>) -> Vec<Out> {
        rx.try_iter().map(|(_, o)| o).collect()
    }

    #[test]
    fn setup_starts_a_thread_with_model_instructions_and_mcp_then_runs_the_queue() {
        let (mut s, lines, rx) = test_state();
        s.queue.push_back("hi".into());
        s.request("initialize", json!({}), Pending::Init).unwrap();
        s.incoming(&json!({"id": 1, "result": {"userAgent": "x"}})).unwrap();

        let m = sent(&lines);
        assert_eq!(m[1]["method"], "initialized");
        assert_eq!(m[2]["method"], "thread/start");
        assert_eq!(m[2]["params"]["model"], "gpt-6.1-sol");
        assert_eq!(m[2]["params"]["developerInstructions"], "You are ada.");
        assert_eq!(m[2]["params"]["approvalPolicy"], "untrusted");

        s.incoming(&json!({"id": 2, "result": {"model": "gpt-6.1-sol", "thread": {"id": "t1"}}})).unwrap();

        assert!(matches!(&events(&rx)[0], Out::Event(Kind::Session { session, model }) if session == "t1" && model == "gpt-6.1-sol"));
        let m = sent(&lines);
        assert_eq!(m[0]["method"], "turn/start");
        assert_eq!(m[0]["params"]["threadId"], "t1");
        assert_eq!(m[0]["params"]["input"][0]["text"], "hi");
        assert_eq!(m[0]["params"]["effort"], "high");

        // a 2nd prompt waits for the turn end
        s.queue.push_back("next".into());
        s.pump().unwrap();
        assert!(sent(&lines).is_empty());
    }

    #[test]
    fn a_lost_thread_on_resume_starts_a_new_one() {
        let (mut s, lines, _rx) = test_state();
        s.setup.resume = Some("old".into());
        s.request("initialize", json!({}), Pending::Init).unwrap();
        s.incoming(&json!({"id": 1, "result": {}})).unwrap();

        let m = sent(&lines);
        assert_eq!((m[2]["method"].as_str(), m[2]["params"]["threadId"].as_str()), (Some("thread/resume"), Some("old")));

        s.incoming(&json!({"id": 2, "error": {"code": -32600, "message": "thread not found"}})).unwrap();
        assert_eq!(sent(&lines)[0]["method"], "thread/start");
    }

    #[test]
    fn a_turn_maps_text_thoughts_tools_usage_and_its_end() {
        let (mut s, lines, rx) = test_state();
        s.thread = Some("t1".into());
        s.queue.push_back("go".into());
        s.total = TurnUse { input: 100, output: 10, cache_read: 50, cache_write: 0, ctx: 0 };
        s.pump().unwrap();
        sent(&lines);

        let n = |m: &str, p: Value| json!({"method": m, "params": p});
        s.incoming(&n("turn/started", json!({"turn": {"id": "u1"}}))).unwrap();
        s.incoming(&n("item/completed", json!({"item": {"type": "reasoning", "id": "r", "summary": ["Cause found."], "content": []}}))).unwrap();
        s.incoming(&n("item/started", json!({"item": {"type": "commandExecution", "id": "c1", "command": "cargo test", "cwd": "/w", "status": "inProgress"}}))).unwrap();
        s.incoming(&n("item/completed", json!({"item": {"type": "commandExecution", "id": "c1", "command": "cargo test", "status": "completed", "exitCode": 1, "aggregatedOutput": "1 failed"}}))).unwrap();
        s.incoming(&n("item/agentMessage/delta", json!({"delta": "Do"}))).unwrap();
        s.incoming(&n("item/completed", json!({"item": {"type": "agentMessage", "id": "m", "text": "Done."}}))).unwrap();
        let tokens = json!({"total": {"inputTokens": 1100, "cachedInputTokens": 850, "outputTokens": 30, "totalTokens": 1130}, "last": {"inputTokens": 900, "cachedInputTokens": 800, "outputTokens": 20, "totalTokens": 920}, "modelContextWindow": 272000});
        s.incoming(&n("thread/tokenUsage/updated", json!({"tokenUsage": tokens}))).unwrap();
        s.incoming(&n("turn/completed", json!({"turn": {"id": "u1", "status": "completed", "error": null, "durationMs": 4530}}))).unwrap();

        let o = events(&rx);
        assert!(matches!(&o[0], Out::Event(Kind::Thought { text }) if text == "Cause found."));
        assert!(matches!(&o[1], Out::Event(Kind::Tool { id, name, input }) if id == "c1" && name == "Bash" && input["command"] == "cargo test"));
        assert!(matches!(&o[2], Out::Event(Kind::ToolResult { ok: false, output, .. }) if output == "1 failed"));
        assert!(matches!(&o[3], Out::Delta(t) if t == "Do"));
        assert!(matches!(&o[4], Out::Event(Kind::Text { text }) if text == "Done."));
        assert!(matches!(&o[5], Out::Usage(u) if u.input == 150 && u.cache_read == 800 && u.output == 20 && u.ctx == 920));
        let Out::Event(Kind::TurnEnd { input, output, cache_read, ctx, window, ms, error, .. }) = &o[6] else { panic!("{:?}", o[6]) };
        assert_eq!((*input, *output, *cache_read, *ctx, *window, *ms, error.clone()), (150, 20, 800, 920, 272000, 4530, None));
        assert!(!s.busy && s.turn.is_none());

        // a failed turn: its error
        s.queue.push_back("again".into());
        s.pump().unwrap();
        s.incoming(&n("turn/completed", json!({"turn": {"id": "u2", "status": "failed", "error": {"message": "The 'x' model is not supported"}}}))).unwrap();
        assert!(events(&rx).iter().any(|o| matches!(o, Out::Event(Kind::TurnEnd { error: Some(e), .. }) if e.contains("not supported"))));
    }

    #[test]
    fn asks_go_to_the_host_and_answers_go_back_by_kind() {
        let (mut s, lines, rx) = test_state();
        s.thread = Some("t1".into());
        let item = json!({"method": "item/started", "params": {"item": {"type": "fileChange", "id": "f1", "status": "inProgress", "changes": [{"path": "/w/a.rs", "kind": {"type": "update"}, "diff": "+x"}]}}});
        s.incoming(&item).unwrap();
        events(&rx);

        s.incoming(&json!({"id": 7, "method": "item/fileChange/requestApproval", "params": {"itemId": "f1", "threadId": "t1", "turnId": "u", "startedAtMs": 1}})).unwrap();
        let Out::Control(id, req) = events(&rx).remove(0) else { panic!() };
        assert_eq!(req["tool_name"], "Edit");
        assert_eq!(req["input"]["_kind"], "edit");
        assert_eq!(req["input"]["file_path"], "/w/a.rs");

        s.answer(&id, true, &json!({})).unwrap();
        assert_eq!(sent(&lines)[0], json!({"id": 7, "result": {"decision": "accept"}}));

        // a command, declined
        s.incoming(&json!({"id": 8, "method": "item/commandExecution/requestApproval", "params": {"itemId": "c", "command": "rm -rf ~/x", "threadId": "t1", "turnId": "u", "startedAtMs": 1}})).unwrap();
        let Out::Control(id, req) = events(&rx).remove(0) else { panic!() };
        assert_eq!((req["tool_name"].as_str(), req["input"]["command"].as_str()), (Some("Bash"), Some("rm -rf ~/x")));

        s.answer(&id, false, &Value::Null).unwrap();
        assert_eq!(sent(&lines)[0]["result"]["decision"], "decline");

        // a question: answers by question text → by question id
        let q = json!({"id": 9, "method": "item/tool/requestUserInput", "params": {"isBlocking": true, "itemId": "q", "threadId": "t1", "turnId": "u", "questions": [{"id": "q1", "header": "Branch", "question": "Which branch?", "options": [{"label": "main", "description": "x"}]}]}});
        s.incoming(&q).unwrap();
        let Out::Control(id, req) = events(&rx).remove(0) else { panic!() };
        assert_eq!(req["tool_name"], "AskUserQuestion");
        assert_eq!(req["input"]["questions"][0]["options"][0]["label"], "main");

        s.answer(&id, true, &json!({"answers": {"Which branch?": "main"}})).unwrap();
        assert_eq!(sent(&lines)[0]["result"], json!({"answers": {"q1": {"answers": ["main"]}}}));

        // an unknown request: an error reply
        s.incoming(&json!({"id": 10, "method": "attestation/generate", "params": {}})).unwrap();
        assert_eq!(sent(&lines)[0]["error"]["code"], -32601);
    }

    #[test]
    fn mcp_config_holds_codehead_and_the_agent_servers() {
        let acp = vec![
            json!({"name": "linear", "command": "npx", "args": ["x"], "env": [{"name": "K", "value": "v"}]}),
            json!({"type": "http", "name": "docs", "url": "https://d", "headers": [{"name": "Authorization", "value": "b"}]}),
        ];
        let c = mcp_config("/bin/codehead", &[("CODEHEAD_AGENT".into(), "ada".into())], &acp);

        assert_eq!(c["codehead"], json!({"command": "/bin/codehead", "args": ["mcp"], "env": {"CODEHEAD_AGENT": "ada"}}));
        assert_eq!(c["linear"]["env"]["K"], "v");
        assert_eq!(c["docs"], json!({"url": "https://d", "http_headers": {"Authorization": "b"}}));
    }

    /// Live: `codex app-server` runs the agent's model. `cargo test codex_live -- --ignored`
    #[test]
    #[ignore]
    fn codex_live_turn_on_the_current_model() {
        let dir = std::env::temp_dir().join(format!("codehead-codex-live-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut child = std::process::Command::new("codex")
            .arg("app-server")
            .current_dir(&dir)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let tail = super::super::harness::stderr_tail(child.stderr.take().unwrap());
        let (tx, rx) = std::sync::mpsc::channel();
        let setup = Setup { cwd: dir.display().to_string(), resume: None, fork: false, model: std::env::var("CODEX_MODEL").ok(), effort: None, instructions: String::new(), config: json!({}), sandbox: "read-only" };
        let mut c = start(child.stdin.take().unwrap(), child.stdout.take().unwrap(), tail, setup, "ada".into(), tx);
        use super::super::harness::Driver;
        c.prompt("reply with the word pong only").unwrap();
        let mut text = String::new();
        let end = loop {
            match rx.recv_timeout(std::time::Duration::from_secs(90)).unwrap().1 {
                Out::Event(Kind::Text { text: t }) => text.push_str(&t),
                Out::Event(Kind::TurnEnd { error, .. }) => break error,
                Out::Exit(e) => panic!("exit: {e}"),
                _ => {}
            }
        };
        let _ = child.kill();
        let _ = child.wait();
        assert_eq!(end, None);
        assert!(text.to_lowercase().contains("pong"), "{text}");
    }

    /// Live: a command asks (policy `untrusted`), the allow runs it, its result comes back.
    /// `cargo test codex_live -- --ignored`
    #[test]
    #[ignore]
    fn codex_live_command_asks_and_runs_on_allow() {
        let dir = std::env::temp_dir().join(format!("codehead-codex-ask-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("marker.txt"), "x").unwrap();
        let mut child = std::process::Command::new("codex")
            .arg("app-server")
            .current_dir(&dir)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let tail = super::super::harness::stderr_tail(child.stderr.take().unwrap());
        let (tx, rx) = std::sync::mpsc::channel();
        let setup = Setup { cwd: dir.display().to_string(), resume: None, fork: false, model: std::env::var("CODEX_MODEL").ok(), effort: None, instructions: String::new(), config: json!({}), sandbox: "danger-full-access" };
        let mut c = start(child.stdin.take().unwrap(), child.stdout.take().unwrap(), tail, setup, "ada".into(), tx);
        use super::super::harness::Driver;
        c.prompt("Run the shell command `touch made-by-codex.txt` in the current directory, then reply with done.").unwrap();
        let (mut asked, mut ran) = (false, false);
        let end = loop {
            match rx.recv_timeout(std::time::Duration::from_secs(120)).unwrap().1 {
                Out::Control(id, req) => {
                    asked = true;
                    assert!(req["tool_name"].is_string(), "{req}");
                    c.allow(&id, &json!({})).unwrap();
                }
                Out::Event(Kind::ToolResult { .. }) => ran = true,
                Out::Event(Kind::TurnEnd { error, .. }) => break error,
                Out::Exit(e) => panic!("exit: {e}"),
                _ => {}
            }
        };
        let _ = child.kill();
        let _ = child.wait();
        assert_eq!(end, None);
        assert!(asked && ran, "asked {asked}, ran {ran}");
        assert!(dir.join("made-by-codex.txt").exists());
    }
}
