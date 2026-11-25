//! Plan limits (`<data>/limits.json`): the windows the harnesses report (used %, reset time),
//! 1 file for every host (the windows are the account's, not the thread's), under its lock.
//!
//! The calibration (`_cal`): the $ of every Claude turn codehead ran, summed, and a sample of
//! that sum per limits report. 2 samples of 1 window period: $ per 1 % of the window, the base of
//! the next message's share of a window. Use outside codehead (claude.ai, other sessions) adds %
//! without $: the share comes out high, never low.

use std::ops::ControlFlow;

use serde_json::{Value, json};

/// Samples kept (the last ones).
const SAMPLES: usize = 400;
/// Min. rise of a window's % between 2 samples of a period: a rate from less is noise.
const MIN_RISE: f64 = 2.0;

fn path() -> std::path::PathBuf {
    super::data_dir().join("limits.json")
}

/// CHANGE on the file's JSON, under its lock, written back.
fn edit(change: impl FnOnce(&mut Value)) {
    let file = path();

    let _ = super::locked(&super::data_dir().join("limits.lock"), || {
        let mut value: Value = std::fs::read(&file)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_else(|| json!({}));

        if !value.is_object() {
            value = json!({});
        }

        change(&mut value);
        super::write_atomic(&file, value.to_string().as_bytes())
    });
}

/// The warm-up of HARNESS (`engine::warm`) at NOW: DUE on the file → marked (`_warm`), true. 1
/// process wins per try (the lock).
pub(crate) fn claim_warm(harness: &str, now: u64, due: impl FnOnce(&Value) -> bool) -> bool {
    let mut won = false;

    edit(|value| {
        if due(value) {
            value["_warm"][harness] = json!(now);
            won = true;
        }
    });

    won
}

/// A turn of a Claude agent ended: its $ into the sum.
pub(crate) fn add_cost(usd: f64) {
    if usd <= 0.0 {
        return;
    }

    edit(|value| {
        let sum = value["_cal"]["usd"].as_f64().unwrap_or(0.0) + usd;

        value["_cal"]["usd"] = json!(sum);
    });
}

/// HARNESS reported WINDOWS (window, used %, resets at): the new values, a calibration sample.
pub(crate) fn record(harness: &str, windows: &[(String, f64, Option<u64>)]) {
    edit(|value| {
        for (window, pct, resets) in windows {
            value[harness][window] = json!({"pct": pct, "resets": resets});
        }

        let usd = value["_cal"]["usd"].as_f64().unwrap_or(0.0);
        let reported: serde_json::Map<String, Value> = windows
            .iter()
            .map(|(window, pct, resets)| (format!("{harness}/{window}"), json!([pct, resets])))
            .collect();
        let mut samples = value["_cal"]["samples"].as_array().cloned().unwrap_or_default();

        samples.push(json!({"ts": super::now_ms() / 1000, "usd": usd, "w": reported}));
        let drop = samples.len().saturating_sub(SAMPLES);

        samples.drain(..drop);
        value["_cal"]["samples"] = Value::Array(samples);
    });
}

/// A window of a plan.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Window {
    pub harness: String,
    pub label: String,
    pub pct: f64,
    pub resets: Option<u64>,
    /// $ of codehead turns per 1 % of the window (None: too few samples yet).
    pub usd_per_pct: Option<f64>,
}

/// The windows the hosts saw last, with their rates.
pub(crate) fn read() -> Vec<Window> {
    let Ok(text) = std::fs::read(path()) else {
        return Vec::new();
    };

    let Ok(value) = serde_json::from_slice::<Value>(&text) else {
        return Vec::new();
    };

    parse(&value)
}

fn parse(value: &Value) -> Vec<Window> {
    let samples = value["_cal"]["samples"].as_array().cloned().unwrap_or_default();
    let mut out = Vec::new();

    for (harness, windows) in value.as_object().into_iter().flatten() {
        if harness.starts_with('_') {
            continue;
        }

        parse_windows(&samples, &mut out, harness, windows);
    }

    out
}

fn parse_windows(samples: &[Value], out: &mut Vec<Window>, harness: &String, windows: &Value) {
    for (label, window) in windows.as_object().into_iter().flatten() {
        // `seven_day_overage_included`: the 7d window again, with overage: 0 own row
        if let ControlFlow::Break(()) = skip_overage_window(label) {
            continue;
        }

        out.push(Window {
            harness: harness.clone(),
            label: label.clone(),
            pct: window["pct"].as_f64().unwrap_or(0.0),
            resets: window["resets"].as_u64(),
            usd_per_pct: rate(samples, &format!("{harness}/{label}")),
        });
    }
}

fn skip_overage_window(label: &str) -> ControlFlow<()> {
    if label.ends_with("_overage_included") {
        return ControlFlow::Break(());
    }

    ControlFlow::Continue(())
}

/// $ per 1 % of window KEY: the latest period (1 reset time) whose % rose by `MIN_RISE`+, from its
/// 1st sample to its last.
fn rate(samples: &[Value], key: &str) -> Option<f64> {
    // per period, in order: (resets, (usd, pct) first, (usd, pct) last)
    let mut periods: Vec<WindowPeriod> = Vec::new();

    for sample in samples {
        let Some(window) = sample["w"].get(key) else { continue };
        let (pct, resets, usd) = (
            window[0].as_f64().unwrap_or(0.0),
            window[1].as_u64(),
            sample["usd"].as_f64().unwrap_or(0.0),
        );

        match periods.last_mut() {
            Some(period) if period.reset == resets && pct >= period.last.percent => {
                period.last = UsageSample { usd, percent: pct }
            }
            _ => periods.push(WindowPeriod {
                reset: resets,
                first: UsageSample { usd, percent: pct },
                last: UsageSample { usd, percent: pct },
            }),
        }
    }

    periods.iter().rev().find_map(|period| {
        let (usd_rise, pct_rise) = (
            period.last.usd - period.first.usd,
            period.last.percent - period.first.percent,
        );

        (pct_rise >= MIN_RISE && usd_rise > 0.0).then(|| usd_rise / pct_rise)
    })
}

#[cfg(test)]
#[path = "limits_tests.rs"]
mod tests;

struct UsageSample {
    usd: f64,
    percent: f64,
}
struct WindowPeriod {
    reset: Option<u64>,
    first: UsageSample,
    last: UsageSample,
}
