use super::*;

#[test]
fn profile_change_needs_a_restart() {
    let agent = Agent {
        name: "lilith".into(),
        harness: "claude".into(),
        mode: "plan".into(),
        ..Default::default()
    };

    assert!(
        !profile_changed(
            &agent,
            &Agent {
                color: Some("red".into()),
                ..agent.clone()
            }
        ),
        "color: 0 restart"
    );
    assert!(profile_changed(
        &agent,
        &Agent {
            mode: "autopilot".into(),
            ..agent.clone()
        }
    ));
    assert!(profile_changed(
        &agent,
        &Agent {
            model: Some("fable".into()),
            ..agent.clone()
        }
    ));
    assert!(profile_changed(
        &agent,
        &Agent {
            prompt: Some("new".into()),
            ..agent.clone()
        }
    ));

    // busy session: left alone (0 host calls)
    let usage = Use {
        thread_id: "t".into(),
        live: vec!["lilith".into()],
        busy: vec!["lilith".into()],
        ..Default::default()
    };
    let result = restart(&agent, true, &[usage], Path::new("/tmp"));

    assert_eq!(
        result,
        Restart {
            busy: vec!["t".into()],
            ..Default::default()
        }
    );
}
