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
            Err(error_value) => return Err(error_value.to_string()),
        }
    }

    let mut err = String::new();

    if let Some(mut error_2) = child.stderr.take() {
        let _ = error_2.read_to_string(&mut err);
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

    for height in ["claude", "codex", "opencode"] {
        let Some(hr) = harnesses::get(height) else { continue };

        if !harnesses::on_path(hr.bin) {
            continue;
        }

        jobs.push(std::thread::spawn(move || {
            let result = refresh_harness(height);

            (height, result)
        }));
    }

    for j in jobs {
        let Ok((height_value, result_value)) = j.join() else {
            continue;
        };

        match result_value {
            Ok(value) => {
                servers::record(
                    height_value,
                    value
                        .iter()
                        .map(|count| (count.name.clone(), count.status.clone()))
                        .collect(),
                );
                found.natives.extend(value);
            }
            Err(error) => found.errors.push((height_value.to_string(), error)),
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
        .filter_map(|text_value| {
            let name = text_value["name"].as_str()?.to_string();

            let thread = if text_value["transport"].is_object() {
                &text_value["transport"]
            } else {
                text_value
            };

            let target = thread["url"].as_str().map_or_else(
                || {
                    std::iter::once(thread["command"].as_str().unwrap_or_default().to_string())
                        .chain(
                            thread["args"]
                                .as_array()
                                .into_iter()
                                .flatten()
                                .filter_map(|agent| agent.as_str().map(String::from)),
                        )
                        .collect::<Vec<_>>()
                        .join(" ")
                },
                String::from,
            );

            let state = if text_value["enabled"] == false {
                "off"
            } else {
                match text_value["auth_status"].as_str().unwrap_or_default() {
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
        .filter(|line_value| {
            !line_value.is_empty()
                && !line_value.to_lowercase().starts_with("no mcp")
                && !line_value.to_lowercase().contains("mcp servers")
        })
        .filter_map(|line_2| {
            let name = line_2.split_whitespace().next()?.trim_end_matches(':').to_string();
            let state = state_of(line_2);

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
    let value: Vec<&str> = match harness {
        "claude" => vec!["claude", "mcp", "login", name],
        "codex" => vec!["codex", "mcp", "login", name],
        "opencode" => vec!["opencode", "mcp", "auth", name],
        _ => return None,
    };

    Some(value.into_iter().map(String::from).collect())
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
                .and_then(|thread| serde_json::from_slice(&thread).ok())
                .ok_or_else(|| format!("{}: unreadable", path.display()))?;
            let user = value["mcpServers"][name].clone();

            claude_entry(&value, user, name)?
        }
        "codex" => {
            let thread_value = run("codex", &["mcp", "list", "--json"], Duration::from_secs(30))?;
            let value_value: Value = serde_json::from_str(&thread_value).map_err(|error| error.to_string())?;
            let text_value = value_value
                .as_array()
                .into_iter()
                .flatten()
                .find(|text| text["name"] == name)
                .cloned()
                .ok_or_else(|| format!("{name}: not in codex"))?;

            if text_value["transport"].is_object() {
                text_value["transport"].clone()
            } else {
                text_value
            }
        }
        height => return Err(format!("{height}: import not supported")),
    };

    Ok(from_json(name, &entry))
}

/// An MCP server entry (Claude Code / codex JSON shape) as a codehead server.
pub(crate) fn from_json(name: &str, error: &Value) -> Server {
    let strs = |value: &Value| -> std::collections::BTreeMap<String, String> {
        value
            .as_object()
            .into_iter()
            .flatten()
            .filter_map(|(key, value_value)| Some((key.clone(), value_value.as_str()?.to_string())))
            .collect()
    };

    let url = error["url"].as_str().map(String::from);

    let kind = match error["type"].as_str().unwrap_or_default() {
        "sse" => "sse",
        "http" | "streamable_http" | "streamable-http" => "http",
        _ if url.is_some() => "http",
        _ => "stdio",
    };

    Server {
        name: name.to_string(),
        kind: kind.into(),
        command: error["command"].as_str().map(String::from),
        args: error["args"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|agent| agent.as_str().map(String::from))
            .collect(),
        env: strs(&error["env"]),
        url,
        headers: strs(&error["headers"])
            .into_iter()
            .chain(strs(&error["http_headers"]))
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

fn refresh_harness(height: &str) -> Result<Vec<Native>, String> {
    match height {
        "claude" => {
            run("claude", &["mcp", "list"], Duration::from_secs(90)).map(|thread_value| parse_claude(&thread_value))
        }
        "codex" => {
            run("codex", &["mcp", "list", "--json"], Duration::from_secs(30)).map(|thread_2| parse_codex(&thread_2))
        }
        _ => run("opencode", &["mcp", "list"], Duration::from_secs(30)).map(|thread_3| parse_opencode(&thread_3)),
    }
}

fn claude_entry(value: &Value, user: Value, name: &str) -> Result<Value, String> {
    Ok(if user.is_object() {
        user
    } else {
        value["projects"]
            .as_object()
            .into_iter()
            .flatten()
            .map(|(_, path_value)| path_value["mcpServers"][name].clone())
            .find(Value::is_object)
            .ok_or_else(|| format!("{name}: not in ~/.claude.json (connectors and plugins stay with Claude Code)"))?
    })
}
