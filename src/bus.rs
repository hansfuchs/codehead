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
pub enum Part {
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
pub enum Git {
    /// Local branch NAME (`refs/heads/NAME`): a commit, a reset.
    Branch(String),
    /// Remote branch NAME (`refs/remotes/<remote>/NAME`): a push, a fetch.
    Remote(String),
    /// HEAD / index of the linked worktree NAME (`worktrees/NAME/`); "": the main checkout.
    Worktree(String),
    /// Anything else (packed-refs, FETCH_HEAD, info/exclude): every checkout of the repo.
    All,
}

/// What changed.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Topic {
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
pub fn subscribe() -> Receiver<Topic> {
    let (tx, rx) = std::sync::mpsc::channel();
    if let Ok(mut s) = SUBS.lock() {
        s.push(tx);
    }
    rx
}

/// Send T to every subscriber (gone ones drop out).
pub fn publish(t: Topic) {
    if let Ok(mut s) = SUBS.lock() {
        s.retain(|tx| tx.send(t.clone()).is_ok());
    }
}

/// New data for the screen: the version goes up, the UI loop wakes.
pub fn changed() {
    VERSION.fetch_add(1, Ordering::Relaxed);
    wake();
}

/// Goes up with each `changed`: the UI loop draws when it differs from the last drawn one.
pub fn version() -> u64 {
    VERSION.load(Ordering::Relaxed)
}

/// The send end of `channel`: each send wakes the UI loop (it sleeps on the tty and the pipe).
pub struct Tx<T>(Sender<T>);

impl<T> Clone for Tx<T> {
    fn clone(&self) -> Self {
        Tx(self.0.clone())
    }
}

impl<T> Tx<T> {
    pub fn send(&self, v: T) -> Result<(), std::sync::mpsc::SendError<T>> {
        let r = self.0.send(v);
        wake();
        r
    }
}

/// A channel from a background job to the UI: its result wakes the UI loop at once.
pub fn channel<T>() -> (Tx<T>, Receiver<T>) {
    let (tx, rx) = std::sync::mpsc::channel();
    (Tx(tx), rx)
}

/// (read end, write end) of the self-pipe, both non-blocking.
fn pipe() -> Option<(RawFd, RawFd)> {
    static P: OnceLock<Option<(RawFd, RawFd)>> = OnceLock::new();
    *P.get_or_init(|| {
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
pub fn wake() {
    if let Some((_, w)) = pipe() {
        // SAFETY: W is the open write end; 1 byte from a live buffer.
        let _ = unsafe { libc::write(w, b"x".as_ptr().cast(), 1) };
    }
}

/// The fd the UI loop polls beside the tty.
pub fn wake_fd() -> Option<RawFd> {
    pipe().map(|p| p.0)
}

/// Empty the pipe after a wake.
pub fn drain() {
    let Some((r, _)) = pipe() else { return };
    let mut buf = [0u8; 256];
    // SAFETY: R is the open, non-blocking read end; BUF is live.
    while unsafe { libc::read(r, buf.as_mut_ptr().cast(), buf.len()) } > 0 {}
}

/// Wait up to WAIT for input on FD or a wake. True: FD is readable. A signal (resize): false.
pub fn wait(fd: RawFd, wait: std::time::Duration) -> bool {
    let mut fds = vec![libc::pollfd { fd, events: libc::POLLIN, revents: 0 }];
    if let Some(w) = wake_fd() {
        fds.push(libc::pollfd { fd: w, events: libc::POLLIN, revents: 0 });
    }
    let ms = wait.as_millis().min(i32::MAX as u128) as i32;
    // SAFETY: FDS is a live array of FDS.len() pollfd.
    let n = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, ms) };
    if n <= 0 {
        return false;
    }
    if fds.get(1).is_some_and(|p| p.revents != 0) {
        drain();
    }
    fds[0].revents != 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    /// A publish reaches every subscriber; `changed` wakes a wait on a silent fd at once.
    #[test]
    fn publish_reaches_subscribers_and_changed_wakes_the_loop() {
        let (a, b) = (subscribe(), subscribe());
        publish(Topic::Git(PathBuf::from("/r/.git"), Git::All));
        assert_eq!(a.try_recv().unwrap(), Topic::Git(PathBuf::from("/r/.git"), Git::All));
        assert_eq!(b.try_recv().unwrap(), Topic::Git(PathBuf::from("/r/.git"), Git::All));

        // a pipe nobody writes: only the wake ends the wait
        let mut fds = [0 as RawFd; 2];
        assert_eq!(unsafe { libc::pipe(fds.as_mut_ptr()) }, 0);
        drain();
        let v = version();
        let t = std::thread::spawn(|| {
            std::thread::sleep(Duration::from_millis(30));
            changed();
        });
        let at = Instant::now();
        assert!(!wait(fds[0], Duration::from_secs(5)), "0 input: false");
        assert!(at.elapsed() < Duration::from_secs(2), "woken, not timed out");
        t.join().unwrap();
        assert!(version() > v);
        unsafe {
            libc::close(fds[0]);
            libc::close(fds[1]);
        }
    }
}
