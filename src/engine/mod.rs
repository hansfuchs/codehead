//! codehead's own agent engine: threads, agents, harness sessions, agent messages.
//!
//! - `store`: a thread = `thread.json` (what it is) + `events.jsonl` (what happened, append only).
//! - `roster`: agents, 1 TOML file per agent.
//! - `harness`, `harnesses`, `acp`: 1 harness process per agent and thread: Claude Code over
//!   stream-json, every other harness over the Agent Client Protocol.
//! - `host`: `codehead host <thread>`, the detached process that owns a thread's sessions.
//! - `client`: the UI side of the host socket.
//! - `mcp`: `codehead mcp`, the tools agents use to message each other.
//! - `servers`: the MCP servers codehead owns and connects to every harness.

pub mod acp;
pub mod client;
pub mod edits;
pub mod guard;
pub mod harness;
pub mod harnesses;
pub mod host;
pub mod limits;
pub mod mcp;
pub mod roster;
pub mod servers;
pub mod store;
pub mod subagents;

use std::path::PathBuf;

/// Tests that set $CODEHEAD_HOME, and tests that need the data dir to stay put while they run.
#[cfg(test)]
pub static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Data dir: $CODEHEAD_HOME, else $XDG_DATA_HOME/codehead, else ~/.local/share/codehead.
pub fn data_dir() -> PathBuf {
    if let Some(p) = std::env::var_os("CODEHEAD_HOME").filter(|p| !p.is_empty()) {
        return PathBuf::from(p);
    }
    // tests never write to the owner's data dir: 1 temp dir per test process
    if cfg!(test) {
        return std::env::temp_dir().join(format!("codehead-test-{}", std::process::id()));
    }
    std::env::var_os("XDG_DATA_HOME")
        .filter(|p| !p.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| crate::config::home().join(".local/share"))
        .join("codehead")
}

/// Config dir: $XDG_CONFIG_HOME/codehead, else ~/.config/codehead.
pub fn config_dir() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .filter(|p| !p.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| crate::config::home().join(".config"))
        .join("codehead")
}

/// Logs of hosts and the thread view.
pub fn logs_dir() -> PathBuf {
    data_dir().join("logs")
}

/// Host sockets: $XDG_RUNTIME_DIR/codehead, else /tmp/codehead-<uid>. Not under the data dir:
/// a Unix socket path holds max. 104-108 bytes.
pub fn run_dir() -> PathBuf {
    if let Some(p) = std::env::var_os("CODEHEAD_RUN").filter(|p| !p.is_empty()) {
        return PathBuf::from(p);
    }
    match std::env::var_os("XDG_RUNTIME_DIR").filter(|p| !p.is_empty()) {
        Some(p) => PathBuf::from(p).join("codehead"),
        // SAFETY: getuid has 0 failure modes.
        None => std::env::temp_dir().join(format!("codehead-{}", unsafe { libc::getuid() })),
    }
}

/// The codehead binary to start hosts and MCP servers with. A rebuild while codehead runs
/// deletes the running file (Linux: `current_exe` ends in " (deleted)"): then the binary at the
/// same path, else `codehead` on $PATH.
pub fn exe() -> PathBuf {
    let cur = std::env::current_exe().ok();
    if let Some(p) = &cur
        && p.exists()
    {
        return p.clone();
    }
    if let Some(p) = cur.as_ref().and_then(|p| p.to_str()).and_then(|s| s.strip_suffix(" (deleted)")).map(PathBuf::from)
        && p.exists()
    {
        return p;
    }
    if let Some(p) = std::env::var_os("PATH").into_iter().flat_map(|v| std::env::split_paths(&v).collect::<Vec<_>>()).map(|d| d.join("codehead")).find(|p| p.is_file()) {
        return p;
    }
    cur.unwrap_or_else(|| PathBuf::from("codehead"))
}

/// Milliseconds since the epoch.
pub fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// 16 hex chars: 8 from the clock (sorts by creation), 8 random.
pub fn new_id() -> String {
    let mut r = [0u8; 4];
    if let Ok(mut f) = std::fs::File::open("/dev/urandom") {
        use std::io::Read;
        let _ = f.read_exact(&mut r);
    }
    format!("{:08x}{}", (now_ms() / 1000) as u32, r.iter().map(|b| format!("{b:02x}")).collect::<String>())
}

/// Write PATH atomically: a temp file next to it, then rename.
/// F while this process holds an exclusive `flock` on PATH (made if missing): a read-modify-write
/// of a file that hosts, views and the CLI share.
pub fn locked<T>(path: &std::path::Path, f: impl FnOnce() -> T) -> std::io::Result<T> {
    let _lock = lock(path, true)?;
    Ok(f())
}

/// An exclusive `flock` on PATH (made if missing), held until the file drops. WAIT false: Err when
/// another process holds it.
pub fn lock(path: &std::path::Path, wait: bool) -> std::io::Result<std::fs::File> {
    use std::os::fd::AsRawFd;
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d)?;
    }
    let file = std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(path)?;
    let op = if wait { libc::LOCK_EX } else { libc::LOCK_EX | libc::LOCK_NB };
    // SAFETY: flock(2) on an fd FILE owns; the lock ends when FILE closes.
    if unsafe { libc::flock(file.as_raw_fd(), op) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(file)
}

pub fn write_atomic(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d)?;
    }
    let tmp = path.with_extension(format!("tmp-{}", std::process::id()));
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)
}

/// $CODEHEAD_TRACE=1: `<ms since start> <label>` lines in logs/trace.log (boot and redraw timing).
pub fn trace(label: &str) {
    use std::sync::OnceLock;
    static START: OnceLock<std::time::Instant> = OnceLock::new();
    static ON: OnceLock<bool> = OnceLock::new();
    let start = START.get_or_init(std::time::Instant::now);
    if !*ON.get_or_init(|| std::env::var_os("CODEHEAD_TRACE").is_some_and(|v| !v.is_empty())) {
        return;
    }
    use std::io::Write;
    let _ = std::fs::create_dir_all(logs_dir());
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(logs_dir().join("trace.log")) {
        let _ = writeln!(f, "{:>6} {label}", start.elapsed().as_millis());
    }
}
