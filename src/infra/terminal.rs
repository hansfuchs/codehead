use std::io::{self, Write};
use std::time::{Duration, Instant};

use ratatui::crossterm::event::{self, DisableBracketedPaste, DisableMouseCapture, Event, PopKeyboardEnhancementFlags};
use ratatui::crossterm::terminal::{self, LeaveAlternateScreen};
use ratatui::crossterm::{cursor, execute};

pub(crate) fn fail(msg: &str, code: i32) -> ! {
    // not eprintln: on a closed terminal it panics, exit 101 instead of CODE
    let _ = writeln!(io::stderr(), "codehead: {msg}");

    std::process::exit(code)
}

pub(crate) fn restore_on_panic() {
    let hook = std::panic::take_hook();

    std::panic::set_hook(Box::new(move |info| {
        let mut out = io::stdout();
        let _ = execute!(
            out,
            PopKeyboardEnhancementFlags,
            DisableMouseCapture,
            DisableBracketedPaste,
            cursor::Show,
            LeaveAlternateScreen
        );
        let _ = terminal::disable_raw_mode();

        hook(info);
    }));
}

/// SIGTERM / SIGHUP came: the next `read_ready` fails, the screen loops end and restore the tty.
static SIGNALED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// The signal that came (its exit code: 128 + it).
static SIGNAL: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);
/// Write end of the exit guard's pipe; -1: no guard.
static GUARD_W: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(-1);

/// Time for the screen loop to end by itself after a signal, before the exit guard ends the process.
const EXIT_GRACE: Duration = Duration::from_secs(2);

extern "C" fn on_signal(signal: libc::c_int) {
    SIGNAL.store(signal, std::sync::atomic::Ordering::Relaxed);
    SIGNALED.store(true, std::sync::atomic::Ordering::Relaxed);
    let guard_fd = GUARD_W.load(std::sync::atomic::Ordering::Relaxed);

    if guard_fd >= 0 {
        // SAFETY: write is async-signal-safe; 1 byte from a static literal.
        let _ = unsafe { libc::write(guard_fd, b"x".as_ptr().cast(), 1) };
    }
}

/// SIGTERM / SIGHUP: the screen loops end and restore the tty. A closed terminal sends SIGHUP; its
/// default action ended the process, this handler does not. So an exit guard: a thread asleep on a
/// pipe the handler writes, which ends the process when the screen loop did not end within
/// `EXIT_GRACE`. crossterm 0.29 spins on a hung-up tty, where no flag check reaches it, and the UI
/// thread can enter it before the hang-up shows: a write to the closing tty wakes it before the
/// kernel marks the tty hung up and before the handler runs.
pub(crate) fn catch_signals() {
    let mut fds = [-1; 2];

    // SAFETY: FDS has room for the 2 fds pipe writes.
    if unsafe { libc::pipe(fds.as_mut_ptr()) } == 0 {
        start_exit_guard(fds);
    }

    for signal in [libc::SIGTERM, libc::SIGHUP] {
        // SAFETY: the handler stores to atomics and writes 1 byte (async-signal-safe).
        unsafe { libc::signal(signal, on_signal as *const () as libc::sighandler_t) };
    }
}

fn start_exit_guard(fds: [i32; 2]) {
    for fd in fds {
        // SAFETY: FD is open (pipe above); flags change only.
        unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) };
    }

    let read_fd = fds[0];

    let guard = std::thread::Builder::new().name("exit-guard".into()).spawn(move || {
        let mut byte = [0u8; 1];

        // SAFETY: READ_FD is the open read end; BYTE is live. Blocks until the handler writes.
        while unsafe { libc::read(read_fd, byte.as_mut_ptr().cast(), 1) } < 0
            && io::Error::last_os_error().kind() == io::ErrorKind::Interrupted
        {}

        std::thread::sleep(EXIT_GRACE);
        let signal = SIGNAL.load(std::sync::atomic::Ordering::Relaxed);
        let _ = writeln!(
            io::stderr(),
            "codehead: signal {signal}, the screen did not end within {}s: exit",
            EXIT_GRACE.as_secs()
        );

        std::process::exit(128 + signal)
    });

    if guard.is_ok() {
        GUARD_W.store(fds[1], std::sync::atomic::Ordering::Relaxed);
    }
}

/// The fd crossterm reads keys from: stdin when a tty, else /dev/tty (crossterm's own pick).
/// -1: no terminal.
fn input_fd() -> std::os::fd::RawFd {
    static FD: std::sync::OnceLock<std::os::fd::RawFd> = std::sync::OnceLock::new();
    *FD.get_or_init(|| {
        // SAFETY: isatty on fd 0 reads no memory.
        if unsafe { libc::isatty(libc::STDIN_FILENO) } == 1 {
            return libc::STDIN_FILENO;
        }

        // SAFETY: a NUL-terminated path literal; the fd stays open for the process.
        unsafe { libc::open(c"/dev/tty".as_ptr(), libc::O_RDONLY | libc::O_CLOEXEC) }
    })
}

/// Why FD can give no more keys: no tty (any more), or hung up (window closed, other pty end
/// gone). None: live. crossterm 0.29 reads a hung-up tty in an endless loop (EOF reads as "more
/// to come"): 100% CPU, a SIGTERM never reaches `read_ready`. So no crossterm call after a hang-up.
fn tty_gone(fd: std::os::fd::RawFd) -> Option<String> {
    if fd < 0 {
        return Some("no terminal".into());
    }

    // the hang-up first: Linux reports a hung-up pty as 0 tty (isatty fails), macOS does not
    let mut pollfd = libc::pollfd {
        fd,
        events: libc::POLLIN,
        revents: 0,
    };

    // SAFETY: POLLFD is 1 live pollfd; 0 ms: no wait.
    let ready = unsafe { libc::poll(&raw mut pollfd, 1, 0) };

    if ready > 0 && pollfd.revents & (libc::POLLHUP | libc::POLLERR | libc::POLLNVAL) != 0 {
        return Some("terminal closed".into());
    }

    // SAFETY: isatty reads no memory; a bad fd gives 0.
    if unsafe { libc::isatty(fd) } != 1 {
        return Some("no terminal".into());
    }

    None
}

/// Every event ready within WAIT (the 1st one) and right after it, each with its read time.
/// Read first, handled after: the handling time of 1 key never splits a burst of the OS.
/// Err: a signal came or the terminal is gone; every caller ends its screen loop.
pub(crate) fn read_ready(wait: Duration) -> Result<Vec<(Event, Instant)>, String> {
    if SIGNALED.load(std::sync::atomic::Ordering::Relaxed) {
        return Err("terminated by a signal".into());
    }

    let fd = input_fd();

    if let Some(why) = tty_gone(fd) {
        return Err(why);
    }

    let mut out = Vec::new();
    let mut wait = wait;

    // sleep on the tty and on the bus's wake pipe: new data wakes the loop at once. Events
    // crossterm buffered already: read first.
    if !wait.is_zero() && !event::poll(Duration::ZERO).map_err(|error| error.to_string())? {
        crate::infra::bus::wait(fd, wait);
        wait = Duration::ZERO;
    }

    while out.len() < 512 {
        // a hang-up or a signal while asleep or within a burst: before crossterm reads again. A
        // closed terminal's SIGHUP wakes the sleep before the tty reads as hung up.
        if SIGNALED.load(std::sync::atomic::Ordering::Relaxed) {
            return Err("terminated by a signal".into());
        }

        if let Some(why) = tty_gone(fd) {
            return Err(why);
        }

        if !event::poll(wait).map_err(|error| error.to_string())? {
            break;
        }

        let ev = event::read().map_err(|error| error.to_string())?;

        key_log(&ev);
        out.push((ev, Instant::now()));
        wait = Duration::ZERO;
    }

    Ok(out)
}

/// $`CODEHEAD_KEYLOG=1`: each key event as 1 line in ~/.cache/codehead/keys.log (what the terminal
/// sends for a key).
fn key_log(ev: &Event) {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();

    if !*ON.get_or_init(|| std::env::var_os("CODEHEAD_KEYLOG").is_some_and(|value| !value.is_empty() && value != "0")) {
        return;
    }

    let Event::Key(key) = ev else { return };
    let path = crate::infra::config::home().join(".cache/codehead/keys.log");

    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }

    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(
            file,
            "{} {:?} {:?} {:?}",
            crate::engine::now_ms(),
            key.code,
            key.modifiers,
            key.kind
        );
    }
}

#[cfg(test)]
#[path = "terminal_tests.rs"]
mod tests;
