use super::*;

fn waiting_bar(wait: Wait) -> FlowBar {
    let mut bar = FlowBar::new("ada");

    bar.state = Some(FlowState {
        flow: "chore".into(),
        title: "chore".into(),
        wait: Some(wait),
        ..Default::default()
    });
    bar
}

#[test]
fn a_loop_at_its_max_asks_1_more_round_or_move_on() {
    let mut bar = waiting_bar(Wait::LoopMax);
    let ask = bar.wait_ask().unwrap();

    assert_eq!(ask.tool, WAIT_TOOL);
    assert!(ask.id.starts_with("flow:ada:chore:0:loop"));

    let names: Vec<String> = ask.options().into_iter().map(|option| option.1).collect();

    assert_eq!(
        names,
        [
            "1 more round",
            "move on",
            "reply · the prompt's text to the agent, the wait stays"
        ]
    );

    assert!(matches!(bar.wait_answer("next"), Some(Act::Answer(choice)) if choice == "next"));
    assert!(bar.wait_ask().is_none(), "answered: 0 wait");
}

#[test]
fn two_d_presses_remove_the_flow_x_does_not() {
    let press = |bar: &mut FlowBar, key: char| bar.key(KeyEvent::new(KeyCode::Char(key), KeyModifiers::NONE));
    let mut bar = FlowBar::new("ada");

    bar.state = Some(FlowState {
        flow: "chore".into(),
        title: "chore".into(),
        ..Default::default()
    });
    bar.popup_state.open = true;
    bar.popup_state.mode = Mode::Steps;

    assert!(bar.frame_text().1.contains("d d remove"));

    // x: 0 effect
    assert!(matches!(press(&mut bar, 'x'), Some(Act::None)));
    assert!(bar.state.is_some() && bar.popup_state.open);

    // d: armed; another key between: 0 removal; d d: the flow goes
    assert!(matches!(press(&mut bar, 'd'), Some(Act::None)));

    press(&mut bar, 'x');

    assert!(matches!(press(&mut bar, 'd'), Some(Act::None)));
    assert!(bar.state.is_some());
    assert!(matches!(press(&mut bar, 'd'), Some(Act::Host("flow_clear", _))));
    assert!(bar.state.is_none() && !bar.popup_state.open);
}
