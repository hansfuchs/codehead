//! Event bus: the sources (the file watcher, own actions) publish what changed, the consumers
//! (landed, notify, views) read again only that. 0 periodic pass.
//!
//! The UI loop sleeps in `read_ready` on the tty and on 1 self-pipe: `changed` (new data for the
//! screen) writes 1 byte, the loop wakes in µs and draws.

use std::os::fd::RawFd;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Mutex, OnceLock};

/// The part of a thread's dir that changed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Part {
    /// thread.json: state, checkouts, agents.
    Meta,
    /// events.jsonl: the log.
    Log,
    /// comments.json.
    Comments,
    /// any other file (drafts, attachments, the host socket).
    Other,
}

/// The part of a git dir that changed: the readers read only the checkouts it touches.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Git {
    /// Local branch NAME (`refs/heads/NAME`): a commit, a reset.
    Branch(String),
    /// Remote branch NAME (`refs/remotes/<remote>/NAME`): a push, a fetch.
    Remote(String),
    /// HEAD / index of the linked worktree NAME (`worktrees/NAME/`); "": the main checkout.
    Worktree(String),
    /// Anything else (packed-refs, `FETCH_HEAD`, info/exclude): every checkout of the repo.
    All,
}

/// What changed.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Topic {
    /// A file of thread ID's dir.
    Thread(String, Part),
    /// Git metadata of a repo (its common git dir) changed: what of it.
    Git(PathBuf, Git),
    /// A file of a checkout's work tree (0 ignored file): the checkout's top dir.
    Work(PathBuf),
    /// The watcher lost events under this root (overflow, restart): read it all again.
    Rescan(PathBuf),
}

static SUBS: Mutex<Vec<Sender<Topic>>> = Mutex::new(Vec::new());
static VERSION: AtomicU64 = AtomicU64::new(0);

/// A receiver of every topic published from now on.
pub(crate) fn subscribe() -> Receiver<Topic> {
    let (tx, rx) = std::sync::mpsc::channel();

    if let Ok(mut text) = SUBS.lock() {
        text.push(tx);
    }

    rx
}

/// Send T to every subscriber (gone ones drop out).
pub(crate) fn publish(topic: &Topic) {
    if let Ok(mut subs) = SUBS.lock() {
        subs.retain(|tx| tx.send(topic.clone()).is_ok());
    }
}

/// New data for the screen: the version goes up, the UI loop wakes.
pub(crate) fn changed() {
    VERSION.fetch_add(1, Ordering::Relaxed);
    wake();
}

/// Goes up with each `changed`: the UI loop draws when it differs from the last drawn one.
pub(crate) fn version() -> u64 {
    VERSION.load(Ordering::Relaxed)
}

/// The send end of `channel`: each send wakes the UI loop (it sleeps on the tty and the pipe).
pub(crate) struct Tx<T>(Sender<T>);

impl<T> Clone for Tx<T> {
    fn clone(&self) -> Self {
        Tx(self.0.clone())
    }
}

impl<T> Tx<T> {
    pub(crate) fn send(&self, value: T) -> Result<(), std::sync::mpsc::SendError<T>> {
        let result = self.0.send(value);

        wake();
        result
    }
}

/// A channel from a background job to the UI: its result wakes the UI loop at once.
pub(crate) fn channel<T>() -> (Tx<T>, Receiver<T>) {
    let (tx, rx) = std::sync::mpsc::channel();

    (Tx(tx), rx)
}

/// (read end, write end) of the self-pipe, both non-blocking.
fn pipe() -> Option<(RawFd, RawFd)> {
    static PIPE: OnceLock<Option<(RawFd, RawFd)>> = OnceLock::new();
    *PIPE.get_or_init(|| {
        let mut fds = [0 as RawFd; 2];

        // SAFETY: FDS has room for the 2 fds pipe writes.
        if unsafe { libc::pipe(fds.as_mut_ptr()) } != 0 {
            return None;
        }

        for fd in fds {
            // SAFETY: fd is open (pipe above); flags change only.
            unsafe {
                let fl = libc::fcntl(fd, libc::F_GETFL);

                libc::fcntl(fd, libc::F_SETFL, fl | libc::O_NONBLOCK);
                libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC);
            }
        }

        Some((fds[0], fds[1]))
    })
}

/// Wake the UI loop (a full pipe: a wake is pending already).
pub(crate) fn wake() {
    if let Some((_, width)) = pipe() {
        // SAFETY: W is the open write end; 1 byte from a live buffer.
        let _ = unsafe { libc::write(width, b"x".as_ptr().cast(), 1) };
    }
}

/// The fd the UI loop polls beside the tty.
pub(crate) fn wake_fd() -> Option<RawFd> {
    pipe().map(|fds| fds.0)
}

/// Empty the pipe after a wake.
pub(crate) fn drain() {
    let Some((read_fd, _)) = pipe() else { return };
    let mut buf = [0u8; 256];

    // SAFETY: READ_FD is the open, non-blocking read end; BUF is live.
    while unsafe { libc::read(read_fd, buf.as_mut_ptr().cast(), buf.len()) } > 0 {}
}

/// Wait up to WAIT for input on FD or a wake. True: FD is readable. A signal (resize): false.
pub(crate) fn wait(fd: RawFd, wait: std::time::Duration) -> bool {
    let mut fds = vec![libc::pollfd {
        fd,
        events: libc::POLLIN,
        revents: 0,
    }];

    if let Some(wake) = wake_fd() {
        fds.push(libc::pollfd {
            fd: wake,
            events: libc::POLLIN,
            revents: 0,
        });
    }

    let ms = wait.as_millis().min(i32::MAX as u128) as i32;

    // SAFETY: FDS is a live array of FDS.len() pollfd.
    let ready = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, ms) };

    if ready <= 0 {
        return false;
    }

    if fds.get(1).is_some_and(|wake| wake.revents != 0) {
        drain();
    }

    fds[0].revents != 0
}

#[cfg(test)]
#[path = "bus_tests.rs"]
mod tests;
