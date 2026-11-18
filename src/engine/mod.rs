//! codehead's own agent engine: threads, agents, harness sessions, agent messages.
//!
//! - `store`: a thread = `thread.json` (what it is) + `events.jsonl` (what happened, append only).
//! - `roster`: agents, 1 TOML file per agent.
//! - `harness`, `harnesses`, `acp`: 1 harness process per agent and thread: Claude Code over
//!   stream-json, every other harness over the Agent Client Protocol.
//! - `host`: `codehead host <thread>`, the detached process that owns a thread's sessions.
//! - `client`: the UI side of the host socket.
//! - `mcp`: `codehead mcp`, the tools agents use to message each other.

pub mod acp;
pub mod client;
pub mod guard;
pub mod harness;
pub mod harnesses;
pub mod host;
pub mod mcp;
pub mod roster;
pub mod store;

use std::path::PathBuf;

/// Data dir: $CODEHEAD_HOME, else $XDG_DATA_HOME/codehead, else ~/.local/share/codehead.
pub fn data_dir() -> PathBuf {
    if let Some(p) = std::env::var_os("CODEHEAD_HOME").filter(|p| !p.is_empty()) {
        return PathBuf::from(p);
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
