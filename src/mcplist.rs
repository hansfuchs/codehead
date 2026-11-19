//! MCP servers the harnesses have on their own (union with codehead's): their CLIs list them
//! (`claude mcp list`, `codex mcp list --json`, `opencode mcp list`), sign in to them, remove
//! them. A probe asks a codehead server for its tools (MCP `initialize` + `tools/list`). Every
//! call here blocks: the settings tab runs them on a thread.

use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::engine::harnesses;
use crate::engine::servers::{self, Server, Status};

/// 1 server of a harness's own config.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Native {
    pub harness: String,
    pub name: String,
    /// user, local, project, claudeai, plugin (claude); config (others).
    pub source: String,
    /// URL or command line.
    pub target: String,
    pub status: Status,
}

/// What 1 refresh found: the native servers of every installed harness with an MCP CLI, the
/// errors of the CLIs that failed.
#[derive(Clone, Debug, Default)]
pub struct Found {
    pub natives: Vec<Native>,
    pub errors: Vec<(String, String)>,
}

fn run(cmd: &str, args: &[&str], limit: Duration) -> Result<String, String> {
    let mut child = Command::new(cmd)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("{cmd}: {e}"))?;
    let mut out = child.stdout.take().ok_or("0 stdout")?;
    let reader = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = out.read_to_string(&mut s);
        s
    });
    let at = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if at.elapsed() > limit => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("{cmd} {}: 0 answer in {}s", args.join(" "), limit.as_secs()));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(e) => return Err(e.to_string()),
        }
    }
    let mut err = String::new();
    if let Some(mut e) = child.stderr.take() {
        let _ = e.read_to_string(&mut err);
    }
    let out = reader.join().unwrap_or_default();
    if out.trim().is_empty() && !err.trim().is_empty() {
        return Err(err.trim().lines().last().unwrap_or_default().to_string());
    }
    Ok(out)
}

/// Every installed harness with an MCP CLI, listed (claude: its health check, up to 60 s).
pub fn refresh() -> Found {
    let mut found = Found::default();
    let mut jobs = Vec::new();
    for h in ["claude", "codex", "opencode"] {
        let Some(hr) = harnesses::get(h) else { continue };
        if !harnesses::on_path(hr.bin) {
            continue;
        }
        jobs.push(std::thread::spawn(move || {
            let r = match h {
                "claude" => run("claude", &["mcp", "list"], Duration::from_secs(90)).map(|t| parse_claude(&t)),
                "codex" => run("codex", &["mcp", "list", "--json"], Duration::from_secs(30)).map(|t| parse_codex(&t)),
                _ => run("opencode", &["mcp", "list"], Duration::from_secs(30)).map(|t| parse_opencode(&t)),
            };
            (h, r)
        }));
    }
    for j in jobs {
        let Ok((h, r)) = j.join() else { continue };
        match r {
            Ok(v) => {
                servers::record(h, v.iter().map(|n| (n.name.clone(), n.status.clone())).collect());
                found.natives.extend(v);
            }
            Err(e) => found.errors.push((h.to_string(), e)),
        }
    }
    found
}

fn state_of(s: &str) -> &'static str {
    let l = s.to_lowercase();
    if l.contains("needs auth") || l.contains("needs-auth") || l.contains("authenticat") || l.contains("sign-in") || l.contains("login") {
        "needs-auth"
    } else if l.contains("pending") || l.contains("approval") {
        "pending"
    } else if l.contains("fail") || l.contains("error") || l.contains('✘') || l.contains('✗') {
        "failed"
    } else if l.contains("disabled") || l.contains("off") {
        "off"
    } else if l.contains("connected") || l.contains('✔') || l.contains('✓') || l.contains("enabled") {
        "connected"
    } else {
        "unknown"
    }
}

/// `claude mcp list`: `name: target - ✔ Connected`, `… - ✘ Failed to connect — why`.
pub fn parse_claude(text: &str) -> Vec<Native> {
    let at = crate::engine::now_ms();
    text.lines()
        .filter_map(|l| {
            let (name, rest) = l.split_once(": ")?;
            let (target, status) = rest.rsplit_once(" - ").or_else(|| rest.split_once(" - "))?;
            let (status, why) = status.split_once(" — ").unwrap_or((status, ""));
            let source = if name.starts_with("claude.ai ") {
                "claudeai"
            } else if name.starts_with("plugin:") {
                "plugin"
            } else {
                "user"
            };
            let state = state_of(status);
            let error = if state == "failed" { why.chars().take(200).collect() } else { String::new() };
            Some(Native {
                harness: "claude".into(),
                name: name.trim().to_string(),
                source: source.into(),
                target: target.trim().to_string(),
                status: Status { state: state.into(), error, source: source.into(), at, ..Default::default() },
            })
        })
        .collect()
}

/// `codex mcp list --json`: an array of servers (`name`, `transport` or flat fields, `enabled`).
pub fn parse_codex(text: &str) -> Vec<Native> {
    let at = crate::engine::now_ms();
    let v: Value = serde_json::from_str(text).unwrap_or(Value::Null);
    v.as_array()
        .into_iter()
        .flatten()
        .filter_map(|s| {
            let name = s["name"].as_str()?.to_string();
            let t = if s["transport"].is_object() { &s["transport"] } else { s };
            let target = t["url"].as_str().map(String::from).unwrap_or_else(|| {
                std::iter::once(t["command"].as_str().unwrap_or_default().to_string()).chain(t["args"].as_array().into_iter().flatten().filter_map(|a| a.as_str().map(String::from))).collect::<Vec<_>>().join(" ")
            });
            let state = if s["enabled"] == false {
                "off"
            } else {
                match s["auth_status"].as_str().unwrap_or_default() {
                    "not_logged_in" | "notLoggedIn" => "needs-auth",
                    _ => "unknown",
                }
            };
            Some(Native { harness: "codex".into(), name, source: "config".into(), target, status: Status { state: state.into(), source: "config".into(), at, ..Default::default() } })
        })
        .collect()
}

/// `opencode mcp list`: 1 server per line, its state as a word or a mark.
pub fn parse_opencode(text: &str) -> Vec<Native> {
    let at = crate::engine::now_ms();
    text.lines()
        .map(|l| l.trim_start_matches(|c: char| !c.is_alphanumeric()).trim())
        .filter(|l| !l.is_empty() && !l.to_lowercase().starts_with("no mcp") && !l.to_lowercase().contains("mcp servers"))
        .filter_map(|l| {
            let name = l.split_whitespace().next()?.trim_end_matches(':').to_string();
            let state = state_of(l);
            Some(Native { harness: "opencode".into(), name, source: "config".into(), target: String::new(), status: Status { state: state.into(), source: "config".into(), at, ..Default::default() } })
        })
        .collect()
}

// ---------- sign-in, removal, import ----------

/// The CLI call that signs in to server NAME of HARNESS (it opens the browser), None: the
/// harness has none.
pub fn login_command(harness: &str, name: &str) -> Option<Vec<String>> {
    let v: Vec<&str> = match harness {
        "claude" => vec!["claude", "mcp", "login", name],
        "codex" => vec!["codex", "mcp", "login", name],
        "opencode" => vec!["opencode", "mcp", "auth", name],
        _ => return None,
    };
    Some(v.into_iter().map(String::from).collect())
}

/// Sign in to NAME of HARNESS: its CLI, up to 5 min. (the browser flow).
pub fn login(harness: &str, name: &str) -> Result<(), String> {
    let cmd = login_command(harness, name).ok_or_else(|| format!("{harness}: 0 sign-in command"))?;
    let out = Command::new(&cmd[0]).args(&cmd[1..]).stdin(Stdio::null()).output().map_err(|e| format!("{}: {e}", cmd[0]))?;
    if out.status.success() {
        Ok(())
    } else {
        let err = String::from_utf8_lossy(&out.stderr);
        let why = err.trim().lines().last().map(String::from).unwrap_or_else(|| format!("exit {}", out.status));
        Err(format!("{}: {why}", cmd.join(" ")))
    }
}

/// Native server NAME of HARNESS as a codehead server (secrets still plain: the caller moves
/// them into the keychain). Claude: `~/.claude.json` (user scope, local scope of a project).
pub fn import(harness: &str, name: &str) -> Result<Server, String> {
    let entry = match harness {
        "claude" => {
            let path = crate::config::home().join(".claude.json");
            let v: Value = std::fs::read(&path).ok().and_then(|t| serde_json::from_slice(&t).ok()).ok_or_else(|| format!("{}: unreadable", path.display()))?;
            let user = v["mcpServers"][name].clone();
            if user.is_object() {
                user
            } else {
                v["projects"].as_object().into_iter().flatten().map(|(_, p)| p["mcpServers"][name].clone()).find(Value::is_object).ok_or_else(|| format!("{name}: not in ~/.claude.json (connectors and plugins stay with Claude Code)"))?
            }
        }
        "codex" => {
            let t = run("codex", &["mcp", "list", "--json"], Duration::from_secs(30))?;
            let v: Value = serde_json::from_str(&t).map_err(|e| e.to_string())?;
            let s = v.as_array().into_iter().flatten().find(|s| s["name"] == name).cloned().ok_or_else(|| format!("{name}: not in codex"))?;
            if s["transport"].is_object() { s["transport"].clone() } else { s }
        }
        h => return Err(format!("{h}: import not supported")),
    };
    Ok(from_json(name, &entry))
}

/// An MCP server entry (Claude Code / codex JSON shape) as a codehead server.
pub fn from_json(name: &str, e: &Value) -> Server {
    let strs = |v: &Value| -> std::collections::BTreeMap<String, String> { v.as_object().into_iter().flatten().filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string()))).collect() };
    let url = e["url"].as_str().map(String::from);
    let kind = match e["type"].as_str().unwrap_or_default() {
        "sse" => "sse",
        "http" | "streamable_http" | "streamable-http" => "http",
        _ if url.is_some() => "http",
        _ => "stdio",
    };
    Server {
        name: name.to_string(),
        kind: kind.into(),
        command: e["command"].as_str().map(String::from),
        args: e["args"].as_array().into_iter().flatten().filter_map(|a| a.as_str().map(String::from)).collect(),
        env: strs(&e["env"]),
        url,
        headers: strs(&e["headers"]).into_iter().chain(strs(&e["http_headers"])).collect(),
        auto_allow: true,
    }
}

/// Native server NAME of HARNESS gone from its config (after an import).
pub fn remove(harness: &str, name: &str, source: &str) -> Result<(), String> {
    let args: Vec<&str> = match harness {
        "claude" => vec!["mcp", "remove", "-s", if source == "local" { "local" } else { "user" }, name],
        "codex" => vec!["mcp", "remove", name],
        h => return Err(format!("{h}: remove not supported")),
    };
    run(harness, &args, Duration::from_secs(30)).map(|_| ())
}

// ---------- probe ----------

/// Server S, asked for its tools: (state, tool count, error).
pub fn probe(s: &Server) -> Status {
    let at = crate::engine::now_ms();
    let r = match s.kind.as_str() {
        "stdio" => probe_stdio(s),
        "http" => probe_http(s),
        _ => Err("probe: SSE servers are probed in a session only".into()),
    };
    match r {
        Ok(n) => Status { state: "connected".into(), tools: Some(n), source: "codehead".into(), at, ..Default::default() },
        Err(e) if e.contains("401") || e.to_lowercase().contains("unauthorized") => {
            Status { state: "needs-auth".into(), error: "needs OAuth sign-in: codehead cannot sign in for a harness".into(), source: "codehead".into(), at, ..Default::default() }
        }
        Err(e) => Status { state: "failed".into(), error: e, source: "codehead".into(), at, ..Default::default() },
    }
}

const INIT: &str = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"codehead-probe","version":"1"}}}"#;
const INITIALIZED: &str = r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#;
const LIST: &str = r#"{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}"#;

fn probe_stdio(s: &Server) -> Result<usize, String> {
    let cmd = s.command.clone().unwrap_or_default();
    let mut c = Command::new(&cmd);
    c.args(&s.args).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
    for (k, v) in &s.env {
        c.env(k, servers::resolve(v)?);
    }
    let mut child = c.spawn().map_err(|e| format!("{cmd}: {e}"))?;
    let mut stdin = child.stdin.take().ok_or("0 stdin")?;
    let stdout = child.stdout.take().ok_or("0 stdout")?;
    let (tx, rx) = std::sync::mpsc::channel::<Value>();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if let Ok(v) = serde_json::from_str::<Value>(&line)
                && tx.send(v).is_err()
            {
                return;
            }
        }
    });
    let deadline = Instant::now() + Duration::from_secs(20);
    let wait = |id: u64| -> Result<Value, String> {
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let v = rx.recv_timeout(left).map_err(|_| "0 answer in 20s".to_string())?;
            if v["id"] == id {
                return v.get("error").map_or_else(|| Ok(v["result"].clone()), |e| Err(e["message"].as_str().unwrap_or("error").to_string()));
            }
        }
    };
    let r = (|| {
        writeln!(stdin, "{INIT}").map_err(|e| e.to_string())?;
        wait(1)?;
        writeln!(stdin, "{INITIALIZED}\n{LIST}").map_err(|e| e.to_string())?;
        let r = wait(2)?;
        Ok(r["tools"].as_array().map_or(0, Vec::len))
    })();
    let _ = child.kill();
    let _ = child.wait();
    r
}

/// 1 POST to the server (curl; its config, headers included, on stdin: 0 secret in argv).
fn post(s: &Server, body: &str, session: Option<&str>) -> Result<(String, String), String> {
    let mut cfg = format!("url = \"{}\"\nrequest = \"POST\"\nsilent\nshow-error\ninclude\nmax-time = 20\n", s.url.clone().unwrap_or_default().replace('"', "\\\""));
    let mut headers = vec!["Content-Type: application/json".to_string(), "Accept: application/json, text/event-stream".to_string()];
    for (k, v) in &s.headers {
        headers.push(format!("{k}: {}", servers::resolve(v)?));
    }
    if let Some(id) = session {
        headers.push(format!("Mcp-Session-Id: {id}"));
    }
    for h in headers {
        cfg.push_str(&format!("header = \"{}\"\n", h.replace('\\', "\\\\").replace('"', "\\\"")));
    }
    cfg.push_str(&format!("data = \"{}\"\n", body.replace('\\', "\\\\").replace('"', "\\\"")));
    let mut child = Command::new("curl").args(["-K", "-"]).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(|e| format!("curl: {e}"))?;
    child.stdin.take().ok_or("0 stdin")?.write_all(cfg.as_bytes()).map_err(|e| e.to_string())?;
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    let (head, body) = text.split_once("\r\n\r\n").or_else(|| text.split_once("\n\n")).unwrap_or(("", &text));
    let status = head.lines().next().unwrap_or_default();
    if let Some(code) = status.split_whitespace().nth(1)
        && !code.starts_with('2')
    {
        return Err(format!("HTTP {code}"));
    }
    let sid = head.lines().find_map(|l| l.split_once(':').filter(|(k, _)| k.trim().eq_ignore_ascii_case("mcp-session-id")).map(|(_, v)| v.trim().to_string())).unwrap_or_default();
    Ok((sid, body.to_string()))
}

/// The JSON-RPC reply in BODY: plain JSON, or the `data:` lines of an event stream.
fn reply_of(body: &str) -> Option<Value> {
    serde_json::from_str::<Value>(body.trim()).ok().or_else(|| body.lines().filter_map(|l| l.strip_prefix("data:")).filter_map(|d| serde_json::from_str::<Value>(d.trim()).ok()).find(|v| v.get("id").is_some()))
}

fn probe_http(s: &Server) -> Result<usize, String> {
    let (sid, body) = post(s, INIT, None)?;
    let init = reply_of(&body).ok_or("initialize: 0 JSON-RPC reply")?;
    if let Some(e) = init.get("error") {
        return Err(format!("initialize: {}", e["message"].as_str().unwrap_or("error")));
    }
    let sid = (!sid.is_empty()).then_some(sid);
    let _ = post(s, INITIALIZED, sid.as_deref());
    let (_, body) = post(s, LIST, sid.as_deref())?;
    let r = reply_of(&body).ok_or("tools/list: 0 JSON-RPC reply")?;
    if let Some(e) = r.get("error") {
        return Err(format!("tools/list: {}", e["message"].as_str().unwrap_or("error")));
    }
    Ok(r["result"]["tools"].as_array().map_or(0, Vec::len))
}

/// Every codehead server probed (in parallel), the results recorded as harness `codehead`.
pub fn probe_all(list: &[Server]) -> Vec<(String, Status)> {
    let jobs: Vec<_> = list.iter().cloned().map(|s| std::thread::spawn(move || (s.name.clone(), probe(&s)))).collect();
    let rows: Vec<(String, Status)> = jobs.into_iter().filter_map(|j| j.join().ok()).collect();
    servers::record("codehead", rows.clone());
    rows
}

/// The JSON the agent form's `mcp` list needs for a test: unused outside tests.
#[cfg(test)]
fn sample() -> Value {
    use serde_json::json;
    json!({"command": "npx", "args": ["-y", "x"], "env": {"K": "v"}})
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn claude_list_lines_parse_with_states_sources_and_errors() {
        let text = "Checking MCP server health…\n\nclaude.ai Linear: https://mcp.linear.app/mcp - ✔ Connected\nclaude.ai Box: https://mcp.box.com - ! Needs authentication\nplugin:general:domerge: https://d.dev/mcp (HTTP) - ✘ Failed to connect — HTTP 525: Error POSTing\nfiles: npx -y fs /tmp - ⏸ Pending approval\n";
        let v = parse_claude(text);
        assert_eq!(v.len(), 4, "{v:#?}");
        assert_eq!((v[0].name.as_str(), v[0].source.as_str(), v[0].status.state.as_str()), ("claude.ai Linear", "claudeai", "connected"));
        assert_eq!(v[1].status.state, "needs-auth");
        assert_eq!((v[2].name.as_str(), v[2].source.as_str(), v[2].status.state.as_str()), ("plugin:general:domerge", "plugin", "failed"));
        assert_eq!(v[2].target, "https://d.dev/mcp (HTTP)");
        assert!(v[2].status.error.starts_with("HTTP 525"), "{}", v[2].status.error);
        assert_eq!((v[3].source.as_str(), v[3].status.state.as_str()), ("user", "pending"));
    }

    #[test]
    fn codex_json_and_opencode_text_parse() {
        let v = parse_codex(r#"[{"name":"docs","transport":{"type":"stdio","command":"npx","args":["-y","d"]},"enabled":true},{"name":"web","url":"https://w/mcp","enabled":false}]"#);
        assert_eq!(v.len(), 2);
        assert_eq!((v[0].target.as_str(), v[0].status.state.as_str()), ("npx -y d", "unknown"));
        assert_eq!((v[1].target.as_str(), v[1].status.state.as_str()), ("https://w/mcp", "off"));
        assert!(parse_codex("[]").is_empty());
        assert!(parse_opencode("No MCP servers configured\n").is_empty());
        let o = parse_opencode("● ✓ sentry connected\n● ✗ jira failed: 401\n");
        assert_eq!(o.iter().map(|n| (n.name.as_str(), n.status.state.as_str())).collect::<Vec<_>>(), [("sentry", "connected"), ("jira", "failed")]);
    }

    #[test]
    fn entries_become_codehead_servers() {
        let s = from_json("fs", &sample());
        assert_eq!((s.kind.as_str(), s.command.as_deref(), s.args.len(), s.env["K"].as_str()), ("stdio", Some("npx"), 2, "v"));
        let h = from_json("w", &json!({"type": "http", "url": "https://w/mcp", "headers": {"Authorization": "Bearer x"}}));
        assert_eq!((h.kind.as_str(), h.headers.len()), ("http", 1));
        assert_eq!(from_json("c", &json!({"type": "streamable_http", "url": "https://c"})).kind, "http");
        assert_eq!(login_command("opencode", "x").unwrap(), ["opencode", "mcp", "auth", "x"]);
        assert!(login_command("goose", "x").is_none());
    }

    #[test]
    fn probe_counts_the_tools_of_a_stdio_server() {
        // a tiny MCP server in sh: the init reply, then the tool list
        let script = r#"read a; echo '{"jsonrpc":"2.0","id":1,"result":{"capabilities":{}}}'; read b; read c; echo '{"jsonrpc":"2.0","id":2,"result":{"tools":[{"name":"a"},{"name":"b"},{"name":"c"}]}}'; sleep 1"#;
        let s = Server { name: "t".into(), kind: "stdio".into(), command: Some("sh".into()), args: vec!["-c".into(), script.into()], auto_allow: true, ..Default::default() };
        let st = probe(&s);
        assert_eq!((st.state.as_str(), st.tools), ("connected", Some(3)), "{st:?}");
        let bad = Server { command: Some("/nonexistent/x".into()), ..s };
        assert_eq!(probe(&bad).state, "failed");
    }

    #[test]
    fn event_stream_replies_parse() {
        let body = "event: message\ndata: {\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{\"tools\":[{}]}}\n\n";
        assert_eq!(reply_of(body).unwrap()["result"]["tools"].as_array().unwrap().len(), 1);
        assert_eq!(reply_of("{\"id\":1,\"result\":{}}").unwrap()["id"], 1);
    }
}
