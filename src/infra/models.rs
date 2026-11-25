//! Model lists of the harnesses, for the model dropdown of the agent form.
//! - Claude Code: the aliases (`models` setting) with their exact ids, then the exact ids.
//! - Codex: `~/.codex/models_cache.json` (the codex CLI's server list: models of the picker,
//!   by priority). The CLI refreshes it; codehead runs the same CLI (`codex app-server`).
//! - `OpenCode`: `opencode models`, else an ACP probe.
//! - Other ACP harnesses: a probe: the ACP command, `initialize`, `session/new` in a temp dir,
//!   the models of the session (config option `model`, else `models.availableModels`), kill.
//!
//! Probes run in a background thread (app.rs) and land in `<data dir>/models.json` with their
//! time: older than [`MAX_AGE`] → read again.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::engine::harnesses::{self, Drive};

/// A model of a harness: ID goes into the agent (`codex/<id>`), NAME is the detail of its row.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Model {
    pub id: String,
    #[serde(default)]
    pub name: String,
}

/// The models of 1 harness at a time (unix seconds).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(crate) struct Entry {
    pub at: u64,
    pub models: Vec<Model>,
}

/// A cached list older than this: read again.
pub(crate) const MAX_AGE: Duration = Duration::from_hours(24);
/// Most time a probe gets; its process goes in every case.
pub(crate) const TIMEOUT: Duration = Duration::from_secs(15);

/// Exact ids of the Claude Code aliases (current family).
pub(crate) const CLAUDE_IDS: [(&str, &str); 4] = [
    ("fable", "claude-fable-5-2"),
    ("opus", "claude-opus-5-5"),
    ("sonnet", "claude-sonnet-5-5"),
    ("haiku", "claude-haiku-5-5"),
];

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}

fn path() -> std::path::PathBuf {
    crate::engine::data_dir().join("models.json")
}

/// Every cached list, by harness.
pub(crate) fn load() -> HashMap<String, Entry> {
    std::fs::read(path())
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

/// MODELS of HARNESS into the cache (now).
pub(crate) fn store(harness: &str, models: &[Model]) -> Entry {
    let mut all = load();
    let entry = Entry {
        at: now(),
        models: models.to_vec(),
    };

    all.insert(harness.to_string(), entry.clone());
    let file = path();

    if let Some(dir) = file.parent() {
        let _ = std::fs::create_dir_all(dir);
    }

    if let Ok(bytes) = serde_json::to_vec_pretty(&all) {
        let tmp = file.with_extension(format!("{}.tmp", std::process::id()));

        if std::fs::write(&tmp, bytes).is_ok() {
            let _ = std::fs::rename(tmp, file);
        }
    }

    entry
}

/// E is younger than [`MAX_AGE`].
pub(crate) fn fresh(entry: &Entry) -> bool {
    now().saturating_sub(entry.at) < MAX_AGE.as_secs()
}

/// Claude Code: `default`, the ALIASES (exact id as name), then the exact ids.
pub(crate) fn claude(aliases: &[String]) -> Vec<Model> {
    let exact = |short: &str| CLAUDE_IDS.iter().find(|(name, _)| *name == short).map(|(_, id)| *id);
    let mut models = vec![Model {
        id: "default".into(),
        name: "settings.json model".into(),
    }];

    for short in aliases {
        models.push(Model {
            id: short.clone(),
            name: exact(short).map_or_else(|| crate::domain::agents::model_name(short), String::from),
        });
    }

    for (_, id) in CLAUDE_IDS {
        if !models.iter().any(|model| model.id == id) {
            models.push(Model {
                id: id.into(),
                name: format!("{} · pinned", crate::domain::agents::model_name(id)),
            });
        }
    }

    models
}

/// The models of HARNESS, read now (slow: a CLI or a probe). Claude Code: the fixed list.
pub(crate) fn read(harness: &str) -> Result<Vec<Model>, String> {
    let harness = harnesses::get(harness).ok_or_else(|| format!("unknown harness {harness}"))?;

    let probe = || {
        harnesses::acp_command(harness, None)
            .ok_or_else(|| "0 ACP command".to_string())
            .and_then(|command| acp(&command, TIMEOUT))
    };

    match (harness.name, &harness.drive) {
        ("claude", _) | (_, Drive::Claude) => Ok(claude(&crate::infra::config::models())),
        ("codex", _) => {
            let home = crate::infra::config::home();
            let cached = std::fs::read_to_string(home.join(".codex/models_cache.json"))
                .map(|text| codex_cache(&text))
                .unwrap_or_default();

            if cached.is_empty() {
                Err("0 models in ~/.codex/models_cache.json: run `codex` once".into())
            } else {
                Ok(cached)
            }
        }
        ("opencode", _) => match run(&["opencode".into(), "models".into()], TIMEOUT).map(|text| lines(&text)) {
            Ok(models) if !models.is_empty() => Ok(models),
            _ => probe(),
        },
        _ => probe(),
    }
}

/// `~/.codex/models_cache.json`: the models the codex picker lists (`visibility: list`), by priority.
pub(crate) fn codex_cache(text: &str) -> Vec<Model> {
    let root: Value = serde_json::from_str(text).unwrap_or_default();

    let mut ranked: Vec<(i64, Model)> = root["models"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|model| {
            model["visibility"]
                .as_str()
                .is_none_or(|visibility| visibility == "list")
                && model["supported_in_api"].as_bool() != Some(false)
        })
        .filter_map(|model| {
            let id = model["slug"].as_str()?.to_string();

            Some((
                model["priority"].as_i64().unwrap_or(i64::MAX),
                Model {
                    id,
                    name: model["display_name"].as_str().unwrap_or_default().to_string(),
                },
            ))
        })
        .collect();

    ranked.sort_by_key(|(priority, _)| *priority);
    ranked.into_iter().map(|(_, model)| model).collect()
}

/// 1 model id per line (`opencode models`).
pub(crate) fn lines(text: &str) -> Vec<Model> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.contains(' '))
        .map(|line| Model {
            id: line.into(),
            name: String::new(),
        })
        .collect()
}

/// The models of a `session/new` result R: config option `model` (`options[]`: value + name,
/// grouped or flat), else the older `models.availableModels[]` (modelId + name).
pub(crate) fn session_models(result: &Value) -> Vec<Model> {
    let opt = result["configOptions"].as_array().and_then(|options| {
        options
            .iter()
            .find(|option| option["id"] == "model" || option["category"] == "model")
    });

    if let Some(option) = opt {
        return option_models(option);
    }

    result["models"]["availableModels"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|model| {
            Some(Model {
                id: model["modelId"].as_str()?.into(),
                name: model["name"].as_str().unwrap_or_default().into(),
            })
        })
        .collect()
}

/// SIGTERM to the process group of CHILD, a short grace, SIGKILL, reaped.
fn end(mut child: Child) {
    let pgid = -(child.id() as i32);

    // SAFETY: kill(2) on the process group the spawn created (process_group(0)).
    unsafe { libc::kill(pgid, libc::SIGTERM) };
    let at = Instant::now();

    while at.elapsed() < Duration::from_millis(500) {
        if !matches!(child.try_wait(), Ok(None)) {
            break;
        }

        std::thread::sleep(Duration::from_millis(20));
    }

    // SAFETY: as above; children left after the leader's exit go too.
    unsafe { libc::kill(pgid, libc::SIGKILL) };
    let _ = child.wait();
}

/// Spawn CMD in its own process group in DIR, stdout as lines over a channel.
fn spawn(cmd: &[String], dir: &std::path::Path, stdin: bool) -> Result<(Child, mpsc::Receiver<String>), String> {
    let (bin, args) = cmd.split_first().ok_or("empty command")?;
    let mut child = Command::new(bin)
        .args(args)
        .current_dir(dir)
        .stdin(if stdin { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .map_err(|error| format!("{bin}: {error}"))?;
    let out = child.stdout.take().ok_or("0 stdout")?;
    let (tx, rx) = mpsc::channel();

    std::thread::spawn(move || {
        read_model_lines(out, &(tx));
    });

    Ok((child, rx))
}

fn read_model_lines(out: std::process::ChildStdout, tx: &mpsc::Sender<String>) {
    for line in BufReader::new(out).lines() {
        let Ok(line) = line else { break };

        if tx.send(line).is_err() {
            break;
        }
    }
}

/// Stdout of CMD (max. TIMEOUT); the process goes in every case.
fn run(cmd: &[String], timeout: Duration) -> Result<String, String> {
    let (child, rx) = spawn(cmd, &std::env::temp_dir(), false)?;
    let deadline = Instant::now() + timeout;
    let mut out = String::new();

    let res = loop {
        match rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            Ok(line) => {
                out.push_str(&line);
                out.push('\n');
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break Ok(out),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                break Err(format!("{}: 0 answer in {} s", cmd[0], timeout.as_secs()));
            }
        }
    };

    end(child);

    res
}

/// The result of request ID from the lines of RX (other messages: skipped), until DEADLINE.
fn answer(rx: &mpsc::Receiver<String>, id: u64, deadline: Instant) -> Result<Value, String> {
    loop {
        let line = match rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            Ok(line) => line,
            Err(mpsc::RecvTimeoutError::Timeout) => return Err(format!("0 answer in {} s", TIMEOUT.as_secs())),
            Err(mpsc::RecvTimeoutError::Disconnected) => return Err("the agent quit".into()),
        };

        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };

        if value["id"].as_u64() != Some(id) || value.get("method").is_some() {
            continue;
        }

        if let Some(error) = value.get("error") {
            return Err(error["message"].as_str().unwrap_or("error").to_string());
        }

        return Ok(value["result"].clone());
    }
}

/// ACP probe: CMD, `initialize`, `session/new` (temp dir, 0 MCP servers), the session's models.
pub(crate) fn acp(cmd: &[String], timeout: Duration) -> Result<Vec<Model>, String> {
    let dir = std::env::temp_dir().join(format!("codehead-models-{}", std::process::id()));

    std::fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    let (mut child, rx) = spawn(cmd, &dir, true)?;
    let mut stdin = child.stdin.take();
    let deadline = Instant::now() + timeout;

    let mut send = |value: Value| -> Result<(), String> {
        let writer = stdin.as_mut().ok_or("0 stdin")?;

        writeln!(writer, "{value}")
            .and_then(|()| writer.flush())
            .map_err(|error| format!("stdin: {error}"))
    };

    let res = (|| {
        let init = json!({"protocolVersion": 1, "clientCapabilities": {"fs": {"readTextFile": false, "writeTextFile": false}, "terminal": false},
            "clientInfo": {"name": "codehead", "version": crate::infra::update::VERSION}});

        send(json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": init}))?;
        answer(&rx, 1, deadline).map_err(|error| format!("initialize: {error}"))?;
        send(json!({"jsonrpc": "2.0", "id": 2, "method": "session/new", "params": {"cwd": dir, "mcpServers": []}}))?;
        let result = answer(&rx, 2, deadline).map_err(|error| format!("session/new: {error}"))?;
        let models = session_models(&result);

        if models.is_empty() {
            Err("the session lists 0 models".to_string())
        } else {
            Ok(models)
        }
    })();

    drop(stdin);
    end(child);
    let _ = std::fs::remove_dir_all(&dir);

    res
}

#[cfg(test)]
#[path = "models_tests.rs"]
mod tests;

fn option_models(option: &Value) -> Vec<Model> {
    option["options"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|item| {
            item["options"]
                .as_array()
                .map_or_else(|| vec![item], |group| group.iter().collect())
        })
        .filter_map(|model| {
            Some(Model {
                id: model["value"].as_str()?.into(),
                name: model["name"].as_str().unwrap_or_default().into(),
            })
        })
        .collect()
}
