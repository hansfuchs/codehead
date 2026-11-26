//! 1 harness process per agent and thread. Claude Code: `claude -p` with stream-json in and out;
//! prompts go in as user messages on stdin (the process queues them while a turn runs),
//! permission requests and questions come out as `control_request`s, answered on stdin with a
//! `control_response` once the owner picks. Codex: `codex app-server` (`engine::codex`). Every
//! other harness: ACP (`engine::acp`).

use std::io::{BufRead, BufReader, Write};
use std::ops::ControlFlow;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};

use std::sync::mpsc::Sender;

use serde_json::{Value, json};

use super::roster::Agent;
use super::store::Kind;

/// What a session reports to the host.
#[derive(Debug)]
pub(crate) enum Out {
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
    /// MCP servers as a harness saw them (harness, rows): for the settings' MCP tab.
    Mcp(String, Vec<(String, super::servers::Status)>),
    /// Tokens of the running turn so far: the cost box shows them live, the turn end replaces
    /// them with its final numbers.
    Usage(TurnUse),
}

/// Tokens of a running turn so far. CTX: the context of the last model call (0: unknown).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct TurnUse {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    pub ctx: u64,
}

/// Model prices (`[prices]` of the config) for a new session: edits apply to the next one. Tests:
/// 0 prices, never the machine's config.
pub(super) fn price_table() -> Prices {
    if cfg!(test) {
        Prices::new()
    } else {
        crate::infra::config::prices()
    }
}

pub(super) type Prices = std::collections::BTreeMap<String, crate::infra::config::Price>;

/// $ of USAGE (input without cache reads) on MODEL by PRICES: 0 without a price.
pub(super) fn priced_usd(prices: &Prices, model: &str, usage: TurnUse) -> f64 {
    crate::infra::config::price_of(prices, model).map_or(0.0, |price| {
        price.usd(crate::infra::config::Tokens {
            input: usage.input,
            cached: usage.cache_read,
            cache_write: usage.cache_write,
            output: usage.output,
        })
    })
}

/// codehead servers the harness did not get (ERRORS: server, why) → status rows, via TX.
pub(super) fn report_mcp_errors(harness: &str, errors: &[(String, String)], tx: &Sender<(String, Out)>, agent: &str) {
    if errors.is_empty() {
        return;
    }

    let at = super::now_ms();

    let rows = errors
        .iter()
        .map(|(server, why)| {
            (
                server.clone(),
                super::servers::Status {
                    state: "failed".into(),
                    error: why.clone(),
                    source: "codehead".into(),
                    at,
                    ..Default::default()
                },
            )
        })
        .collect();

    let _ = tx.send((agent.to_string(), Out::Mcp(harness.to_string(), rows)));
}

pub(crate) struct Spec<'a> {
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
    /// More dirs the agent may work in (the thread's other checkouts).
    pub extra: &'a [PathBuf],
}

pub(crate) struct Session {
    pub child: Child,
    driver: Box<dyn Driver>,
}

/// The protocol of a session on the harness's stdin: Claude Code's stream-json or ACP.
pub(crate) trait Driver {
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
    fn write(&mut self, value: &Value) -> Result<(), String> {
        let mut line = value.to_string();

        line.push('\n');
        self.0
            .write_all(line.as_bytes())
            .and_then(|()| self.0.flush())
            .map_err(|error| error.to_string())
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
        self.write(
            &json!({"type": "control_request", "request_id": super::new_id(), "request": {"subtype": "interrupt"}}),
        )
    }
}

impl Driver for super::acp::Conn {
    fn prompt(&mut self, text: &str) -> Result<(), String> {
        super::acp::Conn::prompt(self, text)
    }

    fn allow(&mut self, id: &str, input: &Value) -> Result<(), String> {
        self.answer(id, input["_option"].as_str(), &["allow_once", "allow_always"])
    }

    fn deny(&mut self, id: &str, _: &str) -> Result<(), String> {
        self.answer(id, None, &["reject_once", "reject_always"])
    }

    fn interrupt(&mut self) -> Result<(), String> {
        self.cancel()
    }

    fn host_allows(&self) -> bool {
        true
    }
}

/// Output of a tool result kept in the log.
const OUTPUT_CAP: usize = 4096;

impl Session {
    /// The host answers this session's permission requests per the agent's mode (ACP).
    pub(crate) fn host_allows(&self) -> bool {
        self.driver.host_allows()
    }

    pub(crate) fn prompt(&mut self, text: &str) -> Result<(), String> {
        self.driver.prompt(text)
    }

    pub(crate) fn allow(&mut self, id: &str, input: &Value) -> Result<(), String> {
        self.driver.allow(id, input)
    }

    pub(crate) fn deny(&mut self, id: &str, why: &str) -> Result<(), String> {
        self.driver.deny(id, why)
    }

    pub(crate) fn interrupt(&mut self) -> Result<(), String> {
        self.driver.interrupt()
    }

    /// End the harness and its children (its process group): SIGTERM, 2 s grace, SIGKILL.
    pub(crate) fn kill(&mut self) {
        let group = -(self.child.id() as i32);

        // SAFETY: kill(2) on the process group this session's spawn created (process_group(0)).
        unsafe { libc::kill(group, libc::SIGTERM) };
        let started = std::time::Instant::now();

        wait_for_exit(started, self);

        // SAFETY: as above; children left after the leader's exit go too.
        unsafe { libc::kill(group, libc::SIGKILL) };
        let _ = self.child.wait();
    }
}

/// stream-json lines → events. Keeps what spans lines: the last cost total, the context size.
#[derive(Default)]
pub(crate) struct Parser {
    /// `total_cost_usd` of the last result: the process reports a running total.
    cost_total: f64,
    /// Context tokens of the last assistant message.
    ctx: u64,
    /// Model of the session (`init` line): its `modelUsage` entry gives the context window.
    model: String,
    /// The running turn: usage per model call (message id; the blocks of 1 message repeat it).
    turn: Vec<(String, TurnUse)>,
}

#[cfg(test)]
#[path = "harness_tests.rs"]
mod tests;

#[path = "harness_spawn.rs"]
mod harness_spawn;
pub(crate) use harness_spawn::*;

#[path = "harness_parser.rs"]
mod harness_parser;

#[path = "harness_prompt.rs"]
mod harness_prompt;
pub(crate) use harness_prompt::*;

fn stop_on_exit(session: &mut Session) -> ControlFlow<()> {
    if !matches!(session.child.try_wait(), Ok(None)) {
        return ControlFlow::Break(());
    }

    ControlFlow::Continue(())
}

fn wait_for_exit(started: std::time::Instant, session: &mut Session) {
    while started.elapsed() < std::time::Duration::from_secs(2) {
        if let ControlFlow::Break(()) = stop_on_exit(session) {
            break;
        }

        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}
