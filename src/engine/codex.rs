//! Codex: `codex app-server`, the CLI's own JSON-RPC protocol on stdio, 1 JSON message per line.
//! codex-acp (last release 0.16.0) bundles an old codex core: the API refuses every current model
//! with it ("requires a newer version of Codex", "not supported when using Codex with a `ChatGPT`
//! account"). The app server is the installed CLI itself: its models, its login.
//!
//! Flow: `initialize` → `initialized` → `thread/start` (or `thread/resume`, `thread/fork`) → 1
//! `turn/start` at a time. A turn streams `item/*` notifications (messages, reasoning, commands,
//! file changes, MCP calls) and ends with `turn/completed`. The app server asks
//! `item/commandExecution/requestApproval`, `item/fileChange/requestApproval`,
//! `mcpServer/elicitation/request` and `item/tool/requestUserInput`: codehead answers once the
//! owner (or the agent's mode) picks. `turn/interrupt` stops the running turn.

use std::collections::{HashMap, VecDeque};
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

pub(crate) struct Setup {
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
pub(crate) struct Conn(Arc<Mutex<State>>);

/// A JSON-RPC error E as text: its message, the API's own message after it.
fn error_text(error: &Value) -> String {
    let msg = error["message"].as_str().unwrap_or("error").to_string();
    let data = error["data"]["message"]
        .as_str()
        .or_else(|| error["data"].as_str())
        .unwrap_or_default();
    let inner = serde_json::from_str::<Value>(data)
        .ok()
        .and_then(|value| value["error"]["message"].as_str().map(String::from))
        .unwrap_or_else(|| data.to_string());

    if inner.is_empty() || inner == msg {
        msg
    } else {
        format!("{msg}: {inner}")
    }
}

/// The `mcp_servers` config of codehead's MCP server (EXE `mcp`, ENV) and the agent's servers
/// (ACP entries of `servers::acp_config`) → codex config entries.
pub(crate) fn mcp_config(exe: &str, env: &[(String, String)], acp: &[Value]) -> Value {
    let mut all = serde_json::Map::new();

    let pairs = |list: &Value| -> serde_json::Map<String, Value> {
        list.as_array()
            .into_iter()
            .flatten()
            .filter_map(|pair| Some((pair["name"].as_str()?.to_string(), pair["value"].clone())))
            .collect()
    };

    all.insert("codehead".into(), json!({"command": exe, "args": ["mcp"], "env": env.iter().map(|(key, value)| (key.clone(), json!(value))).collect::<serde_json::Map<_, _>>()}));

    for server in acp {
        let Some(name) = server["name"].as_str().filter(|name| *name != "codehead") else {
            continue;
        };

        let entry = match server["url"].as_str() {
            Some(url) => json!({"url": url, "http_headers": pairs(&server["headers"])}),
            None => json!({"command": server["command"], "args": server["args"], "env": pairs(&server["env"])}),
        };

        all.insert(name.to_string(), entry);
    }

    Value::Object(all)
}

/// A tool item of the app server → (codehead tool name, input). None: not a tool (messages,
/// reasoning, the user's own message).
fn tool_of(item: &Value) -> Option<(String, Value)> {
    let text = |key: &str| item[key].as_str().unwrap_or_default().to_string();

    Some(match item["type"].as_str()? {
        "commandExecution" => (
            "Bash".into(),
            json!({"command": text("command"), "cwd": item["cwd"], "_kind": "execute"}),
        ),
        "fileChange" => {
            let paths: Vec<String> = item["changes"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|change| change["path"].as_str().map(String::from))
                .collect();
            let mut input = json!({"_kind": "edit", "description": paths.join(", "), "locations": paths.iter().map(|path| json!({"path": path})).collect::<Vec<_>>()});

            if let Some(path) = paths.first() {
                input["file_path"] = json!(path);
            }

            ("Edit".into(), input)
        }
        "mcpToolCall" => (
            format!("mcp__{}__{}", text("server"), text("tool")),
            if item["arguments"].is_object() {
                item["arguments"].clone()
            } else {
                json!({})
            },
        ),
        "dynamicToolCall" => (
            text("tool"),
            if item["arguments"].is_object() {
                item["arguments"].clone()
            } else {
                json!({})
            },
        ),
        "webSearch" => ("WebSearch".into(), json!({"query": text("query"), "_kind": "fetch"})),
        "imageView" => ("Read".into(), json!({"file_path": text("path"), "_kind": "read"})),
        "collabAgentToolCall" => (
            "Agent".into(),
            json!({"description": text("tool"), "prompt": item["prompt"]}),
        ),
        _ => return None,
    })
}

/// A finished tool item → (ok, output).
fn result_of(item: &Value) -> (bool, String) {
    let status = item["status"].as_str().unwrap_or_default();
    let mut ok = !matches!(status, "failed" | "declined");

    let text = match item["type"].as_str().unwrap_or_default() {
        "commandExecution" => {
            ok &= item["exitCode"].as_i64().is_none_or(|code| code == 0);
            item["aggregatedOutput"].as_str().unwrap_or_default().to_string()
        }
        "fileChange" => item["changes"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|change| change["diff"].as_str().unwrap_or_default())
            .collect::<Vec<_>>()
            .join("\n"),
        "mcpToolCall" => match item["error"]["message"].as_str() {
            Some(error) => {
                ok = false;
                error.to_string()
            }
            None => item["result"]["content"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|content| content["text"].as_str())
                .collect::<Vec<_>>()
                .join("\n"),
        },
        "dynamicToolCall" => {
            ok &= item["success"].as_bool().unwrap_or(true);
            item["contentItems"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|content| content["text"].as_str())
                .collect::<Vec<_>>()
                .join("\n")
        }
        "webSearch" => item["query"].as_str().unwrap_or_default().to_string(),
        _ => String::new(),
    };

    if status == "declined" && text.is_empty() {
        return (false, "declined".into());
    }

    (ok, cap(text))
}

fn cap(mut text: String) -> String {
    if text.len() > 4096 {
        let mut end = 4096;

        while !text.is_char_boundary(end) {
            end -= 1;
        }

        text.truncate(end);
        text.push_str("\n…");
    }

    text
}

/// A token breakdown of `thread/tokenUsage/updated` → codehead's numbers: input without the
/// cached part (`OpenAI` counts it in `inputTokens`).
fn usage_of(breakdown: &Value) -> TurnUse {
    let count = |key: &str| breakdown[key].as_u64().unwrap_or(0);

    TurnUse {
        input: count("inputTokens").saturating_sub(count("cachedInputTokens")),
        output: count("outputTokens"),
        cache_read: count("cachedInputTokens"),
        cache_write: count("cacheWriteInputTokens"),
        ctx: count("totalTokens"),
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
pub(crate) fn start(
    stdin: ChildStdin,
    stdout: std::process::ChildStdout,
    stderr: Arc<Mutex<Vec<String>>>,
    setup: Setup,
    name: String,
    tx: Sender<(String, Out)>,
) -> Conn {
    let writer = crate::engine::harness::json_writer(stdin);
    let conn = Conn(Arc::new(Mutex::new(state(writer, setup, name, tx))));

    {
        let mut state = conn.0.lock().expect("fresh lock");
        let init = json!({"clientInfo": {"name": "codehead", "title": "codehead", "version": crate::infra::update::VERSION}, "capabilities": {"experimentalApi": true}});

        if let Err(error) = state.request("initialize", &init, Pending::Init) {
            state.out(Out::Event(Kind::Error {
                message: format!("codex: {error}"),
            }));
        }
    }

    conn.read_protocol(stdout, stderr.clone());
    conn.watch_setup(stderr);

    conn
}

impl super::harness::Driver for Conn {
    fn prompt(&mut self, text: &str) -> Result<(), String> {
        let mut state = self.0.lock().map_err(|error| error.to_string())?;

        state.queue.push_back(text.to_string());
        state.pump()
    }

    fn allow(&mut self, id: &str, input: &Value) -> Result<(), String> {
        self.0
            .lock()
            .map_err(|error| error.to_string())?
            .answer(id, true, input)
    }

    fn deny(&mut self, id: &str, _: &str) -> Result<(), String> {
        self.0
            .lock()
            .map_err(|error| error.to_string())?
            .answer(id, false, &Value::Null)
    }

    fn interrupt(&mut self) -> Result<(), String> {
        let mut state = self.0.lock().map_err(|error| error.to_string())?;

        // open asks first: cancelled
        let asks: Vec<(Value, Ask)> = state.asks.drain().map(|(_, ask)| ask).collect();

        for (rpc, kind) in asks {
            let result = match kind {
                Ask::Command | Ask::FileChange => json!({"decision": "cancel"}),
                Ask::Elicitation => json!({"action": "cancel"}),
                Ask::Questions(_) => json!({"answers": {}}),
            };

            state.send(&json!({"id": rpc, "result": result}))?;
        }

        state.queue.clear();

        match (state.thread.clone(), state.turn.clone()) {
            (Some(thread), Some(turn)) => state.request(
                "turn/interrupt",
                &json!({"threadId": thread, "turnId": turn}),
                Pending::Interrupt,
            ),
            _ => Ok(()),
        }
    }

    fn host_allows(&self) -> bool {
        true
    }
}

#[cfg(test)]
#[path = "codex_tests.rs"]
mod tests;

#[path = "codex_protocol.rs"]
mod codex_protocol;

#[path = "codex_events.rs"]
mod codex_events;

#[path = "codex_transport.rs"]
mod transport;
