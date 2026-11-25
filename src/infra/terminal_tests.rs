use super::tty_gone;
use std::fs::{File, OpenOptions};
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;

/// A live pty: keys can come. Its other end closed (the window gone): gone, so no crossterm
/// read spins on it. A pipe: no tty.
#[test]
fn tty_gone_on_hang_up_and_on_no_tty() {
    let (master, slave_file) = pseudo_terminal();
    let slave = slave_file.as_raw_fd();

    assert_eq!(tty_gone(slave), None, "live pty");

    drop(master);

    assert_eq!(tty_gone(slave).as_deref(), Some("terminal closed"));

    let mut fds = [0; 2];

    // SAFETY: FDS has room for the 2 fds pipe writes.
    assert_eq!(unsafe { libc::pipe(fds.as_mut_ptr()) }, 0);
    assert_eq!(tty_gone(fds[0]).as_deref(), Some("no terminal"));
    assert_eq!(tty_gone(-1).as_deref(), Some("no terminal"));

    // SAFETY: each fd is open.
    unsafe {
        libc::close(fds[0]);
        libc::close(fds[1]);
    }
}

fn pseudo_terminal() -> (File, File) {
    let master = OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(libc::O_NOCTTY)
        .open("/dev/ptmx")
        .unwrap();

    let name = unsafe {
        assert_eq!(libc::grantpt(master.as_raw_fd()), 0);
        assert_eq!(libc::unlockpt(master.as_raw_fd()), 0);
        assert_ne!(libc::fcntl(master.as_raw_fd(), libc::F_GETFD) & libc::FD_CLOEXEC, 0);
        let name = libc::ptsname(master.as_raw_fd());

        assert!(!name.is_null());
        std::ffi::CStr::from_ptr(name).to_string_lossy().into_owned()
    };

    let slave = OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(libc::O_NOCTTY)
        .open(name)
        .unwrap();

    (master, slave)
}
