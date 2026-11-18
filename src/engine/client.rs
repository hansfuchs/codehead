//! The UI side of a thread host: start it when it is down, send it commands.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::Value;

use super::host::socket;

/// A host serves thread ID.
pub fn alive(id: &str) -> bool {
    UnixStream::connect(socket(id)).is_ok()
}

/// Start the host of thread ID (detached: it outlives this terminal) unless one runs.
pub fn ensure(id: &str) -> Result<(), String> {
    if alive(id) {
        return Ok(());
    }
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let logs = super::logs_dir();
    std::fs::create_dir_all(&logs).map_err(|e| e.to_string())?;
    let log = logs.join(format!("host-{id}.log"));
    let file = std::fs::OpenOptions::new().create(true).append(true).open(&log).map_err(|e| format!("{}: {e}", log.display()))?;
    let mut c = Command::new(exe);
    c.args(["host", id]).stdin(Stdio::null()).stdout(file.try_clone().map_err(|e| e.to_string())?).stderr(file);
    // SAFETY: setsid is async-signal-safe; runs in the child before exec.
    unsafe {
        c.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    let mut child = c.spawn().map_err(|e| format!("codehead host: {e}"))?;
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
        std::thread::sleep(Duration::from_millis(30));
    }
    Err(format!("codehead host did not start. log: {}", log.display()))
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
    let Ok(v) = send_within(id, &serde_json::json!({"op": "ping"}), Duration::from_millis(500)) else { return Default::default() };
    let names = |k: &str| v[k].as_array().into_iter().flatten().filter_map(Value::as_str).map(String::from).collect();
    (names("live"), names("busy"))
}
