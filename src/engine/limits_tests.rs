use super::*;

fn sample(usd: f64, pct: f64, resets: u64) -> Value {
    json!({"ts": 0, "usd": usd, "w": {"claude/five_hour": [pct, resets]}})
}

#[test]
fn the_rate_comes_from_the_latest_period_that_rose_enough() {
    let value = json!({
        "claude": {"five_hour": {"pct": 30.0, "resets": 200}, "seven_day_overage_included": {"pct": 1.0}},
        "_cal": {"usd": 9.0, "samples": [
            sample(0.0, 10.0, 100), sample(4.0, 20.0, 100),
            // a new period: 1 % rise only → the period before counts
            sample(5.0, 29.0, 200), sample(6.0, 30.0, 200),
        ]},
    });
    let windows = parse(&value);

    assert_eq!(windows.len(), 1, "{windows:?}");
    assert_eq!(
        (windows[0].label.as_str(), windows[0].pct, windows[0].resets),
        ("five_hour", 30.0, Some(200))
    );
    assert_eq!(windows[0].usd_per_pct, Some(0.4));

    // the new period rises 3 %: its own rate
    let mut rose = value;

    rose["_cal"]["samples"]
        .as_array_mut()
        .unwrap()
        .push(sample(8.0, 32.0, 200));

    assert_eq!(parse(&rose)[0].usd_per_pct, Some(1.0));

    // 0 samples: 0 rate
    assert_eq!(
        parse(&json!({"claude": {"five_hour": {"pct": 5.0}}}))[0].usd_per_pct,
        None
    );
}

#[test]
fn window_labels_follow_the_duration() {
    let labels: Vec<String> = [300, 10_080, 1440, 43_200, 60, 120, 90].map(window_label).to_vec();

    assert_eq!(
        labels,
        [
            "five_hour",
            "seven_day",
            "1_day",
            "30_day",
            "1_hour",
            "2_hour",
            "90_min"
        ]
    );
}

#[test]
fn each_harness_rates_its_windows_by_its_own_dollars() {
    // an older file: 1 number, Claude's sum
    let mut value = json!({"_cal": {"usd": 4.0, "samples": [sample(0.0, 10.0, 100)]}});

    add_usd(&mut value, "codex", 1.0);

    assert_eq!(value["_cal"]["usd"], json!({"claude": 4.0, "codex": 1.0}));

    record_windows(&mut value, "codex", &[("seven_day".into(), 10.0, Some(500))]);
    add_usd(&mut value, "codex", 2.0);
    add_usd(&mut value, "claude", 100.0);
    record_windows(&mut value, "codex", &[("seven_day".into(), 14.0, Some(500))]);
    record_windows(&mut value, "claude", &[("five_hour".into(), 20.0, Some(100))]);
    let rates: Vec<(String, String, Option<f64>)> = parse(&value)
        .into_iter()
        .map(|window| (window.harness, window.label, window.usd_per_pct))
        .collect();

    // codex: 2 $ over 4 %; claude: 104 $ (the old sum went on) over 10 %
    assert_eq!(
        rates,
        [
            ("codex".to_string(), "seven_day".to_string(), Some(0.5)),
            ("claude".to_string(), "five_hour".to_string(), Some(10.4)),
        ]
    );
}
