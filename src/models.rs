//! Model lists of the harnesses, for the model dropdown of the agent form.
//! - Claude Code: the aliases (`models` setting) with their exact ids, then the exact ids.
//! - Codex: `~/.codex/models_cache.json` (the codex CLI's server list: models of the picker,
//!   by priority). The CLI refreshes it; codehead runs the same CLI (`codex app-server`).
//! - OpenCode: `opencode models`, else an ACP probe.
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
pub struct Model {
    pub id: String,
    #[serde(default)]
    pub name: String,
}

/// The models of 1 harness at a time (unix seconds).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Entry {
    pub at: u64,
    pub models: Vec<Model>,
}

/// A cached list older than this: read again.
pub const MAX_AGE: Duration = Duration::from_secs(24 * 3600);
/// Most time a probe gets; its process goes in every case.
pub const TIMEOUT: Duration = Duration::from_secs(15);

/// Exact ids of the Claude Code aliases (current family).
pub const CLAUDE_IDS: [(&str, &str); 4] = [("fable", "claude-fable-5-2"), ("opus", "claude-opus-5-5"), ("sonnet", "claude-sonnet-5-5"), ("haiku", "claude-haiku-5-5")];

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

fn path() -> std::path::PathBuf {
    crate::engine::data_dir().join("models.json")
}

/// Every cached list, by harness.
pub fn load() -> HashMap<String, Entry> {
    std::fs::read(path()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

/// MODELS of HARNESS into the cache (now).
pub fn store(harness: &str, models: &[Model]) -> Entry {
    let mut all = load();
    let e = Entry { at: now(), models: models.to_vec() };
    all.insert(harness.to_string(), e.clone());
    let p = path();
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    if let Ok(b) = serde_json::to_vec_pretty(&all) {
        let tmp = p.with_extension(format!("{}.tmp", std::process::id()));
        if std::fs::write(&tmp, b).is_ok() {
            let _ = std::fs::rename(tmp, p);
        }
    }
    e
}

/// E is younger than [`MAX_AGE`].
pub fn fresh(e: &Entry) -> bool {
    now().saturating_sub(e.at) < MAX_AGE.as_secs()
}

/// Claude Code: `default`, the ALIASES (exact id as name), then the exact ids.
pub fn claude(aliases: &[String]) -> Vec<Model> {
    let exact = |a: &str| CLAUDE_IDS.iter().find(|(x, _)| *x == a).map(|(_, id)| *id);
    let mut v = vec![Model { id: "default".into(), name: "settings.json model".into() }];
    for a in aliases {
        v.push(Model { id: a.clone(), name: exact(a).map_or_else(|| crate::agents::model_name(a), String::from) });
    }
    for (_, id) in CLAUDE_IDS {
        if !v.iter().any(|m| m.id == id) {
            v.push(Model { id: id.into(), name: format!("{} · pinned", crate::agents::model_name(id)) });
        }
    }
    v
}

/// The models of HARNESS, read now (slow: a CLI or a probe). Claude Code: the fixed list.
pub fn read(harness: &str) -> Result<Vec<Model>, String> {
    let h = harnesses::get(harness).ok_or_else(|| format!("unknown harness {harness}"))?;
    let probe = || harnesses::acp_command(h, None).ok_or_else(|| "0 ACP command".to_string()).and_then(|c| acp(&c, TIMEOUT));
    match (h.name, &h.drive) {
        ("claude", _) | (_, Drive::Claude) => Ok(claude(&crate::config::models())),
        ("codex", _) => {
            let home = crate::config::home();
            let cached = std::fs::read_to_string(home.join(".codex/models_cache.json")).map(|t| codex_cache(&t)).unwrap_or_default();
            if cached.is_empty() { Err("0 models in ~/.codex/models_cache.json: run `codex` once".into()) } else { Ok(cached) }
        }
        ("opencode", _) => match run(&["opencode".into(), "models".into()], TIMEOUT).map(|t| lines(&t)) {
            Ok(m) if !m.is_empty() => Ok(m),
            _ => probe(),
        },
        _ => probe(),
    }
}

/// `~/.codex/models_cache.json`: the models the codex picker lists (`visibility: list`), by priority.
pub fn codex_cache(text: &str) -> Vec<Model> {
    let v: Value = serde_json::from_str(text).unwrap_or_default();
    let mut m: Vec<(i64, Model)> = v["models"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|m| m["visibility"].as_str().is_none_or(|v| v == "list") && m["supported_in_api"].as_bool() != Some(false))
        .filter_map(|m| {
            let id = m["slug"].as_str()?.to_string();
            Some((m["priority"].as_i64().unwrap_or(i64::MAX), Model { id, name: m["display_name"].as_str().unwrap_or_default().to_string() }))
        })
        .collect();
    m.sort_by_key(|(p, _)| *p);
    m.into_iter().map(|(_, m)| m).collect()
}

/// 1 model id per line (`opencode models`).
pub fn lines(text: &str) -> Vec<Model> {
    text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.contains(' ')).map(|l| Model { id: l.into(), name: String::new() }).collect()
}

/// The models of a `session/new` result R: config option `model` (`options[]`: value + name,
/// grouped or flat), else the older `models.availableModels[]` (modelId + name).
pub fn session_models(r: &Value) -> Vec<Model> {
    let opt = r["configOptions"].as_array().and_then(|o| o.iter().find(|c| c["id"] == "model" || c["category"] == "model"));
    if let Some(o) = opt {
        let mut out = Vec::new();
        for it in o["options"].as_array().into_iter().flatten() {
            let group = it["options"].as_array();
            for x in group.map_or_else(|| vec![it], |g| g.iter().collect()) {
                if let Some(id) = x["value"].as_str() {
                    out.push(Model { id: id.into(), name: x["name"].as_str().unwrap_or_default().into() });
                }
            }
        }
        return out;
    }
    r["models"]["availableModels"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|m| Some(Model { id: m["modelId"].as_str()?.into(), name: m["name"].as_str().unwrap_or_default().into() }))
        .collect()
}

/// SIGTERM to the process group of CHILD, a short grace, SIGKILL, reaped.
fn end(mut child: Child) {
    let group = -(child.id() as i32);
    // SAFETY: kill(2) on the process group the spawn created (process_group(0)).
    unsafe { libc::kill(group, libc::SIGTERM) };
    let at = Instant::now();
    while at.elapsed() < Duration::from_millis(500) {
        if !matches!(child.try_wait(), Ok(None)) {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    // SAFETY: as above; children left after the leader's exit go too.
    unsafe { libc::kill(group, libc::SIGKILL) };
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
        .map_err(|e| format!("{bin}: {e}"))?;
    let out = child.stdout.take().ok_or("0 stdout")?;
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(out).lines() {
            let Ok(l) = line else { break };
            if tx.send(l).is_err() {
                break;
            }
        }
    });
    Ok((child, rx))
}

/// Stdout of CMD (max. TIMEOUT); the process goes in every case.
fn run(cmd: &[String], timeout: Duration) -> Result<String, String> {
    let (child, rx) = spawn(cmd, &std::env::temp_dir(), false)?;
    let deadline = Instant::now() + timeout;
    let mut out = String::new();
    let res = loop {
        match rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            Ok(l) => {
                out.push_str(&l);
                out.push('\n');
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break Ok(out),
            Err(mpsc::RecvTimeoutError::Timeout) => break Err(format!("{}: 0 answer in {} s", cmd[0], timeout.as_secs())),
        }
    };
    end(child);
    res
}

/// The result of request ID from the lines of RX (other messages: skipped), until DEADLINE.
fn answer(rx: &mpsc::Receiver<String>, id: u64, deadline: Instant) -> Result<Value, String> {
    loop {
        let line = match rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            Ok(l) => l,
            Err(mpsc::RecvTimeoutError::Timeout) => return Err(format!("0 answer in {} s", TIMEOUT.as_secs())),
            Err(mpsc::RecvTimeoutError::Disconnected) => return Err("the agent quit".into()),
        };
        let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
        if v["id"].as_u64() != Some(id) || v.get("method").is_some() {
            continue;
        }
        if let Some(e) = v.get("error") {
            return Err(e["message"].as_str().unwrap_or("error").to_string());
        }
        return Ok(v["result"].clone());
    }
}

/// ACP probe: CMD, `initialize`, `session/new` (temp dir, 0 MCP servers), the session's models.
pub fn acp(cmd: &[String], timeout: Duration) -> Result<Vec<Model>, String> {
    let dir = std::env::temp_dir().join(format!("codehead-models-{}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let (mut child, rx) = spawn(cmd, &dir, true)?;
    let mut stdin = child.stdin.take();
    let deadline = Instant::now() + timeout;
    let mut send = |v: Value| -> Result<(), String> {
        let w = stdin.as_mut().ok_or("0 stdin")?;
        writeln!(w, "{v}").and_then(|_| w.flush()).map_err(|e| format!("stdin: {e}"))
    };
    let res = (|| {
        let init = json!({"protocolVersion": 1, "clientCapabilities": {"fs": {"readTextFile": false, "writeTextFile": false}, "terminal": false},
            "clientInfo": {"name": "codehead", "version": crate::update::VERSION}});
        send(json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": init}))?;
        answer(&rx, 1, deadline).map_err(|e| format!("initialize: {e}"))?;
        send(json!({"jsonrpc": "2.0", "id": 2, "method": "session/new", "params": {"cwd": dir, "mcpServers": []}}))?;
        let r = answer(&rx, 2, deadline).map_err(|e| format!("session/new: {e}"))?;
        let m = session_models(&r);
        if m.is_empty() { Err("the session lists 0 models".to_string()) } else { Ok(m) }
    })();
    drop(stdin);
    end(child);
    let _ = std::fs::remove_dir_all(&dir);
    res
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(m: &[Model]) -> Vec<&str> {
        m.iter().map(|m| m.id.as_str()).collect()
    }

    #[test]
    fn session_models_of_both_shapes() {
        let new = json!({"configOptions": [{"id": "mode", "options": []}, {"id": "model", "currentValue": "b", "options": [
            {"value": "a", "name": "A"}, {"group": "more", "name": "More", "options": [{"value": "b", "name": "B"}]}]}]});
        assert_eq!(session_models(&new), [Model { id: "a".into(), name: "A".into() }, Model { id: "b".into(), name: "B".into() }]);
        let old = json!({"models": {"currentModelId": "x", "availableModels": [{"modelId": "x", "name": "X"}, {"modelId": "y"}]}});
        assert_eq!(ids(&session_models(&old)), ["x", "y"]);
        assert!(session_models(&json!({})).is_empty());
    }

    #[test]
    fn codex_cache_lists_picker_models_by_priority() {
        let t = r#"{"models": [{"slug": "b", "display_name": "B", "visibility": "list", "priority": 2},
            {"slug": "hidden", "visibility": "hide", "priority": 0},
            {"slug": "a", "display_name": "A", "visibility": "list", "priority": 1}]}"#;
        assert_eq!(ids(&codex_cache(t)), ["a", "b"]);
        assert!(codex_cache("not json").is_empty());
    }

    #[test]
    fn claude_aliases_with_exact_ids_then_ids() {
        let m = claude(&["opus".into(), "fable".into()]);
        assert_eq!(ids(&m), ["default", "opus", "fable", "claude-fable-5-2", "claude-opus-5-5", "claude-sonnet-5-5", "claude-haiku-5-5"]);
        assert_eq!(m[1].name, "claude-opus-5-5");
        assert_eq!(ids(&lines("a/b\n\n  c/d \nnot a model\n")), ["a/b", "c/d"]);
    }

    #[test]
    fn probe_of_a_fake_acp_agent_and_its_end() {
        // a shell ACP agent: answers initialize and session/new, then hangs; the probe ends it
        let script = r#"read a; echo '{"jsonrpc":"2.0","method":"note","params":{}}'; echo '{"jsonrpc":"2.0","id":1,"result":{}}'; read b; echo '{"jsonrpc":"2.0","id":2,"result":{"sessionId":"s","models":{"availableModels":[{"modelId":"m1","name":"M 1"}]}}}'; sleep 60"#;
        let at = Instant::now();
        let m = acp(&["sh".into(), "-c".into(), script.into()], Duration::from_secs(5)).unwrap();
        assert_eq!(m, [Model { id: "m1".into(), name: "M 1".into() }]);
        assert!(at.elapsed() < Duration::from_secs(3));
        // 0 answer: the timeout, the process still goes
        let at = Instant::now();
        let e = acp(&["sh".into(), "-c".into(), "sleep 60".into()], Duration::from_millis(300)).unwrap_err();
        assert!(e.starts_with("initialize: 0 answer"), "{e}");
        assert!(at.elapsed() < Duration::from_secs(3));
        assert!(acp(&["/nonexistent/acp".into()], Duration::from_secs(1)).is_err());
        assert_eq!(run(&["sh".into(), "-c".into(), "echo a; echo b".into()], Duration::from_secs(5)).unwrap(), "a\nb\n");
    }

    #[test]
    #[ignore = "spawns the installed harnesses"]
    fn live_read() {
        for h in harnesses::installed() {
            let at = Instant::now();
            let r = read(h.name).map(|m| m.into_iter().map(|m| m.id).collect::<Vec<_>>());
            println!("{} ({} ms): {r:?}", h.name, at.elapsed().as_millis());
        }
    }

    #[test]
    fn cache_round_trip_and_age() {
        let e = store("probe-test", &[Model { id: "x".into(), name: String::new() }]);
        assert!(fresh(&e));
        assert_eq!(ids(&load()["probe-test"].models), ["x"]);
        assert!(!fresh(&Entry { at: now() - MAX_AGE.as_secs() - 1, models: Vec::new() }));
    }
}
