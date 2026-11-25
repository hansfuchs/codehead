use super::*;
use crate::engine::flows::{Library, parse_flow, resolve};

fn flow() -> (Flow, Vec<Resolved>) {
    let file = parse_flow(
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
    let count = resolve(&file, &Library::default());

    (file, count)
}

#[test]
fn gates_wait_and_outcomes_pick_the_edge() {
    let (file, count) = flow();
    let mut st = FlowState::new(&file, &Library::default(), 0, 0);

    assert_eq!(st.step_done(&count, false, false, 1), Moved::To(1));
    assert_eq!(st.step_done(&count, false, false, 2), Moved::Wait(Wait::Gate));

    // a 2nd report while waiting: still waiting
    assert_eq!(st.step_done(&count, false, false, 3), Moved::Wait(Wait::Gate));
    assert_eq!(st.answer(&count, "changes", 4).unwrap(), Moved::To(0));
    assert!(st.answer(&count, "approve", 5).is_err(), "0 wait: 0 answer");

    st.step_done(&count, false, false, 6);
    st.step_done(&count, false, false, 7);

    assert_eq!(st.answer(&count, "approve", 8).unwrap(), Moved::To(2));

    // an autopilot agent passes a gate itself
    let mut s2 = st.clone();

    s2.node = 1;

    assert_eq!(s2.step_done(&count, false, true, 9), Moved::To(2));
    assert_eq!(s2.history.last().unwrap().how, "passed by agent");
}

#[test]
fn loops_go_back_until_max_then_wait() {
    let (file, count) = flow();
    let mut st = FlowState::new(&file, &Library::default(), 3, 0);

    // round 1 → back to review
    assert_eq!(st.step_done(&count, true, false, 1), Moved::To(2));

    st.step_done(&count, false, false, 2);

    // round 2 = max: the owner decides
    assert_eq!(st.step_done(&count, true, false, 3), Moved::Wait(Wait::LoopMax));
    assert_eq!(st.answer(&count, "again", 4).unwrap(), Moved::To(2));

    st.step_done(&count, false, false, 5);

    assert_eq!(st.step_done(&count, false, false, 6), Moved::To(4));
    assert_eq!(st.step_done(&count, false, false, 7), Moved::Finished);
    assert!(st.finished);
    assert!(outline(&count, &st).contains("[loop → 3, max 2]"));
}

#[test]
fn step_messages_say_how_to_report() {
    let (file, count) = flow();
    let st = FlowState::new(&file, &Library::default(), 3, 0);
    let message = step_message("t", &count, 3, &st, Some("the task"));

    assert!(message.starts_with("[codehead flow: t · step 4/5: address]"));
    assert!(message.contains("round 1/2") && message.contains("`again: true`") && message.ends_with("the task"));
    assert_eq!(
        warnings(&count, "ada", &[], std::path::Path::new("/tmp")),
        [] as [(usize, std::string::String); 0]
    );
}

/// An old `thread.json` flow: its role map is ignored, a `role` wait reads as 0 wait.
#[test]
fn an_old_state_with_roles_and_a_role_wait_loads() {
    let st: FlowState = serde_json::from_str(r#"{"flow":"chore","title":"chore","node":1,"roles":{"lead":"ada","reviewer":"zed"},"wait":{"kind":"role","role":"reviewer"}}"#).unwrap();

    assert_eq!((st.node, st.wait), (1, None));

    let st: FlowState =
        serde_json::from_str(r#"{"flow":"chore","title":"chore","node":1,"wait":{"kind":"gate"}}"#).unwrap();

    assert_eq!(st.wait, Some(Wait::Gate));
}
