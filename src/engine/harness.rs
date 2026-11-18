//! 1 harness process per agent and thread. Claude Code: `claude -p` with stream-json in and out;
//! prompts go in as user messages on stdin (the process queues them while a turn runs),
//! permission requests and questions come out as `control_request`s, answered on stdin with a
//! `control_response` once the owner picks. Every other harness: ACP (`engine::acp`).

use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};

use std::sync::mpsc::Sender;

use serde_json::{Value, json};

use super::roster::Agent;
use super::store::Kind;

/// What a session reports to the host.
#[derive(Debug)]
pub enum Out {
    Event(Kind),
    /// A control request (permission, question): its id, the request.
    Control(String, Value),
    /// Plan limits: window → (used %, resets at).
    Limits(Vec<(String, f64, Option<u64>)>),
    /// Agent text not final yet (a token chunk): shown live, 0 log line. The final text comes
    /// as `Kind::Text`.
    Delta(String),
    /// The process ended: its last stderr lines.
    Exit(String),
}

pub struct Spec<'a> {
    pub agent: &'a Agent,
    pub thread: &'a str,
    pub dir: &'a Path,
    pub resume: Option<&'a str>,
    /// Resume into a new session id (the thread is a branch).
    pub fork: bool,
    /// Appended to Claude Code's system prompt: who the agent is, the thread, the tools.
    pub system: &'a str,
    /// `codehead` binary for the MCP server.
    pub exe: &'a Path,
}

pub struct Session {
    pub child: Child,
    driver: Box<dyn Driver>,
}

/// The protocol of a session on the harness's stdin: Claude Code's stream-json or ACP.
pub trait Driver {
    fn prompt(&mut self, text: &str) -> Result<(), String>;
    /// Allow request ID, with INPUT as the tool's input (a question's input carries the answers;
    /// ACP: `_option`, the picked option id).
    fn allow(&mut self, id: &str, input: &Value) -> Result<(), String>;
    fn deny(&mut self, id: &str, why: &str) -> Result<(), String>;
    /// Stop the running turn (the session stays).
    fn interrupt(&mut self) -> Result<(), String>;
    /// The host answers the permission requests itself, per the agent's mode (ACP).
    fn host_allows(&self) -> bool {
        false
    }
}

/// Claude Code: 1 JSON line per message on stdin.
struct ClaudeStream(ChildStdin);

impl ClaudeStream {
    fn write(&mut self, v: &Value) -> Result<(), String> {
        let mut line = v.to_string();
        line.push('\n');
        self.0.write_all(line.as_bytes()).and_then(|_| self.0.flush()).map_err(|e| e.to_string())
    }
}

impl Driver for ClaudeStream {
    fn prompt(&mut self, text: &str) -> Result<(), String> {
        self.write(&json!({"type": "user", "message": {"role": "user", "content": text}}))
    }

    fn allow(&mut self, id: &str, input: &Value) -> Result<(), String> {
        self.write(&json!({"type": "control_response", "response": {"subtype": "success", "request_id": id, "response": {"behavior": "allow", "updatedInput": input}}}))
    }

    fn deny(&mut self, id: &str, why: &str) -> Result<(), String> {
        self.write(&json!({"type": "control_response", "response": {"subtype": "success", "request_id": id, "response": {"behavior": "deny", "message": why}}}))
    }

    fn interrupt(&mut self) -> Result<(), String> {
        self.write(&json!({"type": "control_request", "request_id": super::new_id(), "request": {"subtype": "interrupt"}}))
    }
}

impl Driver for super::acp::Conn {
    fn prompt(&mut self, text: &str) -> Result<(), String> {
        super::acp::Conn::prompt(self, text)
    }

    fn allow(&mut self, id: &str, input: &Value) -> Result<(), String> {
        self.answer(id, input["_option"].as_str(), &["allow_once", "allow_always"])
    }

    fn deny(&mut self, id: &str, _why: &str) -> Result<(), String> {
        self.answer(id, None, &["reject_once", "reject_always"])
    }

    fn interrupt(&mut self) -> Result<(), String> {
        self.cancel()
    }

    fn host_allows(&self) -> bool {
        true
    }
}

/// JSON lines of R → F until R ends or F says stop (false). Bytes, not text: a line with
/// invalid UTF-8 is skipped, the reader goes on.
pub(super) fn json_lines(r: impl std::io::Read, mut f: impl FnMut(Value) -> bool) {
    let mut r = BufReader::new(r);
    let mut buf = Vec::new();
    loop {
        buf.clear();
        match r.read_until(b'\n', &mut buf) {
            Ok(0) | Err(_) => return,
            Ok(_) => {}
        }
        let Ok(v) = serde_json::from_slice::<Value>(&buf) else { continue };
        if !f(v) {
            return;
        }
    }
}

/// Spawn options of a harness process: its own process group (a kill reaches its children).
fn harness_command(bin: &str, s: &Spec) -> Command {
    use std::os::unix::process::CommandExt;
    let mut c = Command::new(bin);
    c.current_dir(s.dir)
        .env("CODEHEAD_THREAD", s.thread)
        .env("CODEHEAD_AGENT", &s.agent.name)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0);
    c
}

/// Output of a tool result kept in the log.
const OUTPUT_CAP: usize = 4096;

pub fn args(s: &Spec) -> Vec<String> {
    let mcp = json!({"mcpServers": {"codehead": {
        "type": "stdio",
        "command": s.exe.display().to_string(),
        "args": ["mcp"],
        "env": {"CODEHEAD_THREAD": s.thread, "CODEHEAD_AGENT": s.agent.name},
    }}});
    let mut a: Vec<String> = [
        "-p",
        "--input-format",
        "stream-json",
        "--output-format",
        "stream-json",
        "--verbose",
        // token chunks (`stream_event`): live text before each full message
        "--include-partial-messages",
        "--permission-prompt-tool",
        "stdio",
        "--permission-mode",
        super::roster::permission_mode(&s.agent.mode),
        "--name",
        &s.agent.name,
        "--append-system-prompt",
        s.system,
        "--mcp-config",
        &mcp.to_string(),
        // codehead's own tools never wait on the owner
        "--allowedTools",
        "mcp__codehead",
    ]
    .map(String::from)
    .to_vec();
    if let Some(m) = s.agent.model.as_deref().filter(|m| !m.is_empty() && *m != "default") {
        a.extend(["--model".into(), m.into()]);
    }
    if let Some(e) = s.agent.effort.as_deref().filter(|e| !e.is_empty() && *e != "default") {
        a.extend(["--effort".into(), e.into()]);
    }
    if let Some(r) = s.resume {
        a.extend(["--resume".into(), r.into()]);
        if s.fork {
            a.push("--fork-session".into());
        }
    }
    a
}

pub(super) fn stderr_tail(stderr: std::process::ChildStderr) -> std::sync::Arc<std::sync::Mutex<Vec<String>>> {
    let tail = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let t = tail.clone();
    std::thread::spawn(move || {
        // bytes: a non-UTF-8 line must not end the reader (a full pipe blocks the harness)
        let mut r = BufReader::new(stderr);
        let mut buf = Vec::new();
        while matches!(r.read_until(b'\n', &mut buf), Ok(n) if n > 0) {
            let l = String::from_utf8_lossy(&buf).trim_end_matches(['\n', '\r']).to_string();
            buf.clear();
            if let Ok(mut t) = t.lock() {
                t.push(l);
                let n = t.len();
                if n > 20 {
                    t.drain(..n - 20);
                }
            }
        }
    });
    tail
}

/// Start the session; its events go to TX, tagged with the agent's name.
pub fn spawn(s: &Spec, tx: Sender<(String, Out)>) -> Result<Session, String> {
    let h = super::harnesses::get(&s.agent.harness).ok_or_else(|| format!("unknown harness {:?}", s.agent.harness))?;
    match super::harnesses::acp_command(h, s.agent.command.as_deref()) {
        None => spawn_claude(s, tx),
        Some(cmd) => spawn_acp(s, h, &cmd, tx),
    }
}

fn spawn_acp(s: &Spec, h: &super::harnesses::Harness, cmd: &[String], tx: Sender<(String, Out)>) -> Result<Session, String> {
    let bin = cmd.first().ok_or("empty ACP command")?;
    let mut child = harness_command(bin, s)
        .args(&cmd[1..])
        .spawn()
        .map_err(|e| format!("{} ({}): {e}. install: {}", h.label, cmd.join(" "), h.install))?;
    let stdin = child.stdin.take().ok_or("0 stdin")?;
    let stdout = child.stdout.take().ok_or("0 stdout")?;
    let tail = stderr_tail(child.stderr.take().ok_or("0 stderr")?);
    let setup = super::acp::Setup {
        cwd: s.dir.display().to_string(),
        resume: s.resume.filter(|_| !s.fork).map(String::from),
        model: s.agent.model.clone().filter(|m| !m.is_empty() && m != "default"),
        mcp: (s.exe.display().to_string(), vec!["mcp".into()], vec![("CODEHEAD_THREAD".into(), s.thread.into()), ("CODEHEAD_AGENT".into(), s.agent.name.clone())]),
        preamble: s.system.to_string(),
    };
    let conn = super::acp::start(stdin, stdout, tail, setup, s.agent.name.clone(), tx);
    Ok(Session { child, driver: Box::new(conn) })
}

fn spawn_claude(s: &Spec, tx: Sender<(String, Out)>) -> Result<Session, String> {
    let mut child = harness_command("claude", s)
        .args(args(s))
        // Claude Code leaves the Artifact tools out of SDK sessions without this
        .env("CLAUDE_CODE_ARTIFACT", std::env::var("CLAUDE_CODE_ARTIFACT").unwrap_or_else(|_| "1".into()))
        .spawn()
        .map_err(|e| format!("claude: {e}"))?;
    let stdin = child.stdin.take().ok_or("claude: 0 stdin")?;
    let stdout = child.stdout.take().ok_or("claude: 0 stdout")?;
    let err_tail = stderr_tail(child.stderr.take().ok_or("claude: 0 stderr")?);
    let name = s.agent.name.clone();
    std::thread::spawn(move || {
        let mut p = Parser::default();
        let mut open = true;
        json_lines(stdout, |v| {
            open = p.line(&v).into_iter().all(|o| tx.send((name.clone(), o)).is_ok());
            open
        });
        if open {
            let err = err_tail.lock().map(|t| t.join("\n")).unwrap_or_default();
            let _ = tx.send((name, Out::Exit(err)));
        }
    });
    Ok(Session { child, driver: Box::new(ClaudeStream(stdin)) })
}

impl Session {
    /// The host answers this session's permission requests per the agent's mode (ACP).
    pub fn host_allows(&self) -> bool {
        self.driver.host_allows()
    }

    pub fn prompt(&mut self, text: &str) -> Result<(), String> {
        self.driver.prompt(text)
    }

    pub fn allow(&mut self, id: &str, input: &Value) -> Result<(), String> {
        self.driver.allow(id, input)
    }

    pub fn deny(&mut self, id: &str, why: &str) -> Result<(), String> {
        self.driver.deny(id, why)
    }

    pub fn interrupt(&mut self) -> Result<(), String> {
        self.driver.interrupt()
    }

    /// End the harness and its children (its process group): SIGTERM, 2 s grace, SIGKILL.
    pub fn kill(&mut self) {
        let group = -(self.child.id() as i32);
        // SAFETY: kill(2) on the process group this session's spawn created (process_group(0)).
        unsafe { libc::kill(group, libc::SIGTERM) };
        let at = std::time::Instant::now();
        while at.elapsed() < std::time::Duration::from_secs(2) {
            if !matches!(self.child.try_wait(), Ok(None)) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        // SAFETY: as above; children left after the leader's exit go too.
        unsafe { libc::kill(group, libc::SIGKILL) };
        let _ = self.child.wait();
    }
}

/// stream-json lines → events. Keeps what spans lines: the last cost total, the context size.
#[derive(Default)]
pub struct Parser {
    /// `total_cost_usd` of the last result: the process reports a running total.
    cost_total: f64,
    /// Context tokens of the last assistant message.
    ctx: u64,
    /// Model of the session (`init` line): its `modelUsage` entry gives the context window.
    model: String,
}

/// Context window of the result's `modelUsage`: the session model's entry, else the largest
/// (subagents on other models add entries). 0: unknown.
fn window_of(usage: &Value, model: &str) -> u64 {
    let size = |m: &Value| m["contextWindow"].as_u64().unwrap_or(0);
    match usage.get(model) {
        Some(m) if size(m) > 0 => size(m),
        _ => usage.as_object().into_iter().flatten().map(|(_, m)| size(m)).max().unwrap_or(0),
    }
}

fn text_of(content: &Value) -> String {
    match content {
        Value::String(s) => s.clone(),
        Value::Array(a) => a.iter().filter_map(|b| b["text"].as_str()).collect::<Vec<_>>().join("\n"),
        _ => String::new(),
    }
}

fn cap(mut s: String) -> String {
    if s.len() > OUTPUT_CAP {
        let mut end = OUTPUT_CAP;
        while !s.is_char_boundary(end) {
            end -= 1;
        }
        s.truncate(end);
        s.push_str("\n…");
    }
    s
}

impl Parser {
    pub fn line(&mut self, v: &Value) -> Vec<Out> {
        let mut out = Vec::new();
        match v["type"].as_str().unwrap_or_default() {
            "system" if v["subtype"] == "init" => {
                self.model = v["model"].as_str().unwrap_or_default().into();
                out.push(Out::Event(Kind::Session {
                    session: v["session_id"].as_str().unwrap_or_default().into(),
                    model: self.model.clone(),
                }));
            }
            "assistant" if v["parent_tool_use_id"].is_null() => {
                let u = &v["message"]["usage"];
                let ctx = ["input_tokens", "cache_read_input_tokens", "cache_creation_input_tokens"].iter().map(|k| u[k].as_u64().unwrap_or(0)).sum::<u64>();
                if ctx > 0 {
                    self.ctx = ctx;
                }
                for b in v["message"]["content"].as_array().into_iter().flatten() {
                    match b["type"].as_str() {
                        Some("text") => {
                            let t = b["text"].as_str().unwrap_or_default();
                            if !t.trim().is_empty() {
                                out.push(Out::Event(Kind::Text { text: t.into() }));
                            }
                        }
                        // a thought with a summary (most thinking blocks: empty + a signature)
                        Some("thinking") => {
                            let t = b["thinking"].as_str().unwrap_or_default().trim();
                            if !t.is_empty() {
                                out.push(Out::Event(Kind::Thought { text: t.into() }));
                            }
                        }
                        Some("tool_use") => out.push(Out::Event(Kind::Tool {
                            id: b["id"].as_str().unwrap_or_default().into(),
                            name: b["name"].as_str().unwrap_or_default().into(),
                            input: b["input"].clone(),
                        })),
                        _ => {}
                    }
                }
            }
            "user" if v["parent_tool_use_id"].is_null() => {
                for b in v["message"]["content"].as_array().into_iter().flatten() {
                    if b["type"] == "tool_result" {
                        out.push(Out::Event(Kind::ToolResult {
                            id: b["tool_use_id"].as_str().unwrap_or_default().into(),
                            ok: !b["is_error"].as_bool().unwrap_or(false),
                            output: cap(text_of(&b["content"])),
                        }));
                    }
                }
            }
            "result" => {
                let total = v["total_cost_usd"].as_f64().unwrap_or(0.0);
                // a running total; a new process (or a smaller total) starts over
                let cost = if total >= self.cost_total { total - self.cost_total } else { total };
                self.cost_total = total;
                let u = &v["usage"];
                let n = |k: &str| u[k].as_u64().unwrap_or(0);
                let error = if v["is_error"].as_bool().unwrap_or(false) || v["subtype"].as_str().is_some_and(|s| s.starts_with("error")) {
                    Some(v["result"].as_str().or(v["subtype"].as_str()).unwrap_or("error").to_string())
                } else {
                    None
                };
                out.push(Out::Event(Kind::TurnEnd {
                    cost,
                    input: n("input_tokens"),
                    output: n("output_tokens"),
                    cache_read: n("cache_read_input_tokens"),
                    cache_write: n("cache_creation_input_tokens"),
                    ctx: self.ctx,
                    window: window_of(&v["modelUsage"], &self.model),
                    ms: v["duration_ms"].as_u64().unwrap_or(0),
                    error,
                }));
            }
            // a token chunk of the main agent (subagents: 0)
            "stream_event" if v["parent_tool_use_id"].is_null() => {
                let e = &v["event"];
                if e["type"] == "content_block_delta"
                    && e["delta"]["type"] == "text_delta"
                    && let Some(t) = e["delta"]["text"].as_str().filter(|t| !t.is_empty())
                {
                    out.push(Out::Delta(t.into()));
                }
            }
            "rate_limit_event" => {
                let w = &v["rate_limit_info"]["unifiedWindows"];
                let l: Vec<(String, f64, Option<u64>)> = w
                    .as_object()
                    .into_iter()
                    .flatten()
                    .map(|(k, x)| (k.clone(), x["utilization"].as_f64().unwrap_or(0.0) * 100.0, x["resetsAt"].as_u64()))
                    .collect();
                if !l.is_empty() {
                    out.push(Out::Limits(l));
                }
            }
            "control_request" => {
                if let Some(id) = v["request_id"].as_str() {
                    out.push(Out::Control(id.into(), v["request"].clone()));
                }
            }
            _ => {}
        }
        out
    }
}

/// What the agent is told about codehead, appended to its system prompt.
pub fn system_prompt(agent: &Agent, thread: &super::store::Thread, roster: &[Agent]) -> String {
    let others: Vec<String> = thread
        .agents
        .iter()
        .filter(|n| **n != agent.name)
        .map(|n| match roster.iter().find(|a| &a.name == n) {
            Some(a) => format!("{n} ({})", harness_label(a)),
            None => n.clone(),
        })
        .collect();
    let mut s = format!(
        "You are {name}, an agent in codehead. codehead runs named agents on threads: 1 thread = 1 task, \
shared with its owner (the person). The owner reads every line of the thread.\n\
- Answer the owner in your reply text: the owner reads it there. \
Use the `send` tool of the `codehead` MCP server (`to`: agent names) only to hand work to another agent or to ask it something: \
other agents see only what you send them. Sending to an agent not on the thread brings it in.\n\
- `list_agents` shows every agent, its harness and model. `set_status` reports working / blocked / done to the owner.\n\
- Messages from other agents reach you as prompts that start with `[from @name]`.\n\
- Ask the owner, not other agents, for anything only the owner knows.\n\
Thread: {title}\nAgents on the thread: {list}",
        name = agent.name,
        title = thread.title,
        list = if others.is_empty() { "only you".to_string() } else { others.join(", ") },
    );
    let profile = crate::agents::session_prompt(agent);
    if !profile.is_empty() {
        s.push_str("\n\n");
        s.push_str(&profile);
    }
    s
}

/// `claude/opus`, `claude`.
pub fn harness_label(a: &Agent) -> String {
    match a.model.as_deref().filter(|m| !m.is_empty() && *m != "default") {
        Some(m) => format!("{}/{m}", a.harness),
        None => a.harness.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_lines_skip_a_non_utf8_line_and_go_on() {
        let mut bytes = b"{\"a\":1}\n".to_vec();
        bytes.extend_from_slice(b"{\"b\":\"\xff\xfe\"}\n");
        bytes.extend_from_slice(b"{\"c\":3}\n");
        let mut seen = Vec::new();
        json_lines(&bytes[..], |v| {
            seen.push(v);
            true
        });
        assert_eq!(seen, vec![json!({"a": 1}), json!({"c": 3})]);
    }

    #[test]
    fn kill_ends_the_harness_children_too() {
        use std::os::unix::process::CommandExt;
        // a harness with a background child (a dev server, a sleep)
        let mut child = Command::new("sh").args(["-c", "sleep 60 & echo $!; wait"]).stdin(Stdio::piped()).stdout(Stdio::piped()).process_group(0).spawn().unwrap();
        let mut line = String::new();
        BufReader::new(child.stdout.take().unwrap()).read_line(&mut line).unwrap();
        let grandchild: i32 = line.trim().parse().unwrap();
        let stdin = child.stdin.take().unwrap();
        let mut s = Session { child, driver: Box::new(ClaudeStream(stdin)) };
        s.kill();
        // SAFETY: signal 0 only checks that the pid exists.
        let alive = || unsafe { libc::kill(grandchild, 0) } == 0;
        let at = std::time::Instant::now();
        while alive() && at.elapsed() < std::time::Duration::from_secs(3) {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        assert!(!alive(), "grandchild {grandchild} still runs");
    }

    #[test]
    fn parser_maps_stream_json() {
        let mut p = Parser::default();
        let ev = |o: Vec<Out>| -> Vec<Kind> { o.into_iter().filter_map(|o| if let Out::Event(k) = o { Some(k) } else { None }).collect() };
        let init = json!({"type": "system", "subtype": "init", "session_id": "s1", "model": "claude-opus-5-5"});
        assert_eq!(ev(p.line(&init)), vec![Kind::Session { session: "s1".into(), model: "claude-opus-5-5".into() }]);
        let a = json!({"type": "assistant", "parent_tool_use_id": null, "message": {"usage": {"input_tokens": 3, "cache_read_input_tokens": 100}, "content": [
            {"type": "thinking", "thinking": ""}, {"type": "text", "text": "hi"}, {"type": "tool_use", "id": "t1", "name": "Bash", "input": {"command": "ls"}}]}});
        let k = ev(p.line(&a));
        assert_eq!(k.len(), 2);
        assert_eq!(k[0], Kind::Text { text: "hi".into() });
        // a thinking block with a summary: 1 thought, trimmed
        let a = json!({"type": "assistant", "parent_tool_use_id": null, "message": {"content": [
            {"type": "thinking", "thinking": "  ", "signature": "x"}, {"type": "thinking", "thinking": "CI=true missing, fixing it.\n\n", "signature": "y"}]}});
        assert_eq!(ev(p.line(&a)), vec![Kind::Thought { text: "CI=true missing, fixing it.".into() }]);
        // token chunks: text deltas of the main agent only
        let chunk = |parent: Value, kind: &str| json!({"type": "stream_event", "parent_tool_use_id": parent, "event": {"type": "content_block_delta", "index": 1, "delta": {"type": kind, "text": "Hel", "thinking": "x"}}});
        assert!(matches!(p.line(&chunk(Value::Null, "text_delta")).as_slice(), [Out::Delta(t)] if t == "Hel"));
        assert!(p.line(&chunk(json!("t9"), "text_delta")).is_empty());
        assert!(p.line(&chunk(Value::Null, "thinking_delta")).is_empty());
        let sub = json!({"type": "assistant", "parent_tool_use_id": "t9", "message": {"content": [{"type": "text", "text": "sub agent"}]}});
        assert!(ev(p.line(&sub)).is_empty());
        let r = json!({"type": "user", "parent_tool_use_id": null, "message": {"content": [{"type": "tool_result", "tool_use_id": "t1", "content": [{"type": "text", "text": "a\nb"}]}]}});
        assert_eq!(ev(p.line(&r)), vec![Kind::ToolResult { id: "t1".into(), ok: true, output: "a\nb".into() }]);
        let end = |c: f64| json!({"type": "result", "subtype": "success", "total_cost_usd": c, "duration_ms": 5, "usage": {"input_tokens": 1, "output_tokens": 2}});
        let Kind::TurnEnd { cost, ctx, window, .. } = ev(p.line(&end(0.5))).remove(0) else { panic!() };
        assert_eq!((cost, ctx, window), (0.5, 103, 0));
        // the window: the session model's `modelUsage` entry, not a subagent's
        let mut e = end(0.5);
        e["modelUsage"] = json!({"claude-haiku-5-5": {"contextWindow": 200000}, "claude-opus-5-5": {"contextWindow": 1000000}});
        let Kind::TurnEnd { window, .. } = ev(p.line(&e)).remove(0) else { panic!() };
        assert_eq!(window, 1_000_000);
        let Kind::TurnEnd { cost, .. } = ev(p.line(&end(0.75))).remove(0) else { panic!() };
        assert_eq!(cost, 0.25);
        let c = json!({"type": "control_request", "request_id": "r1", "request": {"subtype": "can_use_tool"}});
        assert!(matches!(&p.line(&c)[0], Out::Control(id, _) if id == "r1"));
    }
}
