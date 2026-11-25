use super::*;

fn bar(wait: Wait) -> FlowBar {
    let mut bytes = FlowBar::new("ada");

    bytes.state = Some(FlowState {
        flow: "chore".into(),
        title: "chore".into(),
        wait: Some(wait),
        ..Default::default()
    });
    bytes
}

#[test]
fn a_loop_at_its_max_asks_1_more_round_or_move_on() {
    let mut bytes = bar(Wait::LoopMax);
    let agent = bytes.wait_ask().unwrap();

    assert_eq!(agent.tool, WAIT_TOOL);
    assert!(agent.id.starts_with("flow:ada:chore:0:loop"));

    let names: Vec<String> = agent.options().into_iter().map(|output| output.1).collect();

    assert_eq!(
        names,
        [
            "1 more round",
            "move on",
            "reply · the prompt's text to the agent, the wait stays"
        ]
    );

    assert!(matches!(bytes.wait_answer("next"), Some(Act::Answer(character)) if character == "next"));
    assert!(bytes.wait_ask().is_none(), "answered: 0 wait");
}

#[test]
fn d_d_removes_the_flow_x_does_not() {
    let press =
        |bytes: &mut FlowBar, character: char| bytes.key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
    let mut bytes_value = FlowBar::new("ada");

    bytes_value.state = Some(FlowState {
        flow: "chore".into(),
        title: "chore".into(),
        ..Default::default()
    });
    bytes_value.popup_state.open = true;
    bytes_value.popup_state.mode = Mode::Steps;

    assert!(bytes_value.frame_text().1.contains("d d remove"));

    // x: 0 effect
    assert!(matches!(press(&mut bytes_value, 'x'), Some(Act::None)));
    assert!(bytes_value.state.is_some() && bytes_value.popup_state.open);

    // d: armed; another key between: 0 removal; d d: the flow goes
    assert!(matches!(press(&mut bytes_value, 'd'), Some(Act::None)));

    press(&mut bytes_value, 'x');

    assert!(matches!(press(&mut bytes_value, 'd'), Some(Act::None)));
    assert!(bytes_value.state.is_some());
    assert!(matches!(press(&mut bytes_value, 'd'), Some(Act::Host("flow_clear", _))));
    assert!(bytes_value.state.is_none() && !bytes_value.popup_state.open);
}
