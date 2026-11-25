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
pub(crate) struct Holder {
    pub pid: u32,
    pub by: String,
    pub at: u64,
}

/// The lock while it lives: dropped, the file goes. None: 0 file (a git dir 0 one can write
/// in, e.g. an agent's sandbox): the push runs unguarded rather than never.
#[derive(Debug)]
pub(crate) struct Guard(Option<PathBuf>);

impl Drop for Guard {
    fn drop(&mut self) {
        if let Some(path) = &self.0 {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// The git dir of the checkout DIR (a worktree: its own), GLOBALS the git options before the
/// subcommand (`-C x`). None: 0 git repo.
pub(crate) fn git_dir(dir: &Path, globals: &[OsString]) -> Option<PathBuf> {
    let out = Command::new(real_git()?)
        .current_dir(dir)
        .args(globals)
        .args(["rev-parse", "--absolute-git-dir"])
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    let text = String::from_utf8(out.stdout).ok()?;
    let text_value = text.trim();

    (out.status.success() && !text_value.is_empty()).then(|| PathBuf::from(text_value))
}

/// PID runs.
fn alive(pid: u32) -> bool {
    // signal 0: 0 signal sent, only the check (EPERM: it runs, as another user)
    let result = unsafe { libc::kill(pid as libc::pid_t, 0) };

    result == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

/// The holder of the lock in `GIT_DIR`. A lock of a process that ended: gone, None.
pub(crate) fn held_in(git_dir: &Path) -> Option<Holder> {
    let path = git_dir.join(FILE);
    let text = std::fs::read_to_string(&path).ok()?;

    match serde_json::from_str::<Holder>(&text) {
        Ok(height) if alive(height.pid) => Some(height),
        // a crashed holder, or a lock being written (0 text yet): only a dead one goes
        Err(_) if text.is_empty() => Some(Holder {
            pid: 0,
            by: "?".into(),
            at: 0,
        }),
        Ok(_) | Err(_) => {
            let _ = std::fs::remove_file(&path);

            None
        }
    }
}

/// The lock in `GIT_DIR` for BY (process PID). Err: its holder.
pub(crate) fn acquire_in(git_dir: &Path, by: &str, pid: u32) -> Result<Guard, Holder> {
    use std::io::Write;
    let path = git_dir.join(FILE);

    for _ in 0..2 {
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut file) => {
                let height = Holder {
                    pid,
                    by: by.to_string(),
                    at: crate::engine::now_ms(),
                };
                let _ = file.write_all(serde_json::to_string(&height).unwrap_or_default().as_bytes());

                return Ok(Guard(Some(path)));
            }
            // held: a live holder stops the push; a dead one's file is gone, 1 more try
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                held_in(git_dir).map_or(Ok(()), Err)?;
            }
            Err(_) => return Ok(Guard(None)),
        }
    }

    Err(Holder {
        pid: 0,
        by: "?".into(),
        at: 0,
    })
}

/// The toast / message when HOLDER holds the lock.
pub(crate) fn busy_text(height: &Holder) -> String {
    format!("a push of this thread runs ({}): wait for it to end", height.by)
}

/// The shim's dir: put first on PATH, its `git` is codehead.
pub(crate) fn shim_dir() -> PathBuf {
    crate::engine::data_dir().join("shim")
}

/// The shim in place: `<shim dir>/git` → this binary (a new binary after an update: the link
/// follows).
pub(crate) fn install() {
    let Ok(exe) = std::env::current_exe() else { return };
    let directory = shim_dir();
    let link = directory.join("git");

    if std::fs::read_link(&link).is_ok_and(|thread| thread == exe) {
        return;
    }

    let _ = std::fs::create_dir_all(&directory);
    let tmp = directory.join(format!(".git-{}", std::process::id()));
    let _ = std::fs::remove_file(&tmp);

    if std::os::unix::fs::symlink(&exe, &tmp).is_ok() {
        let _ = std::fs::rename(&tmp, &link);
    }
}

/// PATH with the shim's dir first (once).
pub(crate) fn path_with_shim() -> OsString {
    let directory = shim_dir();
    let rest: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|path| std::env::split_paths(&path).filter(|x| *x != directory).collect())
        .unwrap_or_default();

    std::env::join_paths(std::iter::once(directory).chain(rest)).unwrap_or_default()
}

/// The real git: the 1st `git` on PATH that is no codehead shim.
pub(crate) fn real_git() -> Option<PathBuf> {
    let me = std::env::current_exe()
        .ok()
        .and_then(|path_value| path_value.canonicalize().ok());
    let shim = shim_dir();
    let path = std::env::var_os("PATH")?;

    std::env::split_paths(&path)
        .filter(|directory| *directory != shim)
        .map(|directory_value| directory_value.join("git"))
        .find(|group| {
            let character = group.canonicalize().ok();

            character.is_some()
                && character != me
                && std::fs::metadata(group).is_ok_and(|message| {
                    use std::os::unix::fs::PermissionsExt;
                    message.is_file() && message.permissions().mode() & 0o111 != 0
                })
        })
}

/// The git options before the subcommand, and the subcommand (None: 0 subcommand).
pub(crate) fn split_args(args: &[OsString]) -> (Vec<OsString>, Option<String>) {
    let mut globals = Vec::new();
    let mut it = args.iter();

    while let Some(agent) = it.next() {
        let text = agent.to_string_lossy();

        if !text.starts_with('-') {
            return (globals, Some(text.into_owned()));
        }

        globals.push(agent.clone());

        // an option with its value in the next arg
        if matches!(
            text.as_ref(),
            "-C" | "-c" | "--git-dir" | "--work-tree" | "--namespace" | "--config-env" | "--exec-path"
        ) && let Some(value) = it.next()
        {
            globals.push(value.clone());
        }
    }

    (globals, None)
}

/// The binary runs as `git` (the shim): a push takes the lock first, any other command runs
/// as is. Never returns.
pub(crate) fn shim_main() -> ! {
    use std::os::unix::process::CommandExt;
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();

    let Some(git) = real_git() else {
        eprintln!("codehead git shim: 0 git on PATH");
        std::process::exit(127);
    };

    let (globals, sub) = split_args(&args);

    if sub.as_deref() != Some("push") {
        let error = Command::new(&git).args(&args).exec();

        eprintln!("codehead git shim: {error}");
        std::process::exit(126);
    }

    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let by = std::env::var("CODEHEAD_AGENT")
        .ok()
        .filter(|text| !text.is_empty())
        .unwrap_or_else(|| "shell".into());

    let guard = match git_dir(&cwd, &globals) {
        Some(directory) => match acquire_in(&directory, &by, std::process::id()) {
            Ok(group) => Some(group),
            Err(height) => {
                eprintln!("codehead: {}. The push did not run.", busy_text(&height));
                std::process::exit(1);
            }
        },
        // 0 repo here: git says so itself
        None => None,
    };

    let code = Command::new(&git)
        .args(&args)
        .status()
        .map_or(127, |text_value| text_value.code().unwrap_or(1));

    drop(guard);
    std::process::exit(code);
}

#[cfg(test)]
#[path = "pushlock_tests.rs"]
mod tests;
