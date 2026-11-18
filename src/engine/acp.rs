//! Agent Client Protocol client: codehead drives any ACP agent (OpenCode, Goose, Cursor, Pi, …)
//! over JSON-RPC 2.0 on the agent's stdio, 1 JSON message per line.
//!
//! Flow: `initialize` → `session/new` (or `session/load` to resume) → optional
//! `session/set_model` → 1 `session/prompt` at a time. While a prompt runs, the agent streams
//! `session/update` notifications (message chunks, tool calls) and asks
//! `session/request_permission`; codehead answers that request once the owner (or the agent's
//! mode) picks an option. `session/cancel` stops the running prompt.

use std::collections::{HashMap, VecDeque};
use std::io::{BufRead, BufReader, Write};
use std::process::ChildStdin;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};

use super::harness::Out;
use super::store::Kind;

/// Why a request went out: the reply goes to its handler.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Pending {
    Init,
    New,
    Load,
    Model,
    Prompt,
}

pub struct Setup {
    pub cwd: String,
    pub resume: Option<String>,
    pub model: Option<String>,
    /// The codehead MCP server (command, args, env).
    pub mcp: (String, Vec<String>, Vec<(String, String)>),
    /// Sent before the 1st prompt of a new session: who the agent is, the thread.
    pub preamble: String,
}

struct State {
    stdin: ChildStdin,
    next: u64,
    pending: HashMap<u64, Pending>,
    session: Option<String>,
    /// Loads replay the history as updates: 0 events from them.
    loading: bool,
    can_load: bool,
    busy: bool,
    queue: VecDeque<String>,
    /// The preamble goes before the 1st prompt (a new session only).
    preamble: Option<String>,
    /// Agent text of the running turn, not yet an event.
    text: String,
    /// Agent thought of the running turn (`agent_thought_chunk`s), not yet an event.
    thought: String,
    /// Open permission requests: our id → the agent's JSON-RPC id, the options.
    asks: HashMap<String, (Value, Vec<Value>)>,
    setup: Setup,
    tx: Sender<(String, Out)>,
    name: String,
}

#[derive(Clone)]
pub struct Conn(Arc<Mutex<State>>);

fn mcp_servers(s: &Setup) -> Value {
    let (cmd, args, env) = &s.mcp;
    json!([{"name": "codehead", "command": cmd, "args": args, "env": env.iter().map(|(k, v)| json!({"name": k, "value": v})).collect::<Vec<_>>()}])
}

impl State {
    fn send(&mut self, v: &Value) -> Result<(), String> {
        let mut line = v.to_string();
        line.push('\n');
        self.stdin.write_all(line.as_bytes()).and_then(|_| self.stdin.flush()).map_err(|e| e.to_string())
    }

    fn request(&mut self, method: &str, params: Value, why: Pending) -> Result<(), String> {
        self.next += 1;
        let id = self.next;
        self.pending.insert(id, why);
        self.send(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}))
    }

    fn out(&self, o: Out) {
        let _ = self.tx.send((self.name.clone(), o));
    }

    /// The buffered thought, then the buffered text, as events (empty ones: 0 events).
    fn flush_text(&mut self) {
        self.flush_thought();
        let t = std::mem::take(&mut self.text);
        if !t.trim().is_empty() {
            self.out(Out::Event(Kind::Text { text: t }));
        }
    }

    fn flush_thought(&mut self) {
        let t = std::mem::take(&mut self.thought);
        if !t.trim().is_empty() {
            self.out(Out::Event(Kind::Thought { text: t.trim().to_string() }));
        }
    }

    /// The next queued prompt, when the session is up and idle.
    fn pump(&mut self) -> Result<(), String> {
        if self.busy || self.session.is_none() {
            return Ok(());
        }
        let Some(text) = self.queue.pop_front() else { return Ok(()) };
        let text = match self.preamble.take() {
            Some(p) if !p.is_empty() => format!("{p}\n\n---\n\n{text}"),
            _ => text,
        };
        self.busy = true;
        let sid = self.session.clone().unwrap_or_default();
        self.request("session/prompt", json!({"sessionId": sid, "prompt": [{"type": "text", "text": text}]}), Pending::Prompt)
    }

    fn reply(&mut self, why: Pending, v: &Value) -> Result<(), String> {
        let err = v.get("error").map(|e| e["message"].as_str().unwrap_or("error").to_string());
        let r = &v["result"];
        match why {
            Pending::Init => {
                if let Some(e) = err {
                    return Err(format!("initialize: {e}"));
                }
                self.can_load = r["agentCapabilities"]["loadSession"].as_bool().unwrap_or(false);
                let cwd = self.setup.cwd.clone();
                let mcp = mcp_servers(&self.setup);
                match self.setup.resume.clone().filter(|_| self.can_load) {
                    Some(sid) => {
                        self.loading = true;
                        self.session = None;
                        self.request("session/load", json!({"sessionId": sid, "cwd": cwd, "mcpServers": mcp}), Pending::Load)?;
                        // the id stays the same: kept for the reply
                        self.setup.resume = Some(sid);
                    }
                    None => self.request("session/new", json!({"cwd": cwd, "mcpServers": mcp}), Pending::New)?,
                }
            }
            Pending::New | Pending::Load => {
                self.loading = false;
                let sid = if why == Pending::Load && err.is_none() {
                    self.setup.resume.clone()
                } else if why == Pending::Load {
                    // the old session is gone: a new one
                    let cwd = self.setup.cwd.clone();
                    let mcp = mcp_servers(&self.setup);
                    return self.request("session/new", json!({"cwd": cwd, "mcpServers": mcp}), Pending::New);
                } else {
                    if let Some(e) = err {
                        return Err(format!("session/new: {e}"));
                    }
                    r["sessionId"].as_str().map(String::from)
                };
                let Some(sid) = sid else { return Err("session/new: 0 session id".into()) };
                if why == Pending::Load {
                    self.preamble = None;
                }
                let model = r["models"]["currentModelId"].as_str().unwrap_or_default().to_string();
                self.out(Out::Event(Kind::Session { session: sid.clone(), model }));
                self.session = Some(sid.clone());
                if let Some(m) = self.setup.model.clone() {
                    self.request("session/set_model", json!({"sessionId": sid, "modelId": m}), Pending::Model)?;
                }
                self.pump()?;
            }
            Pending::Model => {}
            Pending::Prompt => {
                self.flush_text();
                self.busy = false;
                let u = &r["usage"];
                let n = |k: &str| u[k].as_u64().unwrap_or(0);
                let error = err.or_else(|| match r["stopReason"].as_str() {
                    Some("refusal") => Some("the agent refused".into()),
                    Some("max_tokens") => Some("hit the token limit".into()),
                    _ => None,
                });
                self.out(Out::Event(Kind::TurnEnd {
                    cost: 0.0,
                    input: n("inputTokens"),
                    output: n("outputTokens"),
                    cache_read: n("cachedReadTokens"),
                    cache_write: n("cachedWriteTokens"),
                    ctx: n("totalTokens"),
                    ms: 0,
                    error,
                }));
                self.pump()?;
            }
        }
        Ok(())
    }

    fn update(&mut self, u: &Value) {
        if self.loading {
            return;
        }
        match u["sessionUpdate"].as_str().unwrap_or_default() {
            "agent_message_chunk" => {
                if let Some(t) = u["content"]["text"].as_str() {
                    // a thought before it: its own event, before the text
                    self.flush_thought();
                    self.text.push_str(t);
                }
            }
            "agent_thought_chunk" => {
                if let Some(t) = u["content"]["text"].as_str().filter(|t| !t.is_empty()) {
                    if !self.text.trim().is_empty() {
                        let text = std::mem::take(&mut self.text);
                        self.out(Out::Event(Kind::Text { text }));
                    }
                    self.thought.push_str(t);
                }
            }
            "tool_call" => {
                self.flush_text();
                let (name, input) = tool_of(u);
                self.out(Out::Event(Kind::Tool { id: u["toolCallId"].as_str().unwrap_or_default().into(), name, input }));
                self.result_of(u);
            }
            "tool_call_update" => self.result_of(u),
            _ => {}
        }
    }

    /// A finished tool call: its result event.
    fn result_of(&mut self, u: &Value) {
        let status = u["status"].as_str().unwrap_or_default();
        if status != "completed" && status != "failed" {
            return;
        }
        let mut out: Vec<String> = Vec::new();
        for c in u["content"].as_array().into_iter().flatten() {
            match c["type"].as_str() {
                Some("content") => out.extend(c["content"]["text"].as_str().map(String::from)),
                Some("diff") => out.push(format!("diff {}", c["path"].as_str().unwrap_or_default())),
                _ => {}
            }
        }
        if out.is_empty()
            && let Some(r) = u.get("rawOutput").filter(|r| !r.is_null())
        {
            out.push(r.as_str().map(String::from).unwrap_or_else(|| r.to_string()));
        }
        let mut text = out.join("\n");
        if text.len() > 4096 {
            let mut end = 4096;
            while !text.is_char_boundary(end) {
                end -= 1;
            }
            text.truncate(end);
            text.push_str("\n…");
        }
        self.out(Out::Event(Kind::ToolResult { id: u["toolCallId"].as_str().unwrap_or_default().into(), ok: status == "completed", output: text }));
    }

    fn incoming(&mut self, v: &Value) -> Result<(), String> {
        // a reply to 1 of our requests
        if v.get("method").is_none() {
            if let Some(id) = v["id"].as_u64()
                && let Some(why) = self.pending.remove(&id)
            {
                return self.reply(why, v);
            }
            return Ok(());
        }
        let method = v["method"].as_str().unwrap_or_default();
        let p = &v["params"];
        match method {
            "session/update" => self.update(&p["update"]),
            "session/request_permission" => {
                self.flush_text();
                let id = super::new_id();
                let options = p["options"].as_array().cloned().unwrap_or_default();
                let tc = &p["toolCall"];
                let (name, mut input) = tool_of(tc);
                if let Some(o) = input.as_object_mut() {
                    o.insert("_options".into(), Value::Array(options.iter().map(|o| json!({"id": o["optionId"], "name": o["name"], "kind": o["kind"]})).collect()));
                    o.insert("_kind".into(), tc["kind"].clone());
                }
                self.asks.insert(id.clone(), (v["id"].clone(), options));
                self.out(Out::Control(id, json!({"subtype": "can_use_tool", "tool_name": name, "input": input})));
            }
            // 0 client capabilities advertised: anything else is an error reply
            _ if v.get("id").is_some() => {
                let id = v["id"].clone();
                self.send(&json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32601, "message": format!("{method}: not supported by codehead")}}))?;
            }
            _ => {}
        }
        Ok(())
    }
}

/// ACP tool call → (codehead tool name, input): its kind as a familiar name, the raw input
/// with the title as description.
pub fn tool_of(tc: &Value) -> (String, Value) {
    let title = tc["title"].as_str().unwrap_or_default().to_string();
    let name = match tc["kind"].as_str().unwrap_or_default() {
        "execute" => "Bash",
        "read" => "Read",
        "edit" => "Edit",
        "delete" => "Delete",
        "move" => "Move",
        "search" => "Grep",
        "fetch" => "WebFetch",
        "think" => "Think",
        _ => "",
    };
    let mut input = tc["rawInput"].clone();
    if !input.is_object() {
        input = json!({});
    }
    if let Some(o) = input.as_object_mut()
        && !o.contains_key("description")
        && !title.is_empty()
    {
        o.insert("description".into(), Value::String(title.clone()));
    }
    let name = if name.is_empty() { if title.is_empty() { "tool".to_string() } else { title } } else { name.to_string() };
    (name, input)
}

/// Start the protocol on a spawned agent: stdout is read here, events go to TX.
pub fn start(stdin: ChildStdin, stdout: std::process::ChildStdout, stderr: Arc<Mutex<Vec<String>>>, setup: Setup, name: String, tx: Sender<(String, Out)>) -> Conn {
    let preamble = Some(setup.preamble.clone());
    let st = State {
        stdin,
        next: 0,
        pending: HashMap::new(),
        session: None,
        loading: false,
        can_load: false,
        busy: false,
        queue: VecDeque::new(),
        preamble,
        text: String::new(),
        thought: String::new(),
        asks: HashMap::new(),
        setup,
        tx,
        name,
    };
    let conn = Conn(Arc::new(Mutex::new(st)));
    {
        let mut s = conn.0.lock().expect("fresh lock");
        let init = json!({"protocolVersion": 1, "clientCapabilities": {"fs": {"readTextFile": false, "writeTextFile": false}, "terminal": false},
            "clientInfo": {"name": "codehead", "version": crate::update::VERSION}});
        if let Err(e) = s.request("initialize", init, Pending::Init) {
            s.out(Out::Event(Kind::Error { message: format!("acp: {e}") }));
        }
    }
    let c = conn.clone();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
            let Ok(mut s) = c.0.lock() else { return };
            if let Err(e) = s.incoming(&v) {
                s.out(Out::Event(Kind::Error { message: format!("acp: {e}") }));
            }
        }
        // stdout closed: the process ended
        let err = stderr.lock().map(|t| t.join("\n")).unwrap_or_default();
        if let Ok(s) = c.0.lock() {
            s.out(Out::Exit(err));
        }
    });
    conn
}

impl Conn {
    pub fn prompt(&self, text: &str) -> Result<(), String> {
        let mut s = self.0.lock().map_err(|e| e.to_string())?;
        s.queue.push_back(text.to_string());
        s.pump()
    }

    /// Answer ask ID with the option OPTION (an option id), else the 1st option of KINDS.
    pub fn answer(&self, id: &str, option: Option<&str>, kinds: &[&str]) -> Result<(), String> {
        let mut s = self.0.lock().map_err(|e| e.to_string())?;
        let (rpc, options) = s.asks.remove(id).ok_or_else(|| format!("no open ask {id}"))?;
        let pick = option.map(String::from).or_else(|| {
            kinds.iter().find_map(|k| options.iter().find(|o| o["kind"] == *k).and_then(|o| o["optionId"].as_str().map(String::from)))
        });
        let outcome = match pick {
            Some(o) => json!({"outcome": "selected", "optionId": o}),
            None => json!({"outcome": "cancelled"}),
        };
        s.send(&json!({"jsonrpc": "2.0", "id": rpc, "result": {"outcome": outcome}}))
    }

    pub fn cancel(&self) -> Result<(), String> {
        let mut s = self.0.lock().map_err(|e| e.to_string())?;
        // open asks first: a cancelled prompt answers them with `cancelled`
        let asks: Vec<(Value, Vec<Value>)> = s.asks.drain().map(|(_, a)| a).collect();
        for (rpc, _) in asks {
            s.send(&json!({"jsonrpc": "2.0", "id": rpc, "result": {"outcome": {"outcome": "cancelled"}}}))?;
        }
        s.queue.clear();
        let sid = s.session.clone().unwrap_or_default();
        s.send(&json!({"jsonrpc": "2.0", "method": "session/cancel", "params": {"sessionId": sid}}))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_calls_map_to_familiar_names() {
        let (n, i) = tool_of(&json!({"kind": "execute", "title": "Run tests", "rawInput": {"command": "cargo test"}}));
        assert_eq!(n, "Bash");
        assert_eq!(i["command"], "cargo test");
        assert_eq!(i["description"], "Run tests");
        let (n, _) = tool_of(&json!({"kind": "other", "title": "codehead_send"}));
        assert_eq!(n, "codehead_send");
    }

    #[test]
    fn thought_chunks_become_1_thought_before_the_next_text_or_tool() {
        let mut child = std::process::Command::new("cat").stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::null()).spawn().unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let setup = Setup { cwd: "/".into(), resume: None, model: None, mcp: (String::new(), Vec::new(), Vec::new()), preamble: String::new() };
        let mut s = State {
            stdin: child.stdin.take().unwrap(),
            next: 0,
            pending: HashMap::new(),
            session: None,
            loading: false,
            can_load: false,
            busy: true,
            queue: VecDeque::new(),
            preamble: None,
            text: String::new(),
            thought: String::new(),
            asks: HashMap::new(),
            setup,
            tx,
            name: "ada".into(),
        };
        let chunk = |kind: &str, t: &str| json!({"sessionUpdate": kind, "content": {"type": "text", "text": t}});
        // empty chunks: 0 events
        s.update(&chunk("agent_thought_chunk", ""));
        for t in ["Cause", " found,", " fixing."] {
            s.update(&chunk("agent_thought_chunk", t));
        }
        s.update(&json!({"sessionUpdate": "tool_call", "toolCallId": "t1", "kind": "execute", "title": "Run", "rawInput": {"command": "ls"}}));
        s.update(&chunk("agent_thought_chunk", "Then the text."));
        s.update(&chunk("agent_message_chunk", "Done."));
        s.flush_text();
        let kinds: Vec<Kind> = rx.try_iter().filter_map(|(_, o)| if let Out::Event(k) = o { Some(k) } else { None }).collect();
        assert_eq!(kinds[0], Kind::Thought { text: "Cause found, fixing.".into() });
        assert!(matches!(kinds[1], Kind::Tool { .. }));
        assert_eq!(kinds[2], Kind::Thought { text: "Then the text.".into() });
        assert_eq!(kinds[3], Kind::Text { text: "Done.".into() });
        assert_eq!(kinds.len(), 4);
        let _ = child.kill();
        let _ = child.wait();
    }
}
