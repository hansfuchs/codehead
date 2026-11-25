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
pub(crate) struct Native {
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
pub(crate) struct Found {
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
        .map_err(|error| format!("{cmd}: {error}"))?;
    let mut out = child.stdout.take().ok_or("0 stdout")?;

    let reader = std::thread::spawn(move || {
        let mut text = String::new();
        let _ = out.read_to_string(&mut text);

        text
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
            Err(error) => return Err(error.to_string()),
        }
    }

    let mut err = String::new();

    if let Some(mut stderr) = child.stderr.take() {
        let _ = stderr.read_to_string(&mut err);
    }

    let out = reader.join().unwrap_or_default();

    if out.trim().is_empty() && !err.trim().is_empty() {
        return Err(err.trim().lines().last().unwrap_or_default().to_string());
    }

    Ok(out)
}

/// Every installed harness with an MCP CLI, listed (claude: its health check, up to 60 s).
pub(crate) fn refresh() -> Found {
    let mut found = Found::default();
    let mut jobs = Vec::new();

    for harness in ["claude", "codex", "opencode"] {
        let Some(hr) = harnesses::get(harness) else { continue };

        if !harnesses::on_path(hr.bin) {
            continue;
        }

        jobs.push(std::thread::spawn(move || {
            let result = refresh_harness(harness);

            (harness, result)
        }));
    }

    for j in jobs {
        let Ok((harness, result)) = j.join() else {
            continue;
        };

        match result {
            Ok(natives) => {
                servers::record(
                    harness,
                    natives
                        .iter()
                        .map(|native| (native.name.clone(), native.status.clone()))
                        .collect(),
                );
                found.natives.extend(natives);
            }
            Err(error) => found.errors.push((harness.to_string(), error)),
        }
    }

    found
}

fn state_of(text: &str) -> &'static str {
    let line = text.to_lowercase();

    if line.contains("needs auth")
        || line.contains("needs-auth")
        || line.contains("authenticat")
        || line.contains("sign-in")
        || line.contains("login")
    {
        "needs-auth"
    } else if line.contains("pending") || line.contains("approval") {
        "pending"
    } else if line.contains("fail") || line.contains("error") || line.contains('✘') || line.contains('✗') {
        "failed"
    } else if line.contains("disabled") || line.contains("off") {
        "off"
    } else if line.contains("connected") || line.contains('✔') || line.contains('✓') || line.contains("enabled") {
        "connected"
    } else {
        "unknown"
    }
}

/// `claude mcp list`: `name: target - ✔ Connected`, `… - ✘ Failed to connect — why`.
pub(crate) fn parse_claude(text: &str) -> Vec<Native> {
    let at = crate::engine::now_ms();

    text.lines()
        .filter_map(|line| {
            let (name, rest) = line.split_once(": ")?;
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

            let error = if state == "failed" {
                why.chars().take(200).collect()
            } else {
                String::new()
            };

            Some(Native {
                harness: "claude".into(),
                name: name.trim().to_string(),
                source: source.into(),
                target: target.trim().to_string(),
                status: Status {
                    state: state.into(),
                    error,
                    source: source.into(),
                    at,
                    ..Default::default()
                },
            })
        })
        .collect()
}

/// `codex mcp list --json`: an array of servers (`name`, `transport` or flat fields, `enabled`).
pub(crate) fn parse_codex(text: &str) -> Vec<Native> {
    let at = crate::engine::now_ms();
    let value: Value = serde_json::from_str(text).unwrap_or(Value::Null);

    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|server| {
            let name = server["name"].as_str()?.to_string();

            let fields = if server["transport"].is_object() {
                &server["transport"]
            } else {
                server
            };

            let target = fields["url"].as_str().map_or_else(
                || {
                    std::iter::once(fields["command"].as_str().unwrap_or_default().to_string())
                        .chain(
                            fields["args"]
                                .as_array()
                                .into_iter()
                                .flatten()
                                .filter_map(|arg| arg.as_str().map(String::from)),
                        )
                        .collect::<Vec<_>>()
                        .join(" ")
                },
                String::from,
            );

            let state = if server["enabled"] == false {
                "off"
            } else {
                match server["auth_status"].as_str().unwrap_or_default() {
                    "not_logged_in" | "notLoggedIn" => "needs-auth",
                    _ => "unknown",
                }
            };

            Some(Native {
                harness: "codex".into(),
                name,
                source: "config".into(),
                target,
                status: Status {
                    state: state.into(),
                    source: "config".into(),
                    at,
                    ..Default::default()
                },
            })
        })
        .collect()
}

/// `opencode mcp list`: 1 server per line, its state as a word or a mark.
pub(crate) fn parse_opencode(text: &str) -> Vec<Native> {
    let at = crate::engine::now_ms();

    text.lines()
        .map(|line| {
            line.trim_start_matches(|character: char| !character.is_alphanumeric())
                .trim()
        })
        .filter(|line| {
            !line.is_empty()
                && !line.to_lowercase().starts_with("no mcp")
                && !line.to_lowercase().contains("mcp servers")
        })
        .filter_map(|line| {
            let name = line.split_whitespace().next()?.trim_end_matches(':').to_string();
            let state = state_of(line);

            Some(Native {
                harness: "opencode".into(),
                name,
                source: "config".into(),
                target: String::new(),
                status: Status {
                    state: state.into(),
                    source: "config".into(),
                    at,
                    ..Default::default()
                },
            })
        })
        .collect()
}

// ---------- sign-in, removal, import ----------

/// The CLI call that signs in to server NAME of HARNESS (it opens the browser), None: the
/// harness has none.
pub(crate) fn login_command(harness: &str, name: &str) -> Option<Vec<String>> {
    let argv: Vec<&str> = match harness {
        "claude" => vec!["claude", "mcp", "login", name],
        "codex" => vec!["codex", "mcp", "login", name],
        "opencode" => vec!["opencode", "mcp", "auth", name],
        _ => return None,
    };

    Some(argv.into_iter().map(String::from).collect())
}

/// Sign in to NAME of HARNESS: its CLI, up to 5 min. (the browser flow).
pub(crate) fn login(harness: &str, name: &str) -> Result<(), String> {
    let cmd = login_command(harness, name).ok_or_else(|| format!("{harness}: 0 sign-in command"))?;
    let out = Command::new(&cmd[0])
        .args(&cmd[1..])
        .stdin(Stdio::null())
        .output()
        .map_err(|error| format!("{}: {error}", cmd[0]))?;

    if out.status.success() {
        Ok(())
    } else {
        let err = String::from_utf8_lossy(&out.stderr);
        let why = err
            .trim()
            .lines()
            .last()
            .map_or_else(|| format!("exit {}", out.status), String::from);

        Err(format!("{}: {why}", cmd.join(" ")))
    }
}

/// Native server NAME of HARNESS as a codehead server (secrets still plain: the caller moves
/// them into the keychain). Claude: `~/.claude.json` (user scope, local scope of a project).
pub(crate) fn import(harness: &str, name: &str) -> Result<Server, String> {
    let entry = match harness {
        "claude" => {
            let path = crate::infra::config::home().join(".claude.json");
            let value: Value = std::fs::read(&path)
                .ok()
                .and_then(|bytes| serde_json::from_slice(&bytes).ok())
                .ok_or_else(|| format!("{}: unreadable", path.display()))?;
            let user_entry = value["mcpServers"][name].clone();

            claude_entry(&value, user_entry, name)?
        }
        "codex" => {
            let listed = run("codex", &["mcp", "list", "--json"], Duration::from_secs(30))?;
            let servers: Value = serde_json::from_str(&listed).map_err(|error| error.to_string())?;
            let server = servers
                .as_array()
                .into_iter()
                .flatten()
                .find(|server| server["name"] == name)
                .cloned()
                .ok_or_else(|| format!("{name}: not in codex"))?;

            if server["transport"].is_object() {
                server["transport"].clone()
            } else {
                server
            }
        }
        height => return Err(format!("{height}: import not supported")),
    };

    Ok(from_json(name, &entry))
}

/// An MCP server entry (Claude Code / codex JSON shape) as a codehead server.
pub(crate) fn from_json(name: &str, entry: &Value) -> Server {
    let strs = |value: &Value| -> std::collections::BTreeMap<String, String> {
        value
            .as_object()
            .into_iter()
            .flatten()
            .filter_map(|(key, text)| Some((key.clone(), text.as_str()?.to_string())))
            .collect()
    };

    let url = entry["url"].as_str().map(String::from);

    let kind = match entry["type"].as_str().unwrap_or_default() {
        "sse" => "sse",
        "http" | "streamable_http" | "streamable-http" => "http",
        _ if url.is_some() => "http",
        _ => "stdio",
    };

    Server {
        name: name.to_string(),
        kind: kind.into(),
        command: entry["command"].as_str().map(String::from),
        args: entry["args"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|arg| arg.as_str().map(String::from))
            .collect(),
        env: strs(&entry["env"]),
        url,
        headers: strs(&entry["headers"])
            .into_iter()
            .chain(strs(&entry["http_headers"]))
            .collect(),
        auto_allow: true,
    }
}

/// Native server NAME of HARNESS gone from its config (after an import).
pub(crate) fn remove(harness: &str, name: &str, source: &str) -> Result<(), String> {
    let args: Vec<&str> = match harness {
        "claude" => vec![
            "mcp",
            "remove",
            "-s",
            if source == "local" { "local" } else { "user" },
            name,
        ],
        "codex" => vec!["mcp", "remove", name],
        height => return Err(format!("{height}: remove not supported")),
    };

    run(harness, &args, Duration::from_secs(30)).map(|_| ())
}

// ---------- probe ----------

const INIT: &str = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"codehead-probe","version":"1"}}}"#;
const INITIALIZED: &str = r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#;
const LIST: &str = r#"{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}"#;

/// The JSON the agent form's `mcp` list needs for a test: unused outside tests.
#[cfg(test)]
fn sample() -> Value {
    use serde_json::json;
    json!({"command": "npx", "args": ["-y", "x"], "env": {"K": "v"}})
}

#[cfg(test)]
#[path = "mcp_list_tests.rs"]
mod tests;

#[path = "mcp_list_probe.rs"]
mod probe;
pub(crate) use probe::probe_all;
#[cfg(test)]
use probe::{probe, reply_of};

fn refresh_harness(harness: &str) -> Result<Vec<Native>, String> {
    match harness {
        "claude" => run("claude", &["mcp", "list"], Duration::from_secs(90)).map(|out| parse_claude(&out)),
        "codex" => run("codex", &["mcp", "list", "--json"], Duration::from_secs(30)).map(|out| parse_codex(&out)),
        _ => run("opencode", &["mcp", "list"], Duration::from_secs(30)).map(|out| parse_opencode(&out)),
    }
}

fn claude_entry(config: &Value, user_entry: Value, name: &str) -> Result<Value, String> {
    Ok(if user_entry.is_object() {
        user_entry
    } else {
        config["projects"]
            .as_object()
            .into_iter()
            .flatten()
            .map(|(_, project)| project["mcpServers"][name].clone())
            .find(Value::is_object)
            .ok_or_else(|| format!("{name}: not in ~/.claude.json (connectors and plugins stay with Claude Code)"))?
    })
}
