use super::*;
use ratatui::backend::TestBackend;

fn view(id: &str) -> View {
    let (tx, _rx) = crate::infra::bus::channel();
    let mut view = View::new(id, Path::new("/tmp"), "t", tx);

    view.agents.members = vec!["ada".into(), "zed".into()];
    view.agents.roster = ["ada", "zed", "nova"]
        .iter()
        .map(|name| crate::domain::agents::Agent {
            name: (*name).into(),
            harness: "claude".into(),
            ..Default::default()
        })
        .collect();
    view.sync_order();
    view.agents.spawns.clear();
    view.sync_flowbars();

    view
}

fn key(view: &mut View, code: KeyCode, modifiers: KeyModifiers) {
    view.key(KeyEvent::new(code, modifiers));
}

fn screen(view: &mut View) -> String {
    let mut term = Terminal::new(TestBackend::new(110, 34)).unwrap();

    term.draw(|frame| view.draw(frame)).unwrap();
    let buf = term.backend().buffer().clone();

    (0..34)
        .map(|y| (0..110).map(|x| buf[(x, y)].symbol().to_string()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn tab_highlights_an_agent_and_its_menu_opens_above_it() {
    let mut view = view("agentsbox-0000");

    view.prompt.input.set_text("draft");
    key(&mut view, KeyCode::Tab, KeyModifiers::NONE);

    assert_eq!(
        view.agents.agents_focus.as_ref().map(|focus| focus.at),
        Some(0),
        "the lead first"
    );

    let text = screen(&mut view);

    println!("--- agents box\n{text}");

    assert!(
        text.contains("@ada")
            && text.contains("flow")
            && text.contains("remove")
            && text.contains("replace with")
            && text.contains("0 flow: pick one"),
        "{text}"
    );

    // →: the next agent; letters and digits never reach the prompt
    key(&mut view, KeyCode::Right, KeyModifiers::NONE);

    assert_eq!(view.agents.agents_focus.as_ref().map(|focus| focus.at), Some(1));

    key(&mut view, KeyCode::Char('x'), KeyModifiers::NONE);

    assert_eq!(view.prompt.input.text(), "draft");
    assert!(screen(&mut view).contains("@zed"));

    // ctrl+m: the prompt again
    key(&mut view, KeyCode::Char('m'), KeyModifiers::CONTROL);

    assert!(view.agents.agents_focus.is_none());

    key(&mut view, KeyCode::Char('!'), KeyModifiers::NONE);

    assert_eq!(view.prompt.input.text(), "draft!");
}

#[test]
fn flow_opens_that_agents_flow_picker_and_sets_it_for_that_agent() {
    let mut view = view("agentsbox-0001");

    key(&mut view, KeyCode::Tab, KeyModifiers::NONE);
    key(&mut view, KeyCode::Right, KeyModifiers::NONE);
    key(&mut view, KeyCode::Enter, KeyModifiers::NONE);

    assert!(view.agents.agents_focus.is_none(), "the flow box has the keys");

    let text = screen(&mut view);

    assert!(text.contains("pick a flow for @zed"), "{text}");

    // a flow, then its start step: flow_set for zed
    for character in "chore".chars() {
        key(&mut view, KeyCode::Char(character), KeyModifiers::NONE);
    }

    key(&mut view, KeyCode::Enter, KeyModifiers::NONE);
    key(&mut view, KeyCode::Enter, KeyModifiers::NONE);
    let op = view
        .ops
        .borrow()
        .iter()
        .find(|recorded| recorded["op"] == "flow_set")
        .cloned()
        .unwrap();

    assert_eq!(
        (op["flow"].as_str(), op["agent"].as_str()),
        (Some("chore"), Some("zed"))
    );
}

#[test]
fn remove_asks_a_2nd_enter_and_replace_picks_an_agent_off_the_thread() {
    let mut view = view("agentsbox-0002");

    key(&mut view, KeyCode::Tab, KeyModifiers::NONE);
    key(&mut view, KeyCode::Right, KeyModifiers::NONE);
    key(&mut view, KeyCode::Char('2'), KeyModifiers::NONE);

    assert!(screen(&mut view).contains("enter again: remove @zed"));
    assert!(!view.ops.borrow().iter().any(|recorded| recorded["op"] == "leave"));

    key(&mut view, KeyCode::Enter, KeyModifiers::NONE);
    let op = view
        .ops
        .borrow()
        .iter()
        .find(|recorded| recorded["op"] == "leave")
        .cloned()
        .unwrap();

    assert_eq!(op["agent"].as_str(), Some("zed"));
    assert!(view.agents.agents_focus.is_none());

    // replace with: the agents 0 on the thread (nova)
    key(&mut view, KeyCode::Tab, KeyModifiers::NONE);
    key(&mut view, KeyCode::Char('3'), KeyModifiers::NONE);
    let text = screen(&mut view);

    assert!(
        text.contains("replace @ada with") && text.contains("nova") && !text.contains("│ zed"),
        "{text}"
    );

    key(&mut view, KeyCode::Esc, KeyModifiers::NONE);

    assert!(
        view.agents
            .agents_focus
            .as_ref()
            .is_some_and(|focus| focus.replace.is_none()),
        "esc: back to the menu"
    );

    key(&mut view, KeyCode::Esc, KeyModifiers::NONE);

    assert!(view.agents.agents_focus.is_none(), "esc: the prompt");
}

/// ctrl+p: per agent of the thread 3 rows (flow, remove, replace with), the agents box's actions.
#[test]
fn the_menu_has_the_agents_box_actions_per_agent() {
    let mut view = view("agentsbox-0004");

    view.open_menu();
    let mut term = Terminal::new(TestBackend::new(110, 50)).unwrap();

    term.draw(|frame| view.draw(frame)).unwrap();
    let buf = term.backend().buffer().clone();
    let text: String = (0..50)
        .map(|y| (0..110).map(|x| buf[(x, y)].symbol().to_string()).collect::<String>() + "\n")
        .collect();

    assert!(
        text.contains("@zed: flow (0 flow)") && text.contains("@ada: remove"),
        "{text}"
    );

    // zed's flow row: its flow box on the flows
    view.menu_action(crate::screens::quick_menu::Action::Command(AGENT_ITEMS + 3));

    assert!(
        view.menu.is_none()
            && view
                .flowbars
                .iter()
                .find(|bar| bar.agent == "zed")
                .is_some_and(crate::features::flows::flow_bar::FlowBar::is_open)
    );

    // zed's remove row: the agents box on zed, remove armed (enter removes)
    view.menu_action(crate::screens::quick_menu::Action::Command(AGENT_ITEMS + 4));

    assert!(
        view.agents
            .agents_focus
            .as_ref()
            .is_some_and(|focus| focus.at == 1 && focus.cur == 1 && focus.armed)
    );

    key(&mut view, KeyCode::Enter, KeyModifiers::NONE);

    assert!(
        view.ops
            .borrow()
            .iter()
            .any(|recorded| recorded["op"] == "leave" && recorded["agent"] == "zed")
    );

    // ada's replace row: the replace picker
    view.menu_action(crate::screens::quick_menu::Action::Command(AGENT_ITEMS + 2));

    assert!(
        view.agents
            .agents_focus
            .as_ref()
            .is_some_and(|focus| focus.at == 0 && focus.replace.is_some())
    );
}

/// reply on a gate of a non-lead agent: the message goes to that agent, the gate stays.
#[test]
fn reply_on_a_gate_goes_to_the_waiting_agent() {
    let mut view = view("agentsbox-0005");

    view.flowbars[1].state = Some(crate::engine::flows::run::FlowState {
        flow: "chore".into(),
        title: "chore".into(),
        wait: Some(crate::engine::flows::run::Wait::Gate),
        ..Default::default()
    });
    view.sync_flow_ask();
    view.prompt.input.set_text("also cover X");

    // chore's gate: approve (1), reply (2)
    key(&mut view, KeyCode::Tab, KeyModifiers::NONE);
    key(&mut view, KeyCode::Char('2'), KeyModifiers::NONE);
    let op = view
        .ops
        .borrow()
        .iter()
        .find(|recorded| recorded["op"] == "prompt")
        .cloned()
        .unwrap();

    assert_eq!(op["to"], json!(["zed"]), "{op}");
    assert!(!view.ops.borrow().iter().any(|recorded| recorded["op"] == "flow_answer"));
    assert!(view.agents.reply_to.is_none());

    // an @name in the reply: that agent too, the waiting agent first
    view.prompt.input.set_text("check with @ada first");
    key(&mut view, KeyCode::Tab, KeyModifiers::NONE);
    key(&mut view, KeyCode::Char('2'), KeyModifiers::NONE);
    let op = view
        .ops
        .borrow()
        .iter()
        .rev()
        .find(|recorded| recorded["op"] == "prompt")
        .cloned()
        .unwrap();

    assert_eq!(op["to"], json!(["zed", "ada"]), "{op}");
}

/// ctrl+f, then another view: the view gets the keys, 0 hidden search bar takes them.
#[test]
fn a_view_switch_takes_the_keys_from_the_search_bar() {
    let mut view = view("agentsbox-0006");

    key(&mut view, KeyCode::Char('f'), KeyModifiers::CONTROL);
    key(&mut view, KeyCode::Char('x'), KeyModifiers::NONE);
    key(&mut view, KeyCode::F(4), KeyModifiers::NONE);

    assert!(view.panes.pane == Pane::Files && !view.find.focus);

    key(&mut view, KeyCode::Char('j'), KeyModifiers::NONE);

    assert_eq!(view.find.ta.text(), "x", "j went to the files view");
}

/// The menu's agent rows keep their agent when the order changes before the pick.
#[test]
fn menu_agent_rows_keep_their_agent_after_a_join() {
    let mut view = view("agentsbox-0007");

    view.open_menu();

    // nova joins at the front while the menu is open
    view.agents.members.insert(0, "nova".into());
    view.agents.order.insert(0, "nova".into());
    view.menu_action(crate::screens::quick_menu::Action::Command(AGENT_ITEMS + 4));
    let focus = view.agents.agents_focus.clone().unwrap();

    assert_eq!(
        view.agents.order[focus.at], "zed",
        "row 2 of the menu: zed, not the new 2nd"
    );
}

#[test]
fn ctrl_m_gives_the_prompt_back_from_the_search_bar_and_another_view() {
    let mut view = view("agentsbox-0003");

    key(&mut view, KeyCode::Char('f'), KeyModifiers::CONTROL);

    assert!(view.find.focus);

    key(&mut view, KeyCode::Char('m'), KeyModifiers::CONTROL);

    assert!(
        !view.find.focus && view.find.open,
        "the bar stays, the prompt has the keys"
    );

    key(&mut view, KeyCode::Char('a'), KeyModifiers::NONE);

    assert_eq!(view.prompt.input.text(), "a");

    key(&mut view, KeyCode::F(4), KeyModifiers::NONE);

    assert!(!view.panes.prompt_focus);

    key(&mut view, KeyCode::Char('m'), KeyModifiers::CONTROL);

    assert!(view.panes.prompt_focus);
}
