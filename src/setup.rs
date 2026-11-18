//! A new worktree that works like the main checkout: gitignored local files copied in
//! (`.worktreeinclude`), a setup script run before the 1st agent turn, a block of 10 ports per
//! thread, a teardown script before the worktree goes.
//!
//! - `.worktreeinclude` (repo root, 1 path per line, `#` comments): files / dirs copied from the
//!   main checkout (APFS clone on macOS, reflink where the file system has it). 0 file: every
//!   gitignored `.env*` file of the root.
//! - setup: `.codehead/setup`, else `.agents/setup` (Amp), else `conductor.json` `scripts.setup`.
//! - teardown: `.codehead/teardown`, else `conductor.json` `scripts.archive`.
//! - env of both: CODEHEAD_ROOT, CODEHEAD_WORKTREE, CODEHEAD_THREAD, CODEHEAD_BRANCH,
//!   CODEHEAD_PORT (+ CONDUCTOR_ROOT_PATH, CONDUCTOR_PORT for conductor.json scripts).

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// 1st port of the 1st block; each thread gets PORTS ports.
pub const PORT_BASE: u16 = 40000;
pub const PORTS: u16 = 10;
/// A setup script runs this long at most.
const SETUP_MAX: Duration = Duration::from_secs(20 * 60);
/// Output of a script kept in the thread log.
const OUTPUT_MAX: usize = 4096;

fn git(dir: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git").arg("-C").arg(dir).args(args).stdin(Stdio::null()).stderr(Stdio::null()).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Paths `.worktreeinclude` of REPO lists; 0 file: the gitignored `.env*` files of the root.
pub fn includes(repo: &Path) -> Vec<String> {
    if let Ok(t) = std::fs::read_to_string(repo.join(".worktreeinclude")) {
        return t.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')).map(|l| l.trim_start_matches("./").trim_end_matches('/').to_string()).collect();
    }
    let Ok(rd) = std::fs::read_dir(repo) else { return Vec::new() };
    let mut v: Vec<String> = rd
        .flatten()
        .filter(|e| e.path().is_file())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with(".env"))
        .filter(|n| Command::new("git").arg("-C").arg(repo).args(["check-ignore", "-q", n]).stdin(Stdio::null()).status().is_ok_and(|s| s.success()))
        .collect();
    v.sort();
    v
}

/// Copy PATH (relative) of REPO into WT: a clone where the file system can (APFS: `cp -c`,
/// Linux: `--reflink=auto`), else a plain copy. A path already in WT (tracked) stays.
fn copy_one(repo: &Path, wt: &Path, path: &str) -> Result<bool, String> {
    let (src, dst) = (repo.join(path), wt.join(path));
    if !src.exists() || dst.exists() || path.contains("..") {
        return Ok(false);
    }
    if let Some(d) = dst.parent() {
        std::fs::create_dir_all(d).map_err(|e| format!("{}: {e}", d.display()))?;
    }
    let clone = if cfg!(target_os = "macos") { "-c" } else { "--reflink=auto" };
    let ok = |args: &[&str]| Command::new("cp").args(args).arg(&src).arg(&dst).stdin(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success());
    if ok(&[clone, "-R"]) || ok(&["-R"]) { Ok(true) } else { Err(format!("cp {path} failed")) }
}

/// Copy the include paths of REPO into the new worktree WT. Returns the copied paths.
pub fn copy_includes(repo: &Path, wt: &Path) -> Vec<String> {
    includes(repo).into_iter().filter(|p| copy_one(repo, wt, p).unwrap_or(false)).collect()
}

/// The command of script KIND (`setup` / `teardown`) in WT: (argv, label, conductor-style).
pub fn script(wt: &Path, kind: &str) -> Option<(Vec<String>, String, bool)> {
    let own = wt.join(".codehead").join(kind);
    if own.is_file() {
        return Some((vec!["sh".into(), own.display().to_string()], format!(".codehead/{kind}"), false));
    }
    if kind == "setup" && wt.join(".agents/setup").is_file() {
        return Some((vec!["sh".into(), wt.join(".agents/setup").display().to_string()], ".agents/setup".into(), false));
    }
    let key = if kind == "setup" { "setup" } else { "archive" };
    let c: serde_json::Value = std::fs::read(wt.join("conductor.json")).ok().and_then(|b| serde_json::from_slice(&b).ok())?;
    let cmd = c["scripts"][key].as_str().filter(|s| !s.trim().is_empty())?;
    Some((vec!["sh".into(), "-c".into(), cmd.to_string()], format!("conductor.json scripts.{key}"), true))
}

/// The env of a script for thread ID: main checkout ROOT, worktree WT, BRANCH, PORT.
pub fn env(root: &Path, wt: &Path, id: &str, branch: &str, port: Option<u16>) -> Vec<(String, String)> {
    let mut v = vec![
        ("CODEHEAD_ROOT".to_string(), root.display().to_string()),
        ("CODEHEAD_WORKTREE".to_string(), wt.display().to_string()),
        ("CODEHEAD_THREAD".to_string(), id.to_string()),
        ("CODEHEAD_BRANCH".to_string(), branch.to_string()),
        ("CONDUCTOR_ROOT_PATH".to_string(), root.display().to_string()),
    ];
    if let Some(p) = port {
        v.push(("CODEHEAD_PORT".into(), p.to_string()));
        v.push(("CONDUCTOR_PORT".into(), p.to_string()));
    }
    v
}

/// Run ARGV in WT with ENV (stdout + stderr together), at most MAX. (ok, last OUTPUT_MAX bytes).
pub fn run(argv: &[String], wt: &Path, env: &[(String, String)], max: Duration) -> (bool, String) {
    use std::io::Read;
    use std::os::unix::process::CommandExt;
    let log = std::env::temp_dir().join(format!("codehead-setup-{}-{}.log", std::process::id(), crate::engine::now_ms()));
    let Ok(file) = std::fs::File::create(&log) else { return (false, "0 log file".into()) };
    let Ok(err) = file.try_clone() else { return (false, "0 log file".into()) };
    let child = Command::new(&argv[0]).args(&argv[1..]).current_dir(wt).envs(env.iter().cloned()).stdin(Stdio::null()).stdout(file).stderr(err).process_group(0).spawn();
    let mut child = match child {
        Ok(c) => c,
        Err(e) => return (false, format!("{}: {e}", argv.join(" "))),
    };
    let start = Instant::now();
    let ok = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s.success(),
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
    if let Ok(mut f) = std::fs::File::open(&log) {
        let _ = f.read_to_string(&mut out);
    }
    let _ = std::fs::remove_file(&log);
    if start.elapsed() > max {
        out.push_str(&format!("\n… stopped after {} min", max.as_secs() / 60));
    }
    let cut = out.len().saturating_sub(OUTPUT_MAX);
    let cut = (cut..out.len()).find(|&i| out.is_char_boundary(i)).unwrap_or(out.len());
    (ok, out[cut..].to_string())
}

/// The setup of thread T (blocking): (label, ok, output). None: 0 setup script.
pub fn run_setup(t: &crate::engine::store::Thread) -> Option<(String, bool, String)> {
    let wt = PathBuf::from(t.worktree.as_deref()?);
    let (argv, label, _) = script(&wt, "setup")?;
    let env = env(Path::new(&t.dir), &wt, &t.id, t.branch.as_deref().unwrap_or_default(), t.port);
    let (ok, out) = run(&argv, &wt, &env, SETUP_MAX);
    Some((label, ok, out))
}

/// The teardown of thread T before its worktree goes (blocking, 2 min max). None: 0 script.
pub fn run_teardown(t: &crate::engine::store::Thread) -> Option<(String, bool, String)> {
    let wt = PathBuf::from(t.worktree.as_deref()?);
    let (argv, label, _) = script(&wt, "teardown")?;
    let env = env(Path::new(&t.dir), &wt, &t.id, t.branch.as_deref().unwrap_or_default(), t.port);
    let (ok, out) = run(&argv, &wt, &env, Duration::from_secs(120));
    Some((label, ok, out))
}

/// The lowest port block no other thread holds.
pub fn free_port(threads: &[crate::engine::store::Thread]) -> u16 {
    let used: Vec<u16> = threads.iter().filter(|t| t.state != "archived").filter_map(|t| t.port).collect();
    (0..2000u16).map(|n| PORT_BASE + n * PORTS).find(|p| !used.contains(p)).unwrap_or(PORT_BASE)
}

/// The base a new worktree branch starts from: `origin/<default>` after a fetch, else HEAD.
pub fn new_branch_base(repo: &Path) -> String {
    if git(repo, &["remote"]).is_some_and(|r| r.lines().any(|l| l == "origin")) {
        let _ = Command::new("git").arg("-C").arg(repo).args(["fetch", "-q", "origin"]).env("GIT_TERMINAL_PROMPT", "0").stdin(Stdio::null()).stderr(Stdio::null()).status();
    }
    let def = git(repo, &["symbolic-ref", "--short", "refs/remotes/origin/HEAD"])
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| ["origin/main", "origin/master"].into_iter().find(|r| git(repo, &["rev-parse", "--verify", "--quiet", r]).is_some()).map(String::from));
    def.unwrap_or_else(|| "HEAD".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sh(d: &Path, c: &str) {
        assert!(Command::new("sh").arg("-c").arg(c).current_dir(d).stdout(Stdio::null()).stderr(Stdio::null()).status().unwrap().success(), "{c}");
    }

    fn repo(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("codehead-setup-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        sh(&d, "git init -q && git config user.email t@t && git config user.name t && printf '.env*\\ncerts/\\n' > .gitignore && echo x > a.rs && git add -A && git commit -qm init");
        d
    }

    #[test]
    fn default_includes_are_the_ignored_env_files() {
        let d = repo("inc");
        sh(&d, "echo S=1 > .env && echo L=1 > .env.local && echo t > .envrc.tracked && git add -f .envrc.tracked && git commit -qm t");
        assert_eq!(includes(&d), [".env", ".env.local"]);
        std::fs::write(d.join(".worktreeinclude"), "# local\n.env\ncerts/\n\n").unwrap();
        assert_eq!(includes(&d), [".env", "certs"]);
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn copy_includes_brings_ignored_files_and_dirs_into_the_worktree() {
        let d = repo("copy");
        sh(&d, "echo S=1 > .env && mkdir -p certs/a && echo k > certs/a/key.pem && printf '.env\\ncerts\\nmissing\\n' > .worktreeinclude");
        let wt = d.join("wt");
        sh(&d, "git worktree add -q -b t wt");
        let copied = copy_includes(&d, &wt);
        assert_eq!(copied, [".env", "certs"]);
        assert_eq!(std::fs::read_to_string(wt.join(".env")).unwrap(), "S=1\n");
        assert!(wt.join("certs/a/key.pem").is_file());
        // a 2nd run: 0 copies over existing files
        std::fs::write(wt.join(".env"), "S=2\n").unwrap();
        assert!(copy_includes(&d, &wt).is_empty());
        assert_eq!(std::fs::read_to_string(wt.join(".env")).unwrap(), "S=2\n");
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn scripts_in_order_and_their_env() {
        let d = repo("script");
        assert!(script(&d, "setup").is_none());
        std::fs::write(d.join("conductor.json"), r#"{"scripts": {"setup": "echo conductor $CONDUCTOR_PORT", "archive": "echo bye"}}"#).unwrap();
        assert_eq!(script(&d, "setup").unwrap().1, "conductor.json scripts.setup");
        assert_eq!(script(&d, "teardown").unwrap().1, "conductor.json scripts.archive");
        std::fs::create_dir_all(d.join(".codehead")).unwrap();
        std::fs::write(d.join(".codehead/setup"), "echo port=$CODEHEAD_PORT root=$CODEHEAD_ROOT\nexit 3\n").unwrap();
        let (argv, label, _) = script(&d, "setup").unwrap();
        assert_eq!(label, ".codehead/setup");
        let (ok, out) = run(&argv, &d, &env(Path::new("/r"), &d, "t1", "b", Some(40010)), Duration::from_secs(10));
        assert!(!ok, "exit 3: failed");
        assert_eq!(out.trim(), "port=40010 root=/r");
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn a_script_over_its_time_gets_killed() {
        let d = repo("slow");
        let (ok, out) = run(&["sh".into(), "-c".into(), "echo start; sleep 30".into()], &d, &[], Duration::from_millis(500));
        assert!(!ok && out.contains("start") && out.contains("stopped after"), "{out}");
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn free_port_skips_held_blocks() {
        let t = |p: Option<u16>| crate::engine::store::Thread { port: p, state: "open".into(), ..Default::default() };
        assert_eq!(free_port(&[]), 40000);
        assert_eq!(free_port(&[t(Some(40000)), t(Some(40020)), t(None)]), 40010);
    }
}
