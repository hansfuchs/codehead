use super::*;
use crate::engine::flows::{Library, parse_flow, resolve};

fn sample_flow() -> (Flow, Vec<Resolved>) {
    let flow = parse_flow(
        "t",
        r#"name = "t"
[[step]]
id = "plan"
text = "plan it"
[[step]]
text = "approve"
gate = { changes = "plan" }
[[step]]
id = "review"
text = "review"
[[step]]
text = "address"
loop = { to = "review", until = "0 findings", max = 2 }
[[step]]
text = "report"
"#,
    )
    .unwrap();
    let nodes = resolve(&flow, &Library::default());

    (flow, nodes)
}

#[test]
fn gates_wait_and_outcomes_pick_the_edge() {
    let (flow, nodes) = sample_flow();
    let mut state = FlowState::new(&flow, &Library::default(), 0, 0);

    assert_eq!(state.step_done(&nodes, false, false, 1), Moved::To(1));
    assert_eq!(state.step_done(&nodes, false, false, 2), Moved::Wait(Wait::Gate));

    // a 2nd report while waiting: still waiting
    assert_eq!(state.step_done(&nodes, false, false, 3), Moved::Wait(Wait::Gate));
    assert_eq!(state.answer(&nodes, "changes", 4).unwrap(), Moved::To(0));
    assert!(state.answer(&nodes, "approve", 5).is_err(), "0 wait: 0 answer");

    state.step_done(&nodes, false, false, 6);
    state.step_done(&nodes, false, false, 7);

    assert_eq!(state.answer(&nodes, "approve", 8).unwrap(), Moved::To(2));

    // an autopilot agent passes a gate itself
    let mut passing = state.clone();

    passing.node = 1;

    assert_eq!(passing.step_done(&nodes, false, true, 9), Moved::To(2));
    assert_eq!(passing.history.last().unwrap().how, "passed by agent");
}

#[test]
fn loops_go_back_until_max_then_wait() {
    let (flow, nodes) = sample_flow();
    let mut state = FlowState::new(&flow, &Library::default(), 3, 0);

    // round 1 → back to review
    assert_eq!(state.step_done(&nodes, true, false, 1), Moved::To(2));

    state.step_done(&nodes, false, false, 2);

    // round 2 = max: the owner decides
    assert_eq!(state.step_done(&nodes, true, false, 3), Moved::Wait(Wait::LoopMax));
    assert_eq!(state.answer(&nodes, "again", 4).unwrap(), Moved::To(2));

    state.step_done(&nodes, false, false, 5);

    assert_eq!(state.step_done(&nodes, false, false, 6), Moved::To(4));
    assert_eq!(state.step_done(&nodes, false, false, 7), Moved::Finished);
    assert!(state.finished);
    assert!(outline(&nodes, &state).contains("[loop → 3, max 2]"));
}

#[test]
fn step_messages_say_how_to_report() {
    let (flow, nodes) = sample_flow();
    let state = FlowState::new(&flow, &Library::default(), 3, 0);
    let message = step_message("t", &nodes, 3, &state, Some("the task"));

    assert!(message.starts_with("[codehead flow: t · step 4/5: address]"));
    assert!(message.contains("round 1/2") && message.contains("`again: true`") && message.ends_with("the task"));
    assert_eq!(
        warnings(&nodes, "ada", &[], std::path::Path::new("/tmp")),
        [] as [(usize, std::string::String); 0]
    );
}

/// An old `thread.json` flow: its role map is ignored, a `role` wait reads as 0 wait.
#[test]
fn an_old_state_with_roles_and_a_role_wait_loads() {
    let state: FlowState = serde_json::from_str(r#"{"flow":"chore","title":"chore","node":1,"roles":{"lead":"ada","reviewer":"zed"},"wait":{"kind":"role","role":"reviewer"}}"#).unwrap();

    assert_eq!((state.node, state.wait), (1, None));

    let state: FlowState =
        serde_json::from_str(r#"{"flow":"chore","title":"chore","node":1,"wait":{"kind":"gate"}}"#).unwrap();

    assert_eq!(state.wait, Some(Wait::Gate));
}
