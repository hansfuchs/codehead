//! The UI side of a thread host: start it when it is down, send it commands.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::Value;

use super::host::socket;

/// A host serves thread ID.
pub fn alive(id: &str) -> bool {
    UnixStream::connect(socket(id)).is_ok()
}

/// Start the host of thread ID (detached: it outlives this terminal) unless one runs; wait
/// until it serves.
pub fn ensure(id: &str) -> Result<(), String> {
    ensure_with(id, &super::exe())
}

/// `ensure` with the host binary EXE (perf suite: the release binary, not the test binary).
pub fn ensure_with(id: &str, exe: &Path) -> Result<(), String> {
    if alive(id) {
        return Ok(());
    }
    let (mut child, log) = spawn_host(id, exe)?;
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(5) {
        if alive(id) {
            // reaped by nobody otherwise: a thread waits for it
            std::thread::spawn(move || child.wait());
            return Ok(());
        }
        if let Ok(Some(s)) = child.try_wait() {
            return Err(format!("codehead host exited ({s}). log: {}", log.display()));
        }
        // a host binds its socket in a few ms: short steps, 0 lost frame budget
        std::thread::sleep(Duration::from_millis(2));
    }
    Err(format!("codehead host did not start. log: {}", log.display()))
}

/// Start the host of thread ID unless one runs, 0 wait for it (a new thread: the host sends the
/// task itself, the view subscribes once the socket is there).
pub fn start(id: &str) -> Result<(), String> {
    start_with(id, &super::exe())
}

/// `start` with the host binary EXE.
pub fn start_with(id: &str, exe: &Path) -> Result<(), String> {
    if alive(id) {
        return Ok(());
    }
    let (mut child, _) = spawn_host(id, exe)?;
    std::thread::spawn(move || child.wait());
    Ok(())
}

/// The host process of thread ID (detached, in its own session), its log file.
fn spawn_host(id: &str, exe: &Path) -> Result<(std::process::Child, std::path::PathBuf), String> {
    let logs = super::logs_dir();
    std::fs::create_dir_all(&logs).map_err(|e| e.to_string())?;
    let log = logs.join(format!("host-{id}.log"));
    let file = std::fs::OpenOptions::new().create(true).append(true).open(&log).map_err(|e| format!("{}: {e}", log.display()))?;
    let mut c = Command::new(exe);
    c.args(["host", id]).stdin(Stdio::null()).stdout(file.try_clone().map_err(|e| e.to_string())?).stderr(file);
    // a test's host: the test's data and run dirs (0 env var in prod: an agent's own `cargo
    // test` would inherit them and write into the owner's data dir)
    if cfg!(test) {
        c.env("CODEHEAD_HOME", super::data_dir()).env("CODEHEAD_RUN", super::run_dir());
    }
    // SAFETY: setsid is async-signal-safe; runs in the child before exec.
    unsafe {
        c.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    let child = c.spawn().map_err(|e| format!("codehead host: {e}"))?;
    Ok((child, log))
}

/// Command C to the host of thread ID, which is up. Err: 0 host, or the host's error.
pub fn send(id: &str, c: &Value) -> Result<Value, String> {
    send_within(id, c, Duration::from_secs(30))
}

/// `send` with a reply deadline of WAIT: a busy host never blocks a list.
pub fn send_within(id: &str, c: &Value, wait: Duration) -> Result<Value, String> {
    let mut s = UnixStream::connect(socket(id)).map_err(|e| format!("host of {id}: {e}"))?;
    s.set_read_timeout(Some(wait)).ok();
    writeln!(s, "{c}").map_err(|e| e.to_string())?;
    let mut line = String::new();
    BufReader::new(&s).read_line(&mut line).map_err(|e| e.to_string())?;
    let v: Value = serde_json::from_str(&line).map_err(|e| format!("host reply: {e}"))?;
    if v["ok"].as_bool() == Some(true) { Ok(v) } else { Err(v["error"].as_str().unwrap_or("host error").to_string()) }
}

/// Command C to thread ID's host, started first if down.
pub fn call(id: &str, c: &Value) -> Result<Value, String> {
    ensure(id)?;
    send(id, c)
}

/// Live and busy agents of thread ID (empty: 0 host).
pub fn state(id: &str) -> (Vec<String>, Vec<String>) {
    let s = state_full(id);
    (s.live, s.busy)
}

/// The host's state of thread ID (empty: 0 host).
#[derive(Default)]
pub struct State {
    pub live: Vec<String>,
    pub busy: Vec<String>,
    /// Per agent in a turn: its tokens so far.
    pub usage: std::collections::HashMap<String, super::harness::TurnUse>,
}

/// Live and busy agents of thread ID, the running turns' tokens (1 ping).
pub fn state_full(id: &str) -> State {
    state_known(id).unwrap_or_default()
}

/// `state_full`, None when a host runs but gives 0 answer in time (busy): its state unknown.
pub fn state_known(id: &str) -> Option<State> {
    let v = match send_within(id, &serde_json::json!({"op": "ping"}), Duration::from_millis(500)) {
        Ok(v) => v,
        // 0 host: 0 live agent
        Err(_) if !alive(id) => return Some(State::default()),
        Err(_) => return None,
    };
    let names = |k: &str| v[k].as_array().into_iter().flatten().filter_map(Value::as_str).map(String::from).collect();
    Some(State { live: names("live"), busy: names("busy"), usage: serde_json::from_value(v["usage"].clone()).unwrap_or_default() })
}
