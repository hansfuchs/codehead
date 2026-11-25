use super::*;
use std::time::{Duration, Instant};

/// A publish reaches every subscriber; `changed` wakes a wait on a silent fd at once.
#[test]
fn publish_reaches_subscribers_and_changed_wakes_the_loop() {
    let (first, second) = (subscribe(), subscribe());

    publish(&(Topic::Git(PathBuf::from("/r/.git"), Git::All)));

    assert_eq!(
        first.try_recv().unwrap(),
        Topic::Git(PathBuf::from("/r/.git"), Git::All)
    );
    assert_eq!(
        second.try_recv().unwrap(),
        Topic::Git(PathBuf::from("/r/.git"), Git::All)
    );

    // a pipe nobody writes: only the wake ends the wait
    let mut fds = [0 as RawFd; 2];

    assert_eq!(unsafe { libc::pipe(fds.as_mut_ptr()) }, 0);

    drain();
    let before = version();

    let waker = std::thread::spawn(|| {
        std::thread::sleep(Duration::from_millis(30));
        changed();
    });

    let at = Instant::now();

    assert!(!wait(fds[0], Duration::from_secs(5)), "0 input: false");
    assert!(at.elapsed() < Duration::from_secs(2), "woken, not timed out");

    waker.join().unwrap();

    assert!(version() > before);

    unsafe {
        libc::close(fds[0]);
        libc::close(fds[1]);
    }
}
