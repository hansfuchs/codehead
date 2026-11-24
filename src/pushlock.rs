//! 1 push per checkout at a time: a lock file in the checkout's git dir
//! (`<git-dir>/codehead-push.lock`, a worktree has its own) while a push runs. Every push of a
//! thread takes it: the changes panel's steps, shell mode, the agents. The agents and shell
//! mode run `git` through codehead's shim (`<data dir>/shim/git` → the codehead binary, first on
//! their PATH): a push waits for the lock, any other git command runs as is. A held lock: the
//! push stops with a message; the changes panel shows its commits as PUSHING.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The lock's file name in the git dir.
const FILE: &str = "codehead-push.lock";

/// Who holds the lock: process, its name (an agent, `you`, `shell`), since when (ms).
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Holder {
    pub pid: u32,
    pub by: String,
    pub at: u64,
}

/// The lock while it lives: dropped, the file goes. None: 0 file (a git dir 0 one can write
/// in, e.g. an agent's sandbox): the push runs unguarded rather than never.
#[derive(Debug)]
pub struct Guard(Option<PathBuf>);

impl Drop for Guard {
    fn drop(&mut self) {
        if let Some(p) = &self.0 {
            let _ = std::fs::remove_file(p);
        }
    }
}

/// The git dir of the checkout DIR (a worktree: its own), GLOBALS the git options before the
/// subcommand (`-C x`). None: 0 git repo.
pub fn git_dir(dir: &Path, globals: &[OsString]) -> Option<PathBuf> {
    let out = Command::new(real_git()?).current_dir(dir).args(globals).args(["rev-parse", "--absolute-git-dir"]).stderr(std::process::Stdio::null()).output().ok()?;
    let s = String::from_utf8(out.stdout).ok()?;
    let s = s.trim();
    (out.status.success() && !s.is_empty()).then(|| PathBuf::from(s))
}

/// PID runs.
fn alive(pid: u32) -> bool {
    // signal 0: 0 signal sent, only the check (EPERM: it runs, as another user)
    let r = unsafe { libc::kill(pid as libc::pid_t, 0) };
    r == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

/// The holder of the lock in GIT_DIR. A lock of a process that ended: gone, None.
pub fn held_in(git_dir: &Path) -> Option<Holder> {
    let p = git_dir.join(FILE);
    let text = std::fs::read_to_string(&p).ok()?;
    match serde_json::from_str::<Holder>(&text) {
        Ok(h) if alive(h.pid) => Some(h),
        // a crashed holder, or a lock being written (0 text yet): only a dead one goes
        Ok(_) => {
            let _ = std::fs::remove_file(&p);
            None
        }
        Err(_) if text.is_empty() => Some(Holder { pid: 0, by: "?".into(), at: 0 }),
        Err(_) => {
            let _ = std::fs::remove_file(&p);
            None
        }
    }
}

/// The lock in GIT_DIR for BY (process PID). Err: its holder.
pub fn acquire_in(git_dir: &Path, by: &str, pid: u32) -> Result<Guard, Holder> {
    use std::io::Write;
    let p = git_dir.join(FILE);
    for _ in 0..2 {
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&p) {
            Ok(mut f) => {
                let h = Holder { pid, by: by.to_string(), at: crate::engine::now_ms() };
                let _ = f.write_all(serde_json::to_string(&h).unwrap_or_default().as_bytes());
                return Ok(Guard(Some(p)));
            }
            // held: a live holder stops the push; a dead one's file is gone, 1 more try
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                if let Some(h) = held_in(git_dir) {
                    return Err(h);
                }
            }
            Err(_) => return Ok(Guard(None)),
        }
    }
    Err(Holder { pid: 0, by: "?".into(), at: 0 })
}

/// The toast / message when HOLDER holds the lock.
pub fn busy_text(h: &Holder) -> String {
    format!("a push of this thread runs ({}): wait for it to end", h.by)
}

/// The shim's dir: put first on PATH, its `git` is codehead.
pub fn shim_dir() -> PathBuf {
    crate::engine::data_dir().join("shim")
}

/// The shim in place: `<shim dir>/git` → this binary (a new binary after an update: the link
/// follows).
pub fn install() {
    let Ok(exe) = std::env::current_exe() else { return };
    let d = shim_dir();
    let link = d.join("git");
    if std::fs::read_link(&link).is_ok_and(|t| t == exe) {
        return;
    }
    let _ = std::fs::create_dir_all(&d);
    let tmp = d.join(format!(".git-{}", std::process::id()));
    let _ = std::fs::remove_file(&tmp);
    if std::os::unix::fs::symlink(&exe, &tmp).is_ok() {
        let _ = std::fs::rename(&tmp, &link);
    }
}

/// PATH with the shim's dir first (once).
pub fn path_with_shim() -> OsString {
    let d = shim_dir();
    let rest: Vec<PathBuf> = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).filter(|x| *x != d).collect()).unwrap_or_default();
    std::env::join_paths(std::iter::once(d).chain(rest)).unwrap_or_default()
}

/// The real git: the 1st `git` on PATH that is no codehead shim.
pub fn real_git() -> Option<PathBuf> {
    let me = std::env::current_exe().ok().and_then(|p| p.canonicalize().ok());
    let shim = shim_dir();
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).filter(|d| *d != shim).map(|d| d.join("git")).find(|g| {
        let c = g.canonicalize().ok();
        c.is_some() && c != me && std::fs::metadata(g).is_ok_and(|m| {
            use std::os::unix::fs::PermissionsExt;
            m.is_file() && m.permissions().mode() & 0o111 != 0
        })
    })
}

/// The git options before the subcommand, and the subcommand (None: 0 subcommand).
pub fn split_args(args: &[OsString]) -> (Vec<OsString>, Option<String>) {
    let mut globals = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let s = a.to_string_lossy();
        if !s.starts_with('-') {
            return (globals, Some(s.into_owned()));
        }
        globals.push(a.clone());
        // an option with its value in the next arg
        if matches!(s.as_ref(), "-C" | "-c" | "--git-dir" | "--work-tree" | "--namespace" | "--config-env" | "--exec-path")
            && let Some(v) = it.next()
        {
            globals.push(v.clone());
        }
    }
    (globals, None)
}

/// The binary runs as `git` (the shim): a push takes the lock first, any other command runs
/// as is. Never returns.
pub fn shim_main() -> ! {
    use std::os::unix::process::CommandExt;
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let Some(git) = real_git() else {
        eprintln!("codehead git shim: 0 git on PATH");
        std::process::exit(127);
    };
    let (globals, sub) = split_args(&args);
    if sub.as_deref() != Some("push") {
        let e = Command::new(&git).args(&args).exec();
        eprintln!("codehead git shim: {e}");
        std::process::exit(126);
    }
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let by = std::env::var("CODEHEAD_AGENT").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| "shell".into());
    let guard = match git_dir(&cwd, &globals) {
        Some(d) => match acquire_in(&d, &by, std::process::id()) {
            Ok(g) => Some(g),
            Err(h) => {
                eprintln!("codehead: {}. The push did not run.", busy_text(&h));
                std::process::exit(1);
            }
        },
        // 0 repo here: git says so itself
        None => None,
    };
    let code = Command::new(&git).args(&args).status().map(|s| s.code().unwrap_or(1)).unwrap_or(127);
    drop(guard);
    std::process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_subcommand_comes_after_the_git_options() {
        let a = |v: &[&str]| v.iter().map(OsString::from).collect::<Vec<_>>();
        assert_eq!(split_args(&a(&["push", "-u", "origin", "HEAD"])).1.as_deref(), Some("push"));
        assert_eq!(split_args(&a(&["-C", "/x", "-c", "a=b", "push"])), (a(&["-C", "/x", "-c", "a=b"]), Some("push".into())));
        assert_eq!(split_args(&a(&["--no-pager", "log"])).1.as_deref(), Some("log"));
        assert_eq!(split_args(&a(&["-C", "push", "status"])).1.as_deref(), Some("status"));
        assert_eq!(split_args(&a(&["--version"])).1, None);
    }

    #[test]
    fn one_holder_at_a_time_a_dead_one_frees_the_lock() {
        let d = std::env::temp_dir().join(format!("codehead-pushlock-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();

        let g = acquire_in(&d, "GIGAS", std::process::id()).unwrap();
        assert_eq!(held_in(&d).map(|h| h.by).as_deref(), Some("GIGAS"));
        assert_eq!(acquire_in(&d, "you", std::process::id()).unwrap_err().by, "GIGAS");
        drop(g);
        assert!(held_in(&d).is_none());

        // a holder that ended (pid of a finished child): its lock goes at the next look
        let mut c = Command::new("true").spawn().unwrap();
        let pid = c.id();
        c.wait().unwrap();
        let _stale = acquire_in(&d, "ADA", pid).unwrap();
        std::mem::forget(_stale);
        assert!(held_in(&d).is_none());
        assert!(acquire_in(&d, "you", std::process::id()).is_ok());
        let _ = std::fs::remove_dir_all(&d);
    }
}
