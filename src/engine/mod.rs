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

pub(crate) mod acp;
pub(crate) mod checkouts;
pub(crate) mod client;
pub(crate) mod codex;
pub(crate) mod edits;
pub(crate) mod flows;
pub(crate) mod guard;
pub(crate) mod harness;
pub(crate) mod harnesses;
pub(crate) mod host;
pub(crate) mod limits;
pub(crate) mod mcp;
pub(crate) mod roster;
pub(crate) mod servers;
pub(crate) mod store;
pub(crate) mod subagents;
pub(crate) mod warm;

use std::path::PathBuf;

/// Tests that need the data dir to stay put while they run. 0 test sets $`CODEHEAD_HOME`: a set
/// var moves the data dir of every test of the process (1 process, tests in threads).
#[cfg(test)]
pub(crate) static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Data dir: $`CODEHEAD_HOME`, else $`XDG_DATA_HOME/codehead`, else ~/.local/share/codehead.
pub(crate) fn data_dir() -> PathBuf {
    if let Some(path_value) = std::env::var_os("CODEHEAD_HOME").filter(|path| !path.is_empty()) {
        return PathBuf::from(path_value);
    }

    // tests never write to the owner's data dir: 1 temp dir per test process
    if cfg!(test) {
        return std::env::temp_dir().join(format!("codehead-test-{}", std::process::id()));
    }

    std::env::var_os("XDG_DATA_HOME")
        .filter(|path_2| !path_2.is_empty())
        .map_or_else(|| crate::infra::config::home().join(".local/share"), PathBuf::from)
        .join("codehead")
}

/// Config dir: $`XDG_CONFIG_HOME/codehead`, else ~/.config/codehead.
pub(crate) fn config_dir() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .filter(|path| !path.is_empty())
        .map_or_else(|| crate::infra::config::home().join(".config"), PathBuf::from)
        .join("codehead")
}

/// Agents, flows, steps, mcp.toml, shortcuts.toml: the home repo's codehead dir (synced to
/// every device), else the config dir.
pub(crate) fn shared_dir() -> PathBuf {
    crate::infra::config::layout().unwrap_or_else(config_dir)
}

/// Logs of hosts and the thread view.
pub(crate) fn logs_dir() -> PathBuf {
    data_dir().join("logs")
}

/// Host sockets: $`XDG_RUNTIME_DIR/codehead`, else /tmp/codehead-<uid>. Not under the data dir:
/// a Unix socket path holds max. 104-108 bytes.
pub(crate) fn run_dir() -> PathBuf {
    if let Some(path_value) = std::env::var_os("CODEHEAD_RUN").filter(|path| !path.is_empty()) {
        return PathBuf::from(path_value);
    }

    // tests never bind sockets in the owner's run dir (short: a socket path holds max. 104 bytes)
    if cfg!(test) {
        return PathBuf::from(format!("/tmp/codehead-test-{}", std::process::id()));
    }

    match std::env::var_os("XDG_RUNTIME_DIR").filter(|path_2| !path_2.is_empty()) {
        Some(path_3) => PathBuf::from(path_3).join("codehead"),
        // SAFETY: getuid has 0 failure modes.
        None => std::env::temp_dir().join(format!("codehead-{}", unsafe { libc::getuid() })),
    }
}

/// The codehead binary to start hosts and MCP servers with. A rebuild while codehead runs
/// deletes the running file (Linux: `current_exe` ends in " (deleted)"): then the binary at the
/// same path, else `codehead` on $PATH.
pub(crate) fn exe() -> PathBuf {
    let cur = std::env::current_exe().ok();

    if let Some(path) = &cur
        && path.exists()
    {
        return path.clone();
    }

    if let Some(path_2) = cur
        .as_ref()
        .and_then(|path_value| path_value.to_str())
        .and_then(|text| text.strip_suffix(" (deleted)"))
        .map(PathBuf::from)
        && path_2.exists()
    {
        return path_2;
    }

    if let Some(path_4) = std::env::var_os("PATH")
        .into_iter()
        .flat_map(|value| std::env::split_paths(&value).collect::<Vec<_>>())
        .map(|directory| directory.join("codehead"))
        .find(|path_3| path_3.is_file())
    {
        return path_4;
    }

    cur.unwrap_or_else(|| PathBuf::from("codehead"))
}

/// Milliseconds since the epoch.
pub(crate) fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |directory| directory.as_millis() as u64)
}

/// 16 hex chars: 8 from the clock (sorts by creation), 8 random.
pub(crate) fn new_id() -> String {
    let mut result = [0u8; 4];

    if let Ok(mut file) = std::fs::File::open("/dev/urandom") {
        use std::io::Read;
        let _ = file.read_exact(&mut result);
    }

    format!(
        "{:08x}{:02x}{:02x}{:02x}{:02x}",
        (now_ms() / 1000) as u32,
        result[0],
        result[1],
        result[2],
        result[3]
    )
}

/// Write PATH atomically: a temp file next to it, then rename.
/// F while this process holds an exclusive `flock` on PATH (made if missing): a read-modify-write
/// of a file that hosts, views and the CLI share.
pub(crate) fn locked<T>(path: &std::path::Path, file: impl FnOnce() -> T) -> std::io::Result<T> {
    let _lock = lock(path, true)?;

    Ok(file())
}

/// An exclusive `flock` on PATH (made if missing), held until the file drops. WAIT false: Err when
/// another process holds it.
pub(crate) fn lock(path: &std::path::Path, wait: bool) -> std::io::Result<std::fs::File> {
    use std::os::fd::AsRawFd;

    if let Some(directory) = path.parent() {
        std::fs::create_dir_all(directory)?;
    }

    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(path)?;

    let op = if wait {
        libc::LOCK_EX
    } else {
        libc::LOCK_EX | libc::LOCK_NB
    };

    // SAFETY: flock(2) on an fd FILE owns; the lock ends when FILE closes.
    if unsafe { libc::flock(file.as_raw_fd(), op) } != 0 {
        return Err(std::io::Error::last_os_error());
    }

    Ok(file)
}

pub(crate) fn write_atomic(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(directory) = path.parent() {
        std::fs::create_dir_all(directory)?;
    }

    let tmp = path.with_extension(format!("tmp-{}", std::process::id()));

    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)
}

/// The UI's budget per step (a key, a draw, a tick, a screen change): longer = a hang.
pub(crate) const BUDGET: std::time::Duration = std::time::Duration::from_millis(20);

pub(crate) fn trace_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("CODEHEAD_TRACE").is_some_and(|value| !value.is_empty()))
}

/// The 1st frame is on screen: longer than BUDGET after the start → `SLOW … boot`.
pub(crate) fn first_frame() {
    static DONE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

    if DONE.swap(true, std::sync::atomic::Ordering::Relaxed) {
        return;
    }

    let took = start_instant().elapsed();

    if took > BUDGET {
        trace(&format!(
            "SLOW {} cpu {} boot (1st frame)",
            took.as_millis(),
            thread_cpu().as_millis()
        ));
    }
}

static TRANSITION: std::sync::Mutex<Option<(std::time::Instant, &'static str)>> = std::sync::Mutex::new(None);

/// A screen change LABEL starts (e.g. the new-thread form's submit): the next screen's 1st frame
/// ends it ([`transition_end`]).
pub(crate) fn transition_begin(label: &'static str) {
    *TRANSITION.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some((std::time::Instant::now(), label));
}

/// A screen's 1st frame: the screen change under way, longer than BUDGET → `SLOW <ms> <label>`.
pub(crate) fn transition_end() {
    let Some((at, label)) = TRANSITION
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .take()
    else {
        return;
    };

    let took = at.elapsed();

    if took > BUDGET {
        trace(&format!("SLOW {} {label}", took.as_millis()));
    }
}

fn start_instant() -> std::time::Instant {
    static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    *START.get_or_init(std::time::Instant::now)
}

/// A UI step under watch: at its end, longer than BUDGET → `SLOW <ms> <label>` in the trace
/// ($`CODEHEAD_TRACE=1`). LABEL runs only then.
pub(crate) struct Watch<F: FnOnce() -> String> {
    at: std::time::Instant,
    cpu: std::time::Duration,
    label: Option<F>,
}

pub(crate) fn watch<F: FnOnce() -> String>(label: F) -> Watch<F> {
    let on = trace_on();

    Watch {
        at: std::time::Instant::now(),
        cpu: if on { thread_cpu() } else { std::time::Duration::ZERO },
        label: on.then_some(label),
    }
}

/// CPU time of the calling thread: a step's own work (its wall time also holds waits and a
/// busy machine).
pub(crate) fn thread_cpu() -> std::time::Duration {
    let mut thread = libc::timespec { tv_sec: 0, tv_nsec: 0 };

    // SAFETY: T is a valid timespec; the clock id is a constant.
    unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &raw mut thread) };
    std::time::Duration::new(thread.tv_sec as u64, thread.tv_nsec as u32)
}

/// $`CODEHEAD_TRACE=all`: every watched step in the trace, not only the slow ones.
fn trace_all() -> bool {
    static ALL: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ALL.get_or_init(|| std::env::var("CODEHEAD_TRACE").is_ok_and(|value| value == "all"))
}

impl<F: FnOnce() -> String> Drop for Watch<F> {
    fn drop(&mut self) {
        let took = self.at.elapsed();

        trace_elapsed(took, self);
    }
}

/// $`CODEHEAD_TRACE=1`: `<ms since start> <label>` lines in logs/trace.log (boot and redraw timing).
pub(crate) fn trace(label: &str) {
    use std::io::Write;

    let start = start_instant();

    if !trace_on() {
        return;
    }

    let _ = std::fs::create_dir_all(logs_dir());

    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(logs_dir().join("trace.log"))
    {
        let _ = writeln!(
            file,
            "{:>6} {:>4} {label}",
            start.elapsed().as_millis(),
            thread_cpu().as_millis()
        );
    }
}

fn trace_elapsed<F: FnOnce() -> String>(took: std::time::Duration, model_state: &mut Watch<F>) {
    if (took > BUDGET || trace_all())
        && let Some(line) = model_state.label.take()
    {
        let tag = if took > BUDGET { "SLOW" } else { "ok" };

        trace(&format!(
            "{tag} {} cpu {} {}",
            took.as_millis(),
            (thread_cpu().saturating_sub(model_state.cpu)).as_millis(),
            line()
        ));
    }
}
