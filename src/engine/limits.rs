//! Plan limits (`<data>/limits.json`): the windows the harnesses report (used %, reset time),
//! 1 file for every host (the windows are the account's, not the thread's), under its lock.
//!
//! The calibration (`_cal`): the $ of every Claude turn codehead ran, summed, and a sample of
//! that sum per limits report. 2 samples of 1 window period: $ per 1 % of the window, the base of
//! the next message's share of a window. Use outside codehead (claude.ai, other sessions) adds %
//! without $: the share comes out high, never low.

use serde_json::{Value, json};

/// Samples kept (the last ones).
const SAMPLES: usize = 400;
/// Min. rise of a window's % between 2 samples of a period: a rate from less is noise.
const MIN_RISE: f64 = 2.0;

fn path() -> std::path::PathBuf {
    super::data_dir().join("limits.json")
}

/// F on the file's JSON, under its lock, written back.
fn edit(f: impl FnOnce(&mut Value)) {
    let p = path();
    let _ = super::locked(&super::data_dir().join("limits.lock"), || {
        let mut v: Value = std::fs::read(&p).ok().and_then(|t| serde_json::from_slice(&t).ok()).unwrap_or_else(|| json!({}));
        if !v.is_object() {
            v = json!({});
        }
        f(&mut v);
        super::write_atomic(&p, v.to_string().as_bytes())
    });
}

/// A turn of a Claude agent ended: its $ into the sum.
pub fn add_cost(usd: f64) {
    if usd <= 0.0 {
        return;
    }
    edit(|v| {
        let sum = v["_cal"]["usd"].as_f64().unwrap_or(0.0) + usd;
        v["_cal"]["usd"] = json!(sum);
    });
}

/// HARNESS reported windows L (window, used %, resets at): the new values, a calibration sample.
pub fn record(harness: &str, l: &[(String, f64, Option<u64>)]) {
    edit(|v| {
        for (w, pct, resets) in l {
            v[harness][w] = json!({"pct": pct, "resets": resets});
        }
        let usd = v["_cal"]["usd"].as_f64().unwrap_or(0.0);
        let windows: serde_json::Map<String, Value> = l.iter().map(|(w, pct, resets)| (format!("{harness}/{w}"), json!([pct, resets]))).collect();
        let mut samples = v["_cal"]["samples"].as_array().cloned().unwrap_or_default();
        samples.push(json!({"ts": super::now_ms() / 1000, "usd": usd, "w": windows}));
        let drop = samples.len().saturating_sub(SAMPLES);
        samples.drain(..drop);
        v["_cal"]["samples"] = Value::Array(samples);
    });
}

/// A window of a plan.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Window {
    pub harness: String,
    pub window: String,
    pub pct: f64,
    pub resets: Option<u64>,
    /// $ of codehead turns per 1 % of the window (None: too few samples yet).
    pub usd_per_pct: Option<f64>,
}

/// The windows the hosts saw last, with their rates.
pub fn read() -> Vec<Window> {
    let Ok(text) = std::fs::read(path()) else { return Vec::new() };
    let Ok(v) = serde_json::from_slice::<Value>(&text) else { return Vec::new() };
    parse(&v)
}

fn parse(v: &Value) -> Vec<Window> {
    let samples = v["_cal"]["samples"].as_array().cloned().unwrap_or_default();
    let mut out = Vec::new();
    for (h, ws) in v.as_object().into_iter().flatten() {
        if h.starts_with('_') {
            continue;
        }
        for (w, l) in ws.as_object().into_iter().flatten() {
            // `seven_day_overage_included`: the 7d window again, with overage: 0 own row
            if w.ends_with("_overage_included") {
                continue;
            }
            out.push(Window { harness: h.clone(), window: w.clone(), pct: l["pct"].as_f64().unwrap_or(0.0), resets: l["resets"].as_u64(), usd_per_pct: rate(&samples, &format!("{h}/{w}")) });
        }
    }
    out
}

/// $ per 1 % of window KEY: the latest period (1 reset time) whose % rose by MIN_RISE+, from its
/// 1st sample to its last.
fn rate(samples: &[Value], key: &str) -> Option<f64> {
    // per period, in order: (resets, (usd, pct) first, (usd, pct) last)
    let mut periods: Vec<(Option<u64>, (f64, f64), (f64, f64))> = Vec::new();
    for s in samples {
        let Some(w) = s["w"].get(key) else { continue };
        let (pct, resets, usd) = (w[0].as_f64().unwrap_or(0.0), w[1].as_u64(), s["usd"].as_f64().unwrap_or(0.0));
        match periods.last_mut() {
            Some(p) if p.0 == resets && pct >= p.2.1 => p.2 = (usd, pct),
            _ => periods.push((resets, (usd, pct), (usd, pct))),
        }
    }
    periods.iter().rev().find_map(|(_, a, b)| {
        let (du, dp) = (b.0 - a.0, b.1 - a.1);
        (dp >= MIN_RISE && du > 0.0).then(|| du / dp)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(usd: f64, pct: f64, resets: u64) -> Value {
        json!({"ts": 0, "usd": usd, "w": {"claude/five_hour": [pct, resets]}})
    }

    #[test]
    fn the_rate_comes_from_the_latest_period_that_rose_enough() {
        let v = json!({
            "claude": {"five_hour": {"pct": 30.0, "resets": 200}, "seven_day_overage_included": {"pct": 1.0}},
            "_cal": {"usd": 9.0, "samples": [
                sample(0.0, 10.0, 100), sample(4.0, 20.0, 100),
                // a new period: 1 % rise only → the period before counts
                sample(5.0, 29.0, 200), sample(6.0, 30.0, 200),
            ]},
        });
        let w = parse(&v);
        assert_eq!(w.len(), 1, "{w:?}");
        assert_eq!((w[0].window.as_str(), w[0].pct, w[0].resets), ("five_hour", 30.0, Some(200)));
        assert_eq!(w[0].usd_per_pct, Some(0.4));
        // the new period rises 3 %: its own rate
        let mut v = v;
        v["_cal"]["samples"].as_array_mut().unwrap().push(sample(8.0, 32.0, 200));
        assert_eq!(parse(&v)[0].usd_per_pct, Some(1.0));
        // 0 samples: 0 rate
        assert_eq!(parse(&json!({"claude": {"five_hour": {"pct": 5.0}}}))[0].usd_per_pct, None);
    }
}
