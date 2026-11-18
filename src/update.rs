//! Releases: `codehead --version`, `codehead --update` (the public install script), and a
//! background check for a newer release. The check never blocks: a thread asks GitHub at most
//! once per day (cache: ~/.cache/codehead/release), the hint shows after codehead leaves the
//! alternate screen. $CODEHEAD_NO_UPDATE_CHECK=1 turns the check off.

use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
const REPO: &str = "hansfuchs/codehead";
const INSTALL: &str = "curl -fsSL https://codehead.sh/install | sh";
const CACHE_SECS: u64 = 24 * 60 * 60;

static LATEST: OnceLock<String> = OnceLock::new();

/// Replace this process with the install script: it downloads, verifies and installs the latest release.
pub fn run() -> ! {
    let err = Command::new("bash").args(["-o", "pipefail", "-c", INSTALL]).exec();
    eprintln!("codehead: run the install script: {err}");
    std::process::exit(1)
}

/// Start the background check (0 effect when turned off).
pub fn spawn_check() {
    if std::env::var_os("CODEHEAD_NO_UPDATE_CHECK").is_some_and(|v| !v.is_empty() && v != "0") {
        return;
    }
    std::thread::spawn(|| {
        if let Some(latest) = cached_latest() {
            let _ = LATEST.set(latest);
        }
    });
}

/// 1 line when a newer release exists and the check has finished.
pub fn hint() -> Option<String> {
    let latest = LATEST.get()?;
    newer(latest, VERSION).then(|| format!("codehead {latest} available (this: {VERSION}). update: codehead --update"))
}

fn cache_path() -> PathBuf {
    crate::config::home().join(".cache/codehead/release")
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Cache file: `<unix seconds> <version>`. Fresh cache → 0 requests.
fn cached_latest() -> Option<String> {
    let path = cache_path();
    let cached = std::fs::read_to_string(&path).ok().and_then(|s| {
        let (at, v) = s.trim().split_once(' ')?;
        Some((at.parse::<u64>().ok()?, v.to_string()))
    });
    let now = now();
    if let Some((at, v)) = &cached
        && *at <= now
        && now - at < CACHE_SECS
    {
        return Some(v.clone());
    }
    let Some(latest) = fetch_latest() else { return cached.map(|(_, v)| v) };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(&path, format!("{now} {latest}\n"));
    Some(latest)
}

/// /releases/latest redirects to /releases/tag/v<version>: 0 API calls, 0 rate limit.
fn fetch_latest() -> Option<String> {
    let url = format!("https://github.com/{REPO}/releases/latest");
    let out = Command::new("curl")
        .args(["-fsSLI", "--max-time", "5", "-o", "/dev/null", "-w", "%{url_effective}", &url])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    let effective = String::from_utf8(out.stdout).ok()?;
    let v = effective.trim().rsplit_once("/tag/v")?.1.to_string();
    parse(&v).map(|_| v)
}

fn parse(v: &str) -> Option<Vec<u64>> {
    let parts: Option<Vec<u64>> = v.split('.').map(|p| p.parse().ok()).collect();
    parts.filter(|p| !p.is_empty())
}

fn newer(latest: &str, current: &str) -> bool {
    match (parse(latest), parse(current)) {
        (Some(l), Some(c)) => l > c,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_numerically() {
        assert!(newer("0.2.10", "0.2.9"));
        assert!(newer("0.3.1", "0.2.40"));
        assert!(!newer("0.2.9", "0.2.9"));
        assert!(!newer("0.2.8", "0.2.9"));
        assert!(!newer("garbage", "0.2.9"));
    }
}
