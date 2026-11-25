//! A new worktree that works like the main checkout: gitignored local files copied in
//! (`.worktreeinclude`), a setup script run before the 1st agent turn, a block of 10 ports per
//! thread, a teardown script before the worktree goes.
//!
//! - `.worktreeinclude` (repo root, 1 path per line, `#` comments): files / dirs copied from the
//!   main checkout (APFS clone on macOS, reflink where the file system has it). 0 file: every
//!   gitignored `.env*` file of the root.
//! - setup: `.codehead/setup`, else `.agents/setup` (Amp), else `conductor.json` `scripts.setup`.
//! - teardown: `.codehead/teardown`, else `conductor.json` `scripts.archive`.
//! - env of both: `CODEHEAD_ROOT`, `CODEHEAD_WORKTREE`, `CODEHEAD_THREAD`, `CODEHEAD_BRANCH`,
//!   `CODEHEAD_PORT` (+ `CONDUCTOR_ROOT_PATH`, `CONDUCTOR_PORT` for conductor.json scripts).

use std::fmt::Write as FormatWrite;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// 1st port of the 1st block; each thread gets PORTS ports.
pub(crate) const PORT_BASE: u16 = 40000;
pub(crate) const PORTS: u16 = 10;
/// A setup script runs this long at most.
const SETUP_MAX: Duration = Duration::from_mins(20);
/// Output of a script kept in the thread log.
const OUTPUT_MAX: usize = 4096;

fn git(dir: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;

    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Paths `.worktreeinclude` of REPO lists; 0 file: the gitignored `.env*` files of the root.
pub(crate) fn includes(repo: &Path) -> Vec<String> {
    if let Ok(status) = std::fs::read_to_string(repo.join(".worktreeinclude")) {
        return status
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .map(|line| line.trim_start_matches("./").trim_end_matches('/').to_string())
            .collect();
    }

    let Ok(rd) = std::fs::read_dir(repo) else {
        return Vec::new();
    };

    let mut names: Vec<String> = rd
        .flatten()
        .filter(|entry| entry.path().is_file())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with(".env"))
        .filter(|name| {
            Command::new("git")
                .arg("-C")
                .arg(repo)
                .args(["check-ignore", "-q", name])
                .stdin(Stdio::null())
                .status()
                .is_ok_and(|status| status.success())
        })
        .collect();

    names.sort();
    names
}

/// Copy PATH (relative) of REPO into WT: a clone where the file system can (APFS: `cp -c`,
/// Linux: `--reflink=auto`), else a plain copy. A path already in WT (tracked) stays.
fn copy_one(repo: &Path, wt: &Path, path: &str) -> Result<bool, String> {
    let (src, dst) = (repo.join(path), wt.join(path));

    if !src.exists() || dst.exists() || path.contains("..") {
        return Ok(false);
    }

    if let Some(dir) = dst.parent() {
        std::fs::create_dir_all(dir).map_err(|error| format!("{}: {error}", dir.display()))?;
    }

    let clone = if cfg!(target_os = "macos") {
        "-c"
    } else {
        "--reflink=auto"
    };

    let ok = |args: &[&str]| {
        Command::new("cp")
            .args(args)
            .arg(&src)
            .arg(&dst)
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    };

    if ok(&[clone, "-R"]) || ok(&["-R"]) {
        Ok(true)
    } else {
        Err(format!("cp {path} failed"))
    }
}

/// Copy the include paths of REPO into the new worktree WT. Returns the copied paths.
pub(crate) fn copy_includes(repo: &Path, wt: &Path) -> Vec<String> {
    includes(repo)
        .into_iter()
        .filter(|path| copy_one(repo, wt, path).unwrap_or(false))
        .collect()
}

/// The command of script KIND (`setup` / `teardown`) in WT: (argv, label, conductor-style).
pub(crate) fn script(wt: &Path, kind: &str) -> Option<(Vec<String>, String, bool)> {
    let own = wt.join(".codehead").join(kind);

    if own.is_file() {
        return Some((
            vec!["sh".into(), own.display().to_string()],
            format!(".codehead/{kind}"),
            false,
        ));
    }

    if kind == "setup" && wt.join(".agents/setup").is_file() {
        return Some((
            vec!["sh".into(), wt.join(".agents/setup").display().to_string()],
            ".agents/setup".into(),
            false,
        ));
    }

    let key = if kind == "setup" { "setup" } else { "archive" };
    let conductor: serde_json::Value = std::fs::read(wt.join("conductor.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())?;
    let cmd = conductor["scripts"][key]
        .as_str()
        .filter(|text| !text.trim().is_empty())?;

    Some((
        vec!["sh".into(), "-c".into(), cmd.to_string()],
        format!("conductor.json scripts.{key}"),
        true,
    ))
}

/// The env of a script for thread ID: main checkout ROOT, worktree WT, BRANCH, PORT.
pub(crate) fn env(root: &Path, wt: &Path, id: &str, branch: &str, port: Option<u16>) -> Vec<(String, String)> {
    let mut env = vec![
        ("CODEHEAD_ROOT".to_string(), root.display().to_string()),
        ("CODEHEAD_WORKTREE".to_string(), wt.display().to_string()),
        ("CODEHEAD_THREAD".to_string(), id.to_string()),
        ("CODEHEAD_BRANCH".to_string(), branch.to_string()),
        ("CONDUCTOR_ROOT_PATH".to_string(), root.display().to_string()),
    ];

    if let Some(port) = port {
        env.push(("CODEHEAD_PORT".into(), port.to_string()));
        env.push(("CONDUCTOR_PORT".into(), port.to_string()));
    }

    // portless: the worktree's label (services the script starts get its URLs)
    if !branch.is_empty() && crate::infra::portless::active() {
        env.push(("PORTLESS_PREFIX".into(), crate::infra::portless::prefix(branch)));
    }

    env
}

/// Run ARGV in WT with ENV (stdout + stderr together), at most MAX. (ok, last `OUTPUT_MAX` bytes).
pub(crate) fn run(argv: &[String], wt: &Path, env: &[(String, String)], max: Duration) -> (bool, String) {
    use std::io::Read;
    use std::os::unix::process::CommandExt;

    // 1 log per run: 2 runs in the same ms (tests in parallel) never share 1 file
    static RUNS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let run = RUNS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let log = std::env::temp_dir().join(format!(
        "codehead-setup-{}-{}-{run}.log",
        std::process::id(),
        crate::engine::now_ms()
    ));

    let Ok(file) = std::fs::File::create(&log) else {
        return (false, "0 log file".into());
    };

    let Ok(err) = file.try_clone() else {
        return (false, "0 log file".into());
    };

    let child = Command::new(&argv[0])
        .args(&argv[1..])
        .current_dir(wt)
        .envs(env.iter().cloned())
        .stdin(Stdio::null())
        .stdout(file)
        .stderr(err)
        .process_group(0)
        .spawn();

    let mut child = match child {
        Ok(child) => child,
        Err(error) => return (false, format!("{}: {error}", argv.join(" "))),
    };

    let start = Instant::now();

    let ok = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status.success(),
            Ok(None) if start.elapsed() > max => {
                // the whole group: the script's children too
                unsafe { libc::kill(-(child.id() as i32), libc::SIGKILL) };
                let _ = child.wait();

                break false;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(200)),
            Err(_) => break false,
        }
    };

    let mut out = String::new();

    if let Ok(mut file) = std::fs::File::open(&log) {
        let _ = file.read_to_string(&mut out);
    }

    let _ = std::fs::remove_file(&log);

    if start.elapsed() > max {
        let _ = write!(out, "\n… stopped after {} min", max.as_secs() / 60);
    }

    let cut = out.len().saturating_sub(OUTPUT_MAX);
    let cut = (cut..out.len()).find(|&i| out.is_char_boundary(i)).unwrap_or(out.len());

    (ok, out[cut..].to_string())
}

/// The setup of thread T (blocking): (label, ok, output). None: 0 setup script.
pub(crate) fn run_setup(thread: &crate::engine::store::Thread) -> Option<(String, bool, String)> {
    let wt = PathBuf::from(thread.worktree.as_deref()?);
    let (argv, label, _) = script(&wt, "setup")?;
    let env = env(
        Path::new(&thread.dir),
        &wt,
        &thread.id,
        thread.branch.as_deref().unwrap_or_default(),
        thread.port,
    );
    let (ok, out) = run(&argv, &wt, &env, SETUP_MAX);

    Some((label, ok, out))
}

/// The teardown of thread T before its worktree goes (blocking, 2 min max). None: 0 script.
pub(crate) fn run_teardown(thread: &crate::engine::store::Thread) -> Option<(String, bool, String)> {
    let wt = PathBuf::from(thread.worktree.as_deref()?);
    let (argv, label, _) = script(&wt, "teardown")?;
    let env = env(
        Path::new(&thread.dir),
        &wt,
        &thread.id,
        thread.branch.as_deref().unwrap_or_default(),
        thread.port,
    );
    let (ok, out) = run(&argv, &wt, &env, Duration::from_secs(120));

    Some((label, ok, out))
}

/// The lowest port block no other thread holds.
pub(crate) fn free_port(threads: &[crate::engine::store::Thread]) -> u16 {
    let used: Vec<u16> = threads
        .iter()
        .filter(|thread| thread.state != "archived")
        .filter_map(|thread| thread.port)
        .collect();

    (0..2000u16)
        .map(|block| PORT_BASE + block * PORTS)
        .find(|port| !used.contains(port))
        .unwrap_or(PORT_BASE)
}

/// The base a new worktree branch starts from: `origin/<default>` after a fetch, else HEAD.
pub(crate) fn new_branch_base(repo: &Path) -> String {
    if git(repo, &["remote"]).is_some_and(|remotes| remotes.lines().any(|line| line == "origin")) {
        let _ = Command::new("git")
            .arg("-C")
            .arg(repo)
            .args(["fetch", "-q", "origin"])
            .env("GIT_TERMINAL_PROMPT", "0")
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }

    let def = git(repo, &["symbolic-ref", "--short", "refs/remotes/origin/HEAD"])
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty())
        .or_else(|| {
            ["origin/main", "origin/master"]
                .into_iter()
                .find(|rev| git(repo, &["rev-parse", "--verify", "--quiet", rev]).is_some())
                .map(String::from)
        });

    def.unwrap_or_else(|| "HEAD".into())
}

#[cfg(test)]
#[path = "worktree_setup_tests.rs"]
mod tests;
