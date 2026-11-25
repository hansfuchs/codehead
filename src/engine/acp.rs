//! Agent Client Protocol client: codehead drives any ACP agent (`OpenCode`, Goose, Cursor, Pi, …)
//! over JSON-RPC 2.0 on the agent's stdio, 1 JSON message per line.
//!
//! Flow: `initialize` → `session/new` (or `session/load` to resume) → optional
//! `session/set_model` → 1 `session/prompt` at a time. While a prompt runs, the agent streams
//! `session/update` notifications (message chunks, tool calls) and asks
//! `session/request_permission`; codehead answers that request once the owner (or the agent's
//! mode) picks an option. `session/cancel` stops the running prompt.

use std::collections::{HashMap, VecDeque};
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

pub(crate) struct Setup {
    pub cwd: String,
    pub resume: Option<String>,
    pub model: Option<String>,
    /// The codehead MCP server (command, args, env).
    pub mcp: (String, Vec<String>, Vec<(String, String)>),
    /// Sent before the 1st prompt of a new session: who the agent is, the thread.
    pub preamble: String,
    /// The harness name (status rows) and the agent's codehead MCP servers.
    pub harness: String,
    pub servers: Vec<super::servers::Server>,
}

struct State {
    /// Lines for the agent's stdin: a writer thread writes them (a full pipe never blocks the
    /// reader, which needs this state).
    stdin: Sender<String>,
    next: u64,
    pending: HashMap<u64, Pending>,
    session: Option<String>,
    /// Loads replay the history as updates: 0 events from them.
    loading: bool,
    /// `mcpServers` of `session/new` / `session/load`, set on the `initialize` reply.
    capabilities: Capabilities,
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
    /// `Out::Exit` went out: 1 per session, a later one would end the next session of the agent.
    dead: bool,
}

#[derive(Clone)]
pub(crate) struct Conn(Arc<Mutex<State>>);

/// A JSON-RPC error E as text: its message, the server's own message (`data.message`, e.g. the
/// model API's 400) after it.
fn error_text(error: &Value) -> String {
    let msg = error["message"].as_str().unwrap_or("error").to_string();
    let data = error["data"]["message"]
        .as_str()
        .or_else(|| error["data"].as_str())
        .unwrap_or_default();

    // `data.message` may hold the API's error JSON: its `error.message`
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

/// The model of a new session R and whether it sets models as config options: `configOptions`
/// (id `model`, ACP config options) or the older `models.currentModelId`.
fn session_model(result: &Value) -> (String, bool) {
    let opt = result["configOptions"].as_array().and_then(|output| {
        output
            .iter()
            .find(|character| character["id"] == "model" || character["category"] == "model")
    });

    match opt {
        Some(output_value) => (
            output_value["currentValue"].as_str().unwrap_or_default().to_string(),
            true,
        ),
        None => (
            result["models"]["currentModelId"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
            false,
        ),
    }
}

/// `mcpServers`: codehead's own, then the agent's servers the harness can run (HTTP, SSE: its
/// `mcpCapabilities`); the rest: (server, why).
fn mcp_servers(text: &Setup, http: bool, sse: bool) -> (Value, Vec<(String, String)>) {
    let (cmd, args, env) = &text.mcp;
    let own = json!({"name": "codehead", "command": cmd, "args": args, "env": env.iter().map(|(key, value)| json!({"name": key, "value": value})).collect::<Vec<_>>()});
    let (more, errors) = super::servers::acp_config(&text.servers, http, sse);

    (Value::Array(std::iter::once(own).chain(more).collect()), errors)
}

/// ACP tool call → (codehead tool name, input): its kind as a familiar name, the raw input
/// with the title as description.
pub(crate) fn tool_of(tc: &Value) -> (String, Value) {
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

    if let Some(output) = input.as_object_mut()
        && !output.contains_key("description")
        && !title.is_empty()
    {
        output.insert("description".into(), Value::String(title.clone()));
    }

    let name = if name.is_empty() {
        if title.is_empty() { "tool".to_string() } else { title }
    } else {
        name.to_string()
    };

    (name, input)
}

/// Start the protocol on a spawned agent: stdout is read here, events go to TX.
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
        let mut text_value = conn.0.lock().expect("fresh lock");
        let init = json!({"protocolVersion": 1, "clientCapabilities": {"fs": {"readTextFile": false, "writeTextFile": false}, "terminal": false},
            "clientInfo": {"name": "codehead", "version": crate::infra::update::VERSION}});

        if let Err(error) = text_value.request("initialize", &init, Pending::Init) {
            text_value.out(Out::Event(Kind::Error {
                message: format!("acp: {error}"),
            }));
        }
    }

    conn.read_protocol(stdout, stderr.clone());
    conn.watch_setup(stderr);

    conn
}

/// Time for `initialize` + `session/new` (or `session/load`) to give a session.
const SETUP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);

impl Conn {
    pub(crate) fn prompt(&self, text: &str) -> Result<(), String> {
        let mut text_value = self.0.lock().map_err(|error| error.to_string())?;

        text_value.queue.push_back(text.to_string());
        text_value.pump()
    }

    /// Answer ask ID with the option OPTION (an option id), else the 1st option of KINDS.
    pub(crate) fn answer(&self, id: &str, option: Option<&str>, kinds: &[&str]) -> Result<(), String> {
        let mut text = self.0.lock().map_err(|error| error.to_string())?;
        let (rpc, options) = text.asks.remove(id).ok_or_else(|| format!("no open ask {id}"))?;

        let pick = option.map(String::from).or_else(|| permission_option(&options, kinds));

        let outcome = match pick {
            Some(output_2) => json!({"outcome": "selected", "optionId": output_2}),
            None => json!({"outcome": "cancelled"}),
        };

        text.send(&json!({"jsonrpc": "2.0", "id": rpc, "result": {"outcome": outcome}}))
    }

    pub(crate) fn cancel(&self) -> Result<(), String> {
        let mut text = self.0.lock().map_err(|error| error.to_string())?;

        // open asks first: a cancelled prompt answers them with `cancelled`
        let asks: Vec<(Value, Vec<Value>)> = text.asks.drain().map(|(_, agent)| agent).collect();

        for (rpc, _) in asks {
            text.send(&json!({"jsonrpc": "2.0", "id": rpc, "result": {"outcome": {"outcome": "cancelled"}}}))?;
        }

        text.queue.clear();
        let sid = text.session.clone().unwrap_or_default();

        text.send(&json!({"jsonrpc": "2.0", "method": "session/cancel", "params": {"sessionId": sid}}))
    }
}

#[cfg(test)]
#[path = "acp_tests.rs"]
mod tests;

#[path = "acp_protocol.rs"]
mod acp_protocol;

#[path = "acp_events.rs"]
mod acp_events;

#[derive(Default)]
struct Capabilities {
    load_session: bool,
    mcp: Value,
}

fn state(wtx: Sender<String>, setup: Setup, name: String, tx: Sender<(String, Out)>) -> State {
    let preamble = Some(setup.preamble.clone());

    State {
        stdin: wtx,
        next: 0,
        pending: HashMap::new(),
        session: None,
        loading: false,
        capabilities: Capabilities::default(),
        busy: false,
        queue: VecDeque::new(),
        preamble,
        text: String::new(),
        thought: String::new(),
        asks: HashMap::new(),
        setup,
        tx,
        name,
        dead: false,
    }
}

#[path = "acp_transport.rs"]
mod transport;

fn permission_option(options: &[Value], kinds: &[&str]) -> Option<String> {
    kinds.iter().find_map(|key| {
        options
            .iter()
            .find(|output| output["kind"] == *key)
            .and_then(|output_value| output_value["optionId"].as_str().map(String::from))
    })
}
