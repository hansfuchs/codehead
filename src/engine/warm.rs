//! Window warm-up: the new-thread view opens, a harness' short plan window (`five_hour`) is not
//! running yet → 1 tiny message on the harness' weakest model starts it. The owner writes the
//! task at their own pace; the window's clock already runs.
//!
//! "Not running": `limits.json` has 0 report of the window, or its reset time is past. 1 try per
//! harness per [`GAP`] (every codehead process: the mark sits in `limits.json`). The reply's
//! limit reports go into `limits.json`: the next check sees the new window.
//!
//! The watch ([`watch`]): every codehead process (the screen, each host) checks `limits.json`
//! every [`TICK`]. A window whose reset time passed [`DELAY`] ago: 1 trigger message starts the new
//! window. 1 process wins per window (the `_warm` mark under the file's lock). A harness without a
//! report (0 plan window, an API key): 0 trigger.

use std::process::{Command, Stdio};

use serde_json::Value;

/// The short window of the plans.
const WINDOW: &str = "five_hour";
/// Min. seconds between 2 tries per harness.
const GAP: u64 = 600;
/// Seconds after a window's reset: the watch's trigger message.
const DELAY: u64 = 60;
/// The watch's tries end this long after its 1st chance (a trigger without a new report).
const GIVE_UP: u64 = 3600;
/// The watch reads `limits.json` this often.
const TICK: std::time::Duration = std::time::Duration::from_secs(15);

/// A harness with a short plan window: its weakest model, the command of 1 message on it.
struct Warm {
    harness: &'static str,
    cmd: fn() -> Vec<String>,
}

/// The harnesses that report a short window to codehead. Codex: its window goes over ACP
/// without limit reports (0 entry until it reports one).
const ALL: [Warm; 1] = [Warm {
    harness: "claude",
    // 0 tools, 0 MCP servers, 0 settings (0 hooks), 0 saved session
    cmd: || {
        [
            "claude",
            "-p",
            "hi",
            "--model",
            "haiku",
            "--output-format",
            "stream-json",
            "--verbose",
            "--no-session-persistence",
            "--tools",
            "",
            "--strict-mcp-config",
            "--setting-sources",
            "",
        ]
        .map(String::from)
        .to_vec()
    },
}];

/// The window of HARNESS in the limits file LIMITS runs at NOW (unix s).
fn running(limits: &Value, harness: &str, now: u64) -> bool {
    limits[harness][WINDOW]["resets"]
        .as_u64()
        .is_some_and(|resets| resets > now)
}

/// A try of HARNESS at NOW is due: its window not running, its last try GAP+ ago.
fn due(limits: &Value, harness: &str, now: u64) -> bool {
    !running(limits, harness, now)
        && limits["_warm"][harness]
            .as_u64()
            .is_none_or(|last_try| now.saturating_sub(last_try) >= GAP)
}

/// The watch's trigger of HARNESS at NOW is due: its window reset DELAY+ ago, 0 trigger since
/// then (a trigger GAP+ ago that brought 0 new report: 1 more, up to `GIVE_UP`).
fn due_new(limits: &Value, harness: &str, now: u64) -> bool {
    let Some(resets) = limits[harness][WINDOW]["resets"].as_u64() else {
        return false;
    };

    let at = resets + DELAY;

    if now < at {
        return false;
    }

    // tries GAP apart while the last one is within GIVE_UP of the 1st chance
    limits["_warm"][harness]
        .as_u64()
        .is_none_or(|last_try| last_try < at || (now.saturating_sub(last_try) >= GAP && last_try < at + GIVE_UP))
}

/// The watch: a thread per process (once), every TICK the triggers that are due.
pub(crate) fn watch() {
    use std::sync::atomic::{AtomicBool, Ordering};
    static ON: AtomicBool = AtomicBool::new(false);

    if cfg!(test) || ON.swap(true, Ordering::Relaxed) {
        return;
    }

    std::thread::spawn(|| watch_loop());
}

fn watch_loop() -> ! {
    loop {
        std::thread::sleep(TICK);

        let Some(limits) = std::fs::read(super::data_dir().join("limits.json"))
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        else {
            continue;
        };

        let now = super::now_ms() / 1000;

        warm_due_windows(&limits, now);
    }
}

fn warm_due_windows(limits: &Value, now: u64) {
    for warm in ALL.iter().filter(|warm| due_new(limits, warm.harness, now)) {
        if super::harnesses::get(warm.harness).is_some_and(|harness| super::harnesses::on_path(harness.bin))
            && super::limits::claim_warm(warm.harness, now, |current| due_new(current, warm.harness, now))
        {
            super::trace(&format!("warm: new {WINDOW} window of {}", warm.harness));
            run(warm);
        }
    }
}

/// Start every short window that does not run (installed harnesses only), in the background.
pub(crate) fn warm() {
    if cfg!(test) {
        return;
    }

    std::thread::spawn(|| {
        let now = super::now_ms() / 1000;

        for warm in ALL.iter().filter(|warm| {
            super::harnesses::get(warm.harness).is_some_and(|harness| super::harnesses::on_path(harness.bin))
        }) {
            claim_due_window(now, warm);
        }
    });
}

fn claim_due_window(now: u64, warm: &Warm) {
    if super::limits::claim_warm(warm.harness, now, |limits| due(limits, warm.harness, now)) {
        run(warm);
    }
}

/// 1 message on WARM: its limit reports into `limits.json`.
fn run(warm: &Warm) {
    let cmd = (warm.cmd)();

    let Ok(output) = Command::new(&cmd[0])
        .args(&cmd[1..])
        .current_dir(super::data_dir())
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
    else {
        return;
    };

    let mut parser = super::harness::Parser::default();

    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };

        for out in parser.line(&value) {
            record_warm_limits(warm, out);
        }
    }
}

fn record_warm_limits(warm: &Warm, out: super::harness::Out) {
    if let super::harness::Out::Limits(windows) = out {
        super::limits::record(warm.harness, &windows);
    }
}

#[cfg(test)]
#[path = "warm_tests.rs"]
mod tests;
