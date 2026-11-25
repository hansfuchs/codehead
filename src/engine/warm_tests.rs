use super::*;
use serde_json::json;

#[test]
fn a_try_is_due_without_a_running_window_and_a_recent_try() {
    // 0 report: due
    assert!(due(&json!({}), "claude", 1000));

    // window runs: 0 try
    assert!(!due(
        &json!({"claude": {"five_hour": {"resets": 2000}}}),
        "claude",
        1000
    ));

    // window over: due
    assert!(due(&json!({"claude": {"five_hour": {"resets": 900}}}), "claude", 1000));

    // a try 100 s ago: 0 try; 700 s ago: due
    assert!(!due(&json!({"_warm": {"claude": 900}}), "claude", 1000));
    assert!(due(&json!({"_warm": {"claude": 300}}), "claude", 1000));
}

#[test]
fn the_watch_triggers_1_min_after_a_reset_once() {
    let value_value = |warm: Option<u64>| {
        let mut value = json!({"claude": {"five_hour": {"resets": 1000}}});

        if let Some(thread) = warm {
            value["_warm"]["claude"] = json!(thread);
        }

        value
    };

    // 0 report: 0 trigger (an API key, 0 plan)
    assert!(!due_new(&json!({}), "claude", 5000));

    // the window runs / reset < 60 s ago: 0 trigger; 60 s after: due
    assert!(!due_new(&value_value(None), "claude", 900));
    assert!(!due_new(&value_value(None), "claude", 1059));
    assert!(due_new(&value_value(None), "claude", 1060));

    // a trigger before the reset (the last window's): due; after it: 0, until GAP
    assert!(due_new(&value_value(Some(500)), "claude", 1060));
    assert!(!due_new(&value_value(Some(1060)), "claude", 1200));
    assert!(due_new(&value_value(Some(1060)), "claude", 1060 + GAP));

    // codehead started hours after the reset: due; the last try GIVE_UP after the 1st chance: 0
    assert!(due_new(&value_value(None), "claude", 1000 + 5 * GIVE_UP));
    assert!(!due_new(
        &value_value(Some(1060 + GIVE_UP)),
        "claude",
        1060 + 2 * GIVE_UP
    ));
}
