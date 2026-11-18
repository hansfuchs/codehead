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
    io: Io,
}

enum Io {
    Claude(ChildStdin),
    Acp(super::acp::Conn),
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
        for l in BufReader::new(stderr).lines().map_while(Result::ok) {
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
    let mut child = Command::new(bin)
        .args(&cmd[1..])
        .current_dir(s.dir)
        .env("CODEHEAD_THREAD", s.thread)
        .env("CODEHEAD_AGENT", &s.agent.name)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
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
    Ok(Session { child, io: Io::Acp(conn) })
}

fn spawn_claude(s: &Spec, tx: Sender<(String, Out)>) -> Result<Session, String> {
    let mut child = Command::new("claude")
        .args(args(s))
        .current_dir(s.dir)
        .env("CODEHEAD_THREAD", s.thread)
        .env("CODEHEAD_AGENT", &s.agent.name)
        // Claude Code leaves the Artifact tools out of SDK sessions without this
        .env("CLAUDE_CODE_ARTIFACT", std::env::var("CLAUDE_CODE_ARTIFACT").unwrap_or_else(|_| "1".into()))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("claude: {e}"))?;
    let stdin = child.stdin.take().ok_or("claude: 0 stdin")?;
    let stdout = child.stdout.take().ok_or("claude: 0 stdout")?;
    let err_tail = stderr_tail(child.stderr.take().ok_or("claude: 0 stderr")?);
    let name = s.agent.name.clone();
    std::thread::spawn(move || {
        let mut p = Parser::default();
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
            for o in p.line(&v) {
                if tx.send((name.clone(), o)).is_err() {
                    return;
                }
            }
        }
        let err = err_tail.lock().map(|t| t.join("\n")).unwrap_or_default();
        let _ = tx.send((name, Out::Exit(err)));
    });
    Ok(Session { child, io: Io::Claude(stdin) })
}

impl Session {
    fn write(&mut self, v: &Value) -> Result<(), String> {
        let Io::Claude(stdin) = &mut self.io else { return Err("not a Claude Code session".into()) };
        let mut line = v.to_string();
        line.push('\n');
        stdin.write_all(line.as_bytes()).and_then(|_| stdin.flush()).map_err(|e| e.to_string())
    }

    /// Codehead runs the tool calls of this session on the owner's word (ACP: the host
    /// answers its permission requests itself in autopilot).
    pub fn is_acp(&self) -> bool {
        matches!(self.io, Io::Acp(_))
    }

    pub fn prompt(&mut self, text: &str) -> Result<(), String> {
        match &self.io {
            Io::Acp(c) => c.prompt(text),
            Io::Claude(_) => self.write(&json!({"type": "user", "message": {"role": "user", "content": text}})),
        }
    }

    /// Allow request ID, with INPUT as the tool's input (a question's input carries the answers;
    /// ACP: `_option`, the picked option id).
    pub fn allow(&mut self, id: &str, input: &Value) -> Result<(), String> {
        match &self.io {
            Io::Acp(c) => c.answer(id, input["_option"].as_str(), &["allow_once", "allow_always"]),
            Io::Claude(_) => self.write(&json!({"type": "control_response", "response": {"subtype": "success", "request_id": id,
                "response": {"behavior": "allow", "updatedInput": input}}})),
        }
    }

    pub fn deny(&mut self, id: &str, why: &str) -> Result<(), String> {
        match &self.io {
            Io::Acp(c) => c.answer(id, None, &["reject_once", "reject_always"]),
            Io::Claude(_) => self.write(&json!({"type": "control_response", "response": {"subtype": "success", "request_id": id,
                "response": {"behavior": "deny", "message": why}}})),
        }
    }

    /// Stop the running turn (the session stays).
    pub fn interrupt(&mut self) -> Result<(), String> {
        match &self.io {
            Io::Acp(c) => c.cancel(),
            Io::Claude(_) => self.write(&json!({"type": "control_request", "request_id": super::new_id(), "request": {"subtype": "interrupt"}})),
        }
    }

    pub fn kill(&mut self) {
        let _ = self.child.kill();
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
            "system" if v["subtype"] == "init" => out.push(Out::Event(Kind::Session {
                session: v["session_id"].as_str().unwrap_or_default().into(),
                model: v["model"].as_str().unwrap_or_default().into(),
            })),
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
                    ms: v["duration_ms"].as_u64().unwrap_or(0),
                    error,
                }));
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
        let sub = json!({"type": "assistant", "parent_tool_use_id": "t9", "message": {"content": [{"type": "text", "text": "sub agent"}]}});
        assert!(ev(p.line(&sub)).is_empty());
        let r = json!({"type": "user", "parent_tool_use_id": null, "message": {"content": [{"type": "tool_result", "tool_use_id": "t1", "content": [{"type": "text", "text": "a\nb"}]}]}});
        assert_eq!(ev(p.line(&r)), vec![Kind::ToolResult { id: "t1".into(), ok: true, output: "a\nb".into() }]);
        let end = |c: f64| json!({"type": "result", "subtype": "success", "total_cost_usd": c, "duration_ms": 5, "usage": {"input_tokens": 1, "output_tokens": 2}});
        let Kind::TurnEnd { cost, ctx, .. } = ev(p.line(&end(0.5))).remove(0) else { panic!() };
        assert_eq!((cost, ctx), (0.5, 103));
        let Kind::TurnEnd { cost, .. } = ev(p.line(&end(0.75))).remove(0) else { panic!() };
        assert_eq!(cost, 0.25);
        let c = json!({"type": "control_request", "request_id": "r1", "request": {"subtype": "can_use_tool"}});
        assert!(matches!(&p.line(&c)[0], Out::Control(id, _) if id == "r1"));
    }
}
