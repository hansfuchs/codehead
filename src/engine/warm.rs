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
    cmd: || ["claude", "-p", "hi", "--model", "haiku", "--output-format", "stream-json", "--verbose", "--no-session-persistence", "--tools", "", "--strict-mcp-config", "--setting-sources", ""].map(String::from).to_vec(),
}];

/// The window of HARNESS in limits file V runs at NOW (unix s).
fn running(v: &Value, harness: &str, now: u64) -> bool {
    v[harness][WINDOW]["resets"].as_u64().is_some_and(|r| r > now)
}

/// A try of HARNESS at NOW is due: its window not running, its last try GAP+ ago.
fn due(v: &Value, harness: &str, now: u64) -> bool {
    !running(v, harness, now) && v["_warm"][harness].as_u64().is_none_or(|t| now.saturating_sub(t) >= GAP)
}

/// The watch's trigger of HARNESS at NOW is due: its window reset DELAY+ ago, 0 trigger since
/// then (a trigger GAP+ ago that brought 0 new report: 1 more, up to GIVE_UP).
fn due_new(v: &Value, harness: &str, now: u64) -> bool {
    let Some(r) = v[harness][WINDOW]["resets"].as_u64() else { return false };
    let at = r + DELAY;
    if now < at {
        return false;
    }
    // tries GAP apart while the last one is within GIVE_UP of the 1st chance
    v["_warm"][harness].as_u64().is_none_or(|t| t < at || (now.saturating_sub(t) >= GAP && t < at + GIVE_UP))
}

/// The watch: a thread per process (once), every TICK the triggers that are due.
pub fn watch() {
    use std::sync::atomic::{AtomicBool, Ordering};
    static ON: AtomicBool = AtomicBool::new(false);
    if cfg!(test) || ON.swap(true, Ordering::Relaxed) {
        return;
    }
    std::thread::spawn(|| {
        loop {
            std::thread::sleep(TICK);
            let Some(v) = std::fs::read(super::data_dir().join("limits.json")).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok()) else { continue };
            let now = super::now_ms() / 1000;
            for w in ALL.iter().filter(|w| due_new(&v, w.harness, now)) {
                if super::harnesses::get(w.harness).is_some_and(|h| super::harnesses::on_path(h.bin)) && super::limits::claim_warm(w.harness, now, |v| due_new(v, w.harness, now)) {
                    super::trace(&format!("warm: new {WINDOW} window of {}", w.harness));
                    run(w);
                }
            }
        }
    });
}

/// Start every short window that does not run (installed harnesses only), in the background.
pub fn warm() {
    if cfg!(test) {
        return;
    }
    std::thread::spawn(|| {
        let now = super::now_ms() / 1000;
        for w in ALL.iter().filter(|w| super::harnesses::get(w.harness).is_some_and(|h| super::harnesses::on_path(h.bin))) {
            if super::limits::claim_warm(w.harness, now, |v| due(v, w.harness, now)) {
                run(w);
            }
        }
    });
}

/// 1 message on W: its limit reports into `limits.json`.
fn run(w: &Warm) {
    let cmd = (w.cmd)();
    let Ok(out) = Command::new(&cmd[0]).args(&cmd[1..]).current_dir(super::data_dir()).stdin(Stdio::null()).stderr(Stdio::null()).output() else { return };
    let mut p = super::harness::Parser::default();
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
        for o in p.line(&v) {
            if let super::harness::Out::Limits(l) = o {
                super::limits::record(w.harness, &l);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_try_is_due_without_a_running_window_and_a_recent_try() {
        // 0 report: due
        assert!(due(&json!({}), "claude", 1000));
        // window runs: 0 try
        assert!(!due(&json!({"claude": {"five_hour": {"resets": 2000}}}), "claude", 1000));
        // window over: due
        assert!(due(&json!({"claude": {"five_hour": {"resets": 900}}}), "claude", 1000));
        // a try 100 s ago: 0 try; 700 s ago: due
        assert!(!due(&json!({"_warm": {"claude": 900}}), "claude", 1000));
        assert!(due(&json!({"_warm": {"claude": 300}}), "claude", 1000));
    }

    #[test]
    fn the_watch_triggers_1_min_after_a_reset_once() {
        let v = |warm: Option<u64>| {
            let mut v = json!({"claude": {"five_hour": {"resets": 1000}}});
            if let Some(t) = warm {
                v["_warm"]["claude"] = json!(t);
            }
            v
        };
        // 0 report: 0 trigger (an API key, 0 plan)
        assert!(!due_new(&json!({}), "claude", 5000));
        // the window runs / reset < 60 s ago: 0 trigger; 60 s after: due
        assert!(!due_new(&v(None), "claude", 900));
        assert!(!due_new(&v(None), "claude", 1059));
        assert!(due_new(&v(None), "claude", 1060));
        // a trigger before the reset (the last window's): due; after it: 0, until GAP
        assert!(due_new(&v(Some(500)), "claude", 1060));
        assert!(!due_new(&v(Some(1060)), "claude", 1200));
        assert!(due_new(&v(Some(1060)), "claude", 1060 + GAP));
        // codehead started hours after the reset: due; the last try GIVE_UP after the 1st chance: 0
        assert!(due_new(&v(None), "claude", 1000 + 5 * GIVE_UP));
        assert!(!due_new(&v(Some(1060 + GIVE_UP)), "claude", 1060 + 2 * GIVE_UP));
    }
}
