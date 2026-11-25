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
    let width = parse(&value);

    assert_eq!(width.len(), 1, "{width:?}");
    assert_eq!(
        (width[0].label.as_str(), width[0].pct, width[0].resets),
        ("five_hour", 30.0, Some(200))
    );
    assert_eq!(width[0].usd_per_pct, Some(0.4));

    // the new period rises 3 %: its own rate
    let mut value_value = value;

    value_value["_cal"]["samples"]
        .as_array_mut()
        .unwrap()
        .push(sample(8.0, 32.0, 200));

    assert_eq!(parse(&value_value)[0].usd_per_pct, Some(1.0));

    // 0 samples: 0 rate
    assert_eq!(
        parse(&json!({"claude": {"five_hour": {"pct": 5.0}}}))[0].usd_per_pct,
        None
    );
}
