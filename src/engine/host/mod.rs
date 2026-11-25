//! `codehead host <thread>`: the detached process that owns a thread. It keeps 1 harness session
//! per working agent, writes every event to the thread's log, and takes commands on a Unix socket
//! (`<data>/run/<thread>.sock`, 1 JSON object per line, 1 reply line each).
//!
//! Commands (`op`):
//! - `prompt` {text, to?}: an owner message. TO, else the @names in TEXT, else the lead.
//! - `send` {from, to, text}: an agent's message (the `send` tool).
//! - `answer` {id, allow, answers?, option?, label}: the owner's pick for an open ask
//!   (OPTION: an ACP agent's option id).
//! - `interrupt`: stop the running turns.
//! - `join` {agent, text?}, `leave` {agent}: thread members.
//! - `status` {agent, state, note}: the `set_status` tool.
//! - `reload` {agent, fresh?}: end the agent's idle session (its profile changed).
//! - `ping`: live / busy agents, open asks.
//! - `subscribe`: the connection stays open; the host writes 1 JSON line per push: `live`
//!   {agent, text} (the live text so far, once), `delta` {agent, text} (a token chunk), `wake`
//!   (new log lines).
//! - `stop`: end every session, exit.
//!
//! Sessions start on the first message to their agent and resume the agent's last harness
//! session. 15 min. without a turn, an open ask or a command: the host exits (sessions resume
//! on the next message).

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::ops::ControlFlow;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Sender};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::roster;
use crate::engine::harness::{self, Out, Session};
use crate::engine::store::{self, Event, Kind};

const IDLE: Duration = Duration::from_mins(15);
/// Error of a turn the owner interrupted (the stream shows it as a note, not an error).
pub(crate) const INTERRUPTED: &str = "interrupted";

pub(crate) fn socket(id: &str) -> PathBuf {
    super::run_dir().join(format!("{id}.sock"))
}

enum Input {
    Cmd(Value, Sender<Value>),
    /// A view subscribed: lines for it (`wake`, `delta`) go to this sender.
    Subscribe(Sender<String>),
    Harness(String, Out),
    /// The worktree setup ended: its label, ok, output.
    Setup(String, bool, String),
    /// The pull of main before the 1st turn ended: ok, its last line.
    Pull(bool, String),
    /// The new thread's worktree: (its path, its port) or the error.
    Worktree(Result<(PathBuf, u16), String>),
}

struct Live {
    session: Session,
    busy: bool,
    /// The owner interrupted the running turn: its end is no error.
    interrupted: bool,
    /// The agent's MCP servers at the session's start (`servers::stamp`): another stamp at the
    /// next message → a new process (resumed) with the new set.
    mcp: String,
}

struct Host {
    id: String,
    exe: PathBuf,
    sessions: HashMap<String, Live>,
    /// Open asks: request id → (agent, request).
    asks: HashMap<String, (String, Value)>,
    harness_tx: Sender<(String, Out)>,
    last: Instant,
    /// `stop` came: exit after its reply.
    stopping: bool,
    /// Open views (`subscribe`): each gets `wake` per log line and the live text chunks.
    subs: std::cell::RefCell<Vec<Sender<String>>>,
    /// Per agent: live text since its last logged message: a new subscriber gets it first.
    live: HashMap<String, String>,
    /// Per agent in a turn: its tokens so far (`ping` gives them to the cost box).
    turn_use: HashMap<String, crate::engine::harness::TurnUse>,
    /// The worktree setup (or the pull of main) runs: messages wait in QUEUE (agent, text)
    /// until it ends.
    setup_running: bool,
    queue: Vec<(String, String)>,
    /// Messages of the last command (agent, text): delivered after its reply. A harness start
    /// and the edits snapshot take a second in a big repo; the sender never waits for them.
    later: Vec<(String, String)>,
    /// Subagents of the agents (their start, steps, end into the log).
    subagents: crate::engine::subagents::Tracker,
    sub_scan: Instant,
    /// Agents stopped on a usage limit: when each gets `continue` (unix ms; thread.json keeps it).
    resume: HashMap<String, u64>,
    /// Messages to this thread's agents while a sub-thread of it runs (same checkout): they go
    /// at its `sub_done`.
    held: Vec<(String, String)>,
    /// This thread as a sub-thread: its end.
    kid: Kid,
    /// The last look at this thread's running sub-threads (`kids_check`).
    kids_at: Instant,
}

/// A sub-thread whose host is down this long after its start: lost.
const KID_LOST: u64 = 20_000;

/// A sub-thread's run: turns ended, the error that failed it, idle since (its end: idle a while
/// after 1+ turns).
#[derive(Default)]
struct Kid {
    turns: usize,
    failed: Option<String>,
    idle_at: Option<Instant>,
}

/// A sub-thread idle this long after its last turn: done (an agent's `send` starts the next
/// turn within it).
const KID_DONE: Duration = Duration::from_secs(3);

/// A sub-thread's result to its thread: at most this many chars.
const KID_TEXT: usize = 20_000;

/// The text an agent gets when its usage window reset.
pub(crate) const RESUME_TEXT: &str = "[codehead: your usage limit window reset. Continue the task where you stopped.]";

/// When a usage limit of error ERR ends (unix ms): the claude plan window at 100 % that resets
/// last, else the `resets 1:50pm` of ERR (local time, the next one), else 30 min; + 1 min.
pub(crate) fn resume_at(err: &str, now: u64) -> u64 {
    let full = crate::engine::limits::read()
        .into_iter()
        .filter(|width| width.harness == "claude" && width.pct >= 99.5)
        .filter_map(|width_value| width_value.resets)
        .map(|result| result * 1000)
        .filter(|&result_value| result_value > now)
        .max();

    let parsed = || {
        static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();

        let character = RE
            .get_or_init(|| {
                regex::Regex::new(r"(?i)resets\s+(?:at\s+)?(\d{1,2})(?::(\d{2}))?\s*(am|pm)?").expect("valid regex")
            })
            .captures(err)?;

        let mut height: i32 = character[1].parse().ok()?;
        let message_value: i32 = character
            .get(2)
            .map_or(Some(0), |message| message.as_str().parse().ok())?;

        match character.get(3).map(|x| x.as_str().to_ascii_lowercase()) {
            Some(ap) if ap == "pm" && height < 12 => height += 12,
            Some(ap) if ap == "am" && height == 12 => height = 0,
            _ => {}
        }

        let secs = (now / 1000) as libc::time_t;

        // SAFETY: localtime_r / mktime read and write only TM.
        let mut tm: libc::tm = unsafe { std::mem::zeroed() };

        unsafe { libc::localtime_r(&raw const secs, &raw mut tm) };
        tm.tm_hour = height;
        tm.tm_min = message_value;
        tm.tm_sec = 0;
        let mut at = unsafe { libc::mktime(&raw mut tm) } as i64 * 1000;

        if at <= now as i64 {
            at += 86_400_000;
        }

        u64::try_from(at).ok()
    };

    full.or_else(parsed).unwrap_or(now + 30 * 60_000) + 60_000
}

/// @names in TEXT that name an active agent, in order, once each.
pub(crate) fn mentions(text: &str, names: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();

    for (i, _) in text.match_indices('@') {
        if i > 0
            && text[..i]
                .chars()
                .next_back()
                .is_some_and(|character| character.is_alphanumeric() || character == '`')
        {
            continue;
        }

        let word: String = text[i + 1..]
            .chars()
            .take_while(|character_value| {
                character_value.is_ascii_alphanumeric() || *character_value == '-' || *character_value == '_'
            })
            .collect();

        if let Some(count_value) = names.iter().find(|count| count.eq_ignore_ascii_case(&word))
            && !out.contains(count_value)
        {
            out.push(count_value.clone());
        }
    }

    out
}

// ---------- flows ----------

/// A flow move's result for the agent that reported (the `flow_step` tool's text).
fn moved_reply(text: Option<&String>, handed: Option<&String>, wait: Option<&str>) -> Value {
    json!({"text": text, "handed": handed, "wait": wait})
}

/// The end of a turn whose session ended (stop, crash, exit): 0 cost known.
fn session_ended() -> Kind {
    Kind::TurnEnd {
        cost: 0.0,
        input: 0,
        output: 0,
        cache_read: 0,
        cache_write: 0,
        ctx: 0,
        window: 0,
        ms: 0,
        error: Some("session ended".into()),
    }
}

/// SIGTERM / SIGINT / SIGHUP came: the host loop ends its sessions and exits.
static SIGNALED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

extern "C" fn on_signal(_: libc::c_int) {
    SIGNALED.store(true, std::sync::atomic::Ordering::Relaxed);
}

fn catch_signals() {
    for text in [libc::SIGTERM, libc::SIGINT, libc::SIGHUP] {
        // SAFETY: the handler only stores to an atomic (async-signal-safe).
        unsafe { libc::signal(text, on_signal as *const () as libc::sighandler_t) };
    }
}

fn serve(conn: UnixStream, tx: &Sender<Input>) {
    let Ok(mut width) = conn.try_clone() else { return };

    for line in BufReader::new(conn).lines().map_while(Result::ok) {
        let reply = match serde_json::from_str::<Value>(&line) {
            // a view subscribes: this connection carries the host's lines from now on
            Ok(character) if character["op"] == "subscribe" => {
                serve_subscription(tx, &mut width);
                return;
            }
            Ok(character_value) => match serve_ok_character_value(tx, character_value) {
                Some(value) => value,
                None => return,
            },
            Err(error) => json!({"ok": false, "error": format!("bad json: {error}")}),
        };

        if writeln!(width, "{reply}").is_err() {
            return;
        }
    }
}

fn serve_ok_character_value(tx: &Sender<Input>, character_value: Value) -> Option<Value> {
    let (rtx, rrx) = mpsc::channel();

    if let ControlFlow::Break(()) = send_command_input(tx, character_value, rtx) {
        return None;
    }

    Some(
        rrx.recv()
            .unwrap_or_else(|_| json!({"ok": false, "error": "host stopped"})),
    )
}

fn send_command_input(tx: &Sender<Input>, character_value: Value, rtx: Sender<Value>) -> ControlFlow<()> {
    if tx.send(Input::Cmd(character_value, rtx)).is_err() {
        return ControlFlow::Break(());
    }

    ControlFlow::Continue(())
}

fn serve_subscription(tx: &Sender<Input>, width: &mut UnixStream) {
    let (stx, srx) = mpsc::channel::<String>();

    if tx.send(Input::Subscribe(stx)).is_err() || writeln!(width, "{}", json!({"ok": true})).is_err() {
        return;
    }

    for line_value in srx {
        if writeln!(width, "{line_value}").is_err() {
            return;
        }
    }
}

/// Bind the socket of thread ID. Err: another host serves it (or the bind failed).
/// The lock file (held for the host's life) closes the race of 2 hosts started at once.
fn bind(id: &str) -> Result<(UnixListener, std::fs::File), String> {
    let path = socket(id);

    std::fs::create_dir_all(super::run_dir()).map_err(|error| error.to_string())?;
    let lock = super::lock(&super::run_dir().join(format!("{id}.lock")), false)
        .map_err(|_| "a host serves this thread".to_string())?;

    if UnixStream::connect(&path).is_ok() {
        return Err("a host serves this thread".into());
    }

    let _ = std::fs::remove_file(&path);
    let line = UnixListener::bind(&path).map_err(|error_value| format!("{}: {error_value}", path.display()))?;

    Ok((line, lock))
}

/// Run the host of thread ID until idle or `stop`.
/// Worktree WT stays on BRANCH (`crate::infra::branch_pin`); a failure: in the host's log.
fn pin_branch(wt: &Path, branch: &str) {
    if let Err(error) = crate::infra::branch_pin::pin(wt, branch) {
        eprintln!("codehead host: pin {} to {branch}: {error}", wt.display());
    }
}

mod runtime;
pub(crate) use runtime::run;

#[cfg(test)]
#[path = "host_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "host_resume_tests.rs"]
mod resume_tests;

mod sessions;

mod resume;

mod commands;

mod events;

mod setup;

mod flows;

mod children;
