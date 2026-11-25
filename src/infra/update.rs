//! Releases: `codehead --version`, `codehead --update` (the public install script), and a
//! background check for a newer release. The check never blocks: a thread asks GitHub at most
//! once per day (cache: ~/.cache/codehead/release), the hint shows after codehead leaves the
//! alternate screen. $`CODEHEAD_NO_UPDATE_CHECK=1` turns the check off.

use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) const VERSION: &str = env!("CARGO_PKG_VERSION");
const REPO: &str = "hansfuchs/codehead";
const INSTALL: &str = "curl -fsSL https://codehead.sh/install | sh";
const CACHE_SECS: u64 = 24 * 60 * 60;

static LATEST: OnceLock<String> = OnceLock::new();

/// Replace this process with the install script: it downloads, verifies and installs the latest release.
pub(crate) fn run() -> ! {
    let err = Command::new("bash").args(["-o", "pipefail", "-c", INSTALL]).exec();

    eprintln!("codehead: run the install script: {err}");
    std::process::exit(1)
}

/// Start the background check (0 effect when turned off).
pub(crate) fn spawn_check() {
    if std::env::var_os("CODEHEAD_NO_UPDATE_CHECK").is_some_and(|value| !value.is_empty() && value != "0") {
        return;
    }

    std::thread::spawn(|| {
        if let Some(latest) = cached_latest() {
            let _ = LATEST.set(latest);
        }
    });
}

/// 1 line when a newer release exists and the check has finished.
pub(crate) fn hint() -> Option<String> {
    let latest = LATEST.get()?;

    newer(latest, VERSION).then(|| format!("codehead {latest} available (this: {VERSION}). update: codehead --update"))
}

fn cache_path() -> PathBuf {
    crate::infra::config::home().join(".cache/codehead/release")
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |directory| directory.as_secs())
}

/// Cache file: `<unix seconds> <version>`. Fresh cache → 0 requests.
fn cached_latest() -> Option<String> {
    let path = cache_path();

    let cached = std::fs::read_to_string(&path).ok().and_then(|text| {
        let (at, value) = text.trim().split_once(' ')?;

        Some((at.parse::<u64>().ok()?, value.to_string()))
    });

    let now = now();

    if let Some((at, value_value)) = &cached
        && *at <= now
        && now - at < CACHE_SECS
    {
        return Some(value_value.clone());
    }

    let Some(latest) = fetch_latest() else {
        return cached.map(|(_, value_2)| value_2);
    };

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
        .args([
            "-fsSLI",
            "--max-time",
            "5",
            "-o",
            "/dev/null",
            "-w",
            "%{url_effective}",
            &url,
        ])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    let effective = String::from_utf8(out.stdout).ok()?;
    let value = effective.trim().rsplit_once("/tag/v")?.1.to_string();

    parse(&value).map(|_| value)
}

fn parse(value: &str) -> Option<Vec<u64>> {
    let parts: Option<Vec<u64>> = value.split('.').map(|path| path.parse().ok()).collect();

    parts.filter(|path_value| !path_value.is_empty())
}

fn newer(latest: &str, current: &str) -> bool {
    match (parse(latest), parse(current)) {
        (Some(line), Some(character)) => line > character,
        _ => false,
    }
}

#[cfg(test)]
#[path = "update_tests.rs"]
mod tests;
