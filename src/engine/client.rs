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
pub(crate) fn alive(id: &str) -> bool {
    UnixStream::connect(socket(id)).is_ok()
}

/// The threads whose host serves: the sockets in DIR (the run dir) that take a connection.
fn hosts_in(dir: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };

    let mut ids: Vec<String> = entries
        .flatten()
        .filter_map(|entry| {
            entry
                .file_name()
                .to_str()
                .and_then(|name| name.strip_suffix(".sock"))
                .map(String::from)
        })
        .filter(|id| UnixStream::connect(dir.join(format!("{id}.sock"))).is_ok())
        .collect();

    ids.sort();
    ids
}

/// Stops every host (`stop`: each ends its harness sessions; their running subagents end too)
/// and waits up to WAIT for them to end. Per host: (thread id, its error; None: ended).
pub(crate) fn stop_all(wait: Duration) -> Vec<(String, Option<String>)> {
    stop_all_in(&super::run_dir(), wait)
}

/// `stop_all` of the hosts with sockets in DIR.
fn stop_all_in(dir: &Path, wait: Duration) -> Vec<(String, Option<String>)> {
    let sock = |id: &str| dir.join(format!("{id}.sock"));
    let up = |id: &str| UnixStream::connect(sock(id)).is_ok();

    let mut out: Vec<(String, Option<String>)> = hosts_in(dir)
        .into_iter()
        .map(|id| {
            let error = send_at(
                &sock(&id),
                &id,
                &serde_json::json!({"op": "stop"}),
                Duration::from_secs(3),
            )
            .err();

            (id, error)
        })
        .collect();

    let until = Instant::now() + wait;

    while out.iter().any(|(id, error)| error.is_none() && up(id)) && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(100));
    }

    for (id, error) in &mut out {
        if error.is_none() && up(id) {
            *error = Some(format!("still runs after {} s", wait.as_secs()));
        }
    }

    out
}

/// Start the host of thread ID (detached: it outlives this terminal) unless one runs; wait
/// until it serves.
pub(crate) fn ensure(id: &str) -> Result<(), String> {
    ensure_with(id, &super::exe())
}

/// `ensure` with the host binary EXE (perf suite: the release binary, not the test binary).
pub(crate) fn ensure_with(id: &str, exe: &Path) -> Result<(), String> {
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

        if let Ok(Some(status)) = child.try_wait() {
            return Err(format!("codehead host exited ({status}). log: {}", log.display()));
        }

        // a host binds its socket in a few ms: short steps, 0 lost frame budget
        std::thread::sleep(Duration::from_millis(2));
    }

    Err(format!("codehead host did not start. log: {}", log.display()))
}

/// Start the host of thread ID unless one runs, 0 wait for it (a new thread: the host sends the
/// task itself, the view subscribes once the socket is there).
pub(crate) fn start(id: &str) -> Result<(), String> {
    start_with(id, &super::exe())
}

/// `start` with the host binary EXE.
pub(crate) fn start_with(id: &str, exe: &Path) -> Result<(), String> {
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

    std::fs::create_dir_all(&logs).map_err(|error| error.to_string())?;
    let log = logs.join(format!("host-{id}.log"));
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log)
        .map_err(|error| format!("{}: {error}", log.display()))?;
    let mut command = Command::new(exe);

    command
        .args(["host", id])
        .stdin(Stdio::null())
        .stdout(file.try_clone().map_err(|error| error.to_string())?)
        .stderr(file);

    // a test's host: the test's data and run dirs (0 env var in prod: an agent's own `cargo
    // test` would inherit them and write into the owner's data dir)
    if cfg!(test) {
        command
            .env("CODEHEAD_HOME", super::data_dir())
            .env("CODEHEAD_RUN", super::run_dir());
    }

    // SAFETY: setsid is async-signal-safe; runs in the child before exec.
    unsafe {
        command.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }

    let child = command.spawn().map_err(|error| format!("codehead host: {error}"))?;

    Ok((child, log))
}

/// Command COMMAND to the host of thread ID, which is up. Err: 0 host, or the host's error.
pub(crate) fn send(id: &str, command: &Value) -> Result<Value, String> {
    send_within(id, command, Duration::from_secs(30))
}

/// `send` with a reply deadline of WAIT: a busy host never blocks a list.
pub(crate) fn send_within(id: &str, command: &Value, wait: Duration) -> Result<Value, String> {
    send_at(&socket(id), id, command, wait)
}

/// `send_within` to the host socket SOCK (of thread ID).
fn send_at(sock: &Path, id: &str, command: &Value, wait: Duration) -> Result<Value, String> {
    let mut stream = UnixStream::connect(sock).map_err(|error| format!("host of {id}: {error}"))?;

    stream.set_read_timeout(Some(wait)).ok();
    writeln!(stream, "{command}").map_err(|error| error.to_string())?;
    let mut line = String::new();

    BufReader::new(&stream)
        .read_line(&mut line)
        .map_err(|error| error.to_string())?;
    let value: Value = serde_json::from_str(&line).map_err(|error| format!("host reply: {error}"))?;

    if value["ok"].as_bool() == Some(true) {
        Ok(value)
    } else {
        Err(value["error"].as_str().unwrap_or("host error").to_string())
    }
}

/// Command COMMAND to thread ID's host, started first if down.
pub(crate) fn call(id: &str, command: &Value) -> Result<Value, String> {
    ensure(id)?;
    send(id, command)
}

/// Live and busy agents of thread ID (empty: 0 host).
pub(crate) fn state(id: &str) -> (Vec<String>, Vec<String>) {
    let state = state_full(id);

    (state.live, state.busy)
}

/// The host's state of thread ID (empty: 0 host).
#[derive(Default)]
pub(crate) struct State {
    pub live: Vec<String>,
    pub busy: Vec<String>,
    /// Per agent in a turn: its tokens so far.
    pub usage: std::collections::HashMap<String, super::harness::TurnUse>,
}

/// Live and busy agents of thread ID, the running turns' tokens (1 ping).
pub(crate) fn state_full(id: &str) -> State {
    state_known(id).unwrap_or_default()
}

/// `state_full`, None when a host runs but gives 0 answer in time (busy): its state unknown.
pub(crate) fn state_known(id: &str) -> Option<State> {
    let reply = match send_within(id, &serde_json::json!({"op": "ping"}), Duration::from_millis(500)) {
        Ok(value) => value,
        // 0 host: 0 live agent
        Err(_) if !alive(id) => return Some(State::default()),
        Err(_) => return None,
    };

    let names = |key: &str| {
        reply[key]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(String::from)
            .collect()
    };

    Some(State {
        live: names("live"),
        busy: names("busy"),
        usage: serde_json::from_value(reply["usage"].clone()).unwrap_or_default(),
    })
}

#[cfg(test)]
#[path = "client_tests.rs"]
mod tests;
