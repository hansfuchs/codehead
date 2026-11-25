use super::*;
use ratatui::backend::TestBackend;

fn view(id: &str) -> View {
    let (tx, _rx) = crate::infra::bus::channel();
    let mut view = View::new(id, Path::new("/tmp"), "t", tx);

    view.agents.members = vec!["ada".into(), "zed".into()];
    view.agents.roster = ["ada", "zed", "nova"]
        .iter()
        .map(|count| crate::domain::agents::Agent {
            name: (*count).into(),
            harness: "claude".into(),
            ..Default::default()
        })
        .collect();
    view.sync_order();
    view.agents.spawns.clear();
    view.sync_flowbars();

    view
}

fn key(view: &mut View, character: KeyCode, message: KeyModifiers) {
    view.key(KeyEvent::new(character, message));
}

fn screen(view: &mut View) -> String {
    let mut thread = Terminal::new(TestBackend::new(110, 34)).unwrap();

    thread.draw(|file| view.draw(file)).unwrap();
    let bytes = thread.backend().buffer().clone();

    (0..34)
        .map(|y| (0..110).map(|x| bytes[(x, y)].symbol().to_string()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn tab_highlights_an_agent_and_its_menu_opens_above_it() {
    let mut value = view("agentsbox-0000");

    value.prompt.input.set_text("draft");
    key(&mut value, KeyCode::Tab, KeyModifiers::NONE);

    assert_eq!(
        value.agents.agents_focus.as_ref().map(|file| file.at),
        Some(0),
        "the lead first"
    );

    let text = screen(&mut value);

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
    key(&mut value, KeyCode::Right, KeyModifiers::NONE);

    assert_eq!(value.agents.agents_focus.as_ref().map(|file| file.at), Some(1));

    key(&mut value, KeyCode::Char('x'), KeyModifiers::NONE);

    assert_eq!(value.prompt.input.text(), "draft");
    assert!(screen(&mut value).contains("@zed"));

    // ctrl+m: the prompt again
    key(&mut value, KeyCode::Char('m'), KeyModifiers::CONTROL);

    assert!(value.agents.agents_focus.is_none());

    key(&mut value, KeyCode::Char('!'), KeyModifiers::NONE);

    assert_eq!(value.prompt.input.text(), "draft!");
}

#[test]
fn flow_opens_that_agents_flow_picker_and_sets_it_for_that_agent() {
    let mut value = view("agentsbox-0001");

    key(&mut value, KeyCode::Tab, KeyModifiers::NONE);
    key(&mut value, KeyCode::Right, KeyModifiers::NONE);
    key(&mut value, KeyCode::Enter, KeyModifiers::NONE);

    assert!(value.agents.agents_focus.is_none(), "the flow box has the keys");

    let text = screen(&mut value);

    assert!(text.contains("pick a flow for @zed"), "{text}");

    // a flow, then its start step: flow_set for zed
    for character in "chore".chars() {
        key(&mut value, KeyCode::Char(character), KeyModifiers::NONE);
    }

    key(&mut value, KeyCode::Enter, KeyModifiers::NONE);
    key(&mut value, KeyCode::Enter, KeyModifiers::NONE);
    let op = value
        .ops
        .borrow()
        .iter()
        .find(|output| output["op"] == "flow_set")
        .cloned()
        .unwrap();

    assert_eq!(
        (op["flow"].as_str(), op["agent"].as_str()),
        (Some("chore"), Some("zed"))
    );
}

#[test]
fn remove_asks_a_2nd_enter_and_replace_picks_an_agent_off_the_thread() {
    let mut value = view("agentsbox-0002");

    key(&mut value, KeyCode::Tab, KeyModifiers::NONE);
    key(&mut value, KeyCode::Right, KeyModifiers::NONE);
    key(&mut value, KeyCode::Char('2'), KeyModifiers::NONE);

    assert!(screen(&mut value).contains("enter again: remove @zed"));
    assert!(!value.ops.borrow().iter().any(|output| output["op"] == "leave"));

    key(&mut value, KeyCode::Enter, KeyModifiers::NONE);
    let op = value
        .ops
        .borrow()
        .iter()
        .find(|output| output["op"] == "leave")
        .cloned()
        .unwrap();

    assert_eq!(op["agent"].as_str(), Some("zed"));
    assert!(value.agents.agents_focus.is_none());

    // replace with: the agents 0 on the thread (nova)
    key(&mut value, KeyCode::Tab, KeyModifiers::NONE);
    key(&mut value, KeyCode::Char('3'), KeyModifiers::NONE);
    let text = screen(&mut value);

    assert!(
        text.contains("replace @ada with") && text.contains("nova") && !text.contains("│ zed"),
        "{text}"
    );

    key(&mut value, KeyCode::Esc, KeyModifiers::NONE);

    assert!(
        value
            .agents
            .agents_focus
            .as_ref()
            .is_some_and(|file| file.replace.is_none()),
        "esc: back to the menu"
    );

    key(&mut value, KeyCode::Esc, KeyModifiers::NONE);

    assert!(value.agents.agents_focus.is_none(), "esc: the prompt");
}

/// ctrl+p: per agent of the thread 3 rows (flow, remove, replace with), the agents box's actions.
#[test]
fn the_menu_has_the_agents_box_actions_per_agent() {
    let mut value = view("agentsbox-0004");

    value.open_menu();
    let mut thread = Terminal::new(TestBackend::new(110, 50)).unwrap();

    thread.draw(|file| value.draw(file)).unwrap();
    let bytes = thread.backend().buffer().clone();
    let text: String = (0..50)
        .map(|y| (0..110).map(|x| bytes[(x, y)].symbol().to_string()).collect::<String>() + "\n")
        .collect();

    assert!(
        text.contains("@zed: flow (0 flow)") && text.contains("@ada: remove"),
        "{text}"
    );

    // zed's flow row: its flow box on the flows
    value.menu_action(crate::screens::quick_menu::Action::Command(AGENT_ITEMS + 3));

    assert!(
        value.menu.is_none()
            && value
                .flowbars
                .iter()
                .find(|bytes| bytes.agent == "zed")
                .is_some_and(crate::features::flows::flow_bar::FlowBar::is_open)
    );

    // zed's remove row: the agents box on zed, remove armed (enter removes)
    value.menu_action(crate::screens::quick_menu::Action::Command(AGENT_ITEMS + 4));

    assert!(
        value
            .agents
            .agents_focus
            .as_ref()
            .is_some_and(|file| file.at == 1 && file.cur == 1 && file.armed)
    );

    key(&mut value, KeyCode::Enter, KeyModifiers::NONE);

    assert!(
        value
            .ops
            .borrow()
            .iter()
            .any(|output| output["op"] == "leave" && output["agent"] == "zed")
    );

    // ada's replace row: the replace picker
    value.menu_action(crate::screens::quick_menu::Action::Command(AGENT_ITEMS + 2));

    assert!(
        value
            .agents
            .agents_focus
            .as_ref()
            .is_some_and(|file| file.at == 0 && file.replace.is_some())
    );
}

/// reply on a gate of a non-lead agent: the message goes to that agent, the gate stays.
#[test]
fn reply_on_a_gate_goes_to_the_waiting_agent() {
    let mut value = view("agentsbox-0005");

    value.flowbars[1].state = Some(crate::engine::flows::run::FlowState {
        flow: "chore".into(),
        title: "chore".into(),
        wait: Some(crate::engine::flows::run::Wait::Gate),
        ..Default::default()
    });
    value.sync_flow_ask();
    value.prompt.input.set_text("also cover X");

    // chore's gate: approve (1), reply (2)
    key(&mut value, KeyCode::Tab, KeyModifiers::NONE);
    key(&mut value, KeyCode::Char('2'), KeyModifiers::NONE);
    let op = value
        .ops
        .borrow()
        .iter()
        .find(|output| output["op"] == "prompt")
        .cloned()
        .unwrap();

    assert_eq!(op["to"], json!(["zed"]), "{op}");
    assert!(!value.ops.borrow().iter().any(|output| output["op"] == "flow_answer"));
    assert!(value.agents.reply_to.is_none());

    // an @name in the reply: that agent too, the waiting agent first
    value.prompt.input.set_text("check with @ada first");
    key(&mut value, KeyCode::Tab, KeyModifiers::NONE);
    key(&mut value, KeyCode::Char('2'), KeyModifiers::NONE);
    let op = value
        .ops
        .borrow()
        .iter()
        .rev()
        .find(|output| output["op"] == "prompt")
        .cloned()
        .unwrap();

    assert_eq!(op["to"], json!(["zed", "ada"]), "{op}");
}

/// ctrl+f, then another view: the view gets the keys, 0 hidden search bar takes them.
#[test]
fn a_view_switch_takes_the_keys_from_the_search_bar() {
    let mut value = view("agentsbox-0006");

    key(&mut value, KeyCode::Char('f'), KeyModifiers::CONTROL);
    key(&mut value, KeyCode::Char('x'), KeyModifiers::NONE);
    key(&mut value, KeyCode::F(4), KeyModifiers::NONE);

    assert!(value.panes.pane == Pane::Files && !value.find.focus);

    key(&mut value, KeyCode::Char('j'), KeyModifiers::NONE);

    assert_eq!(value.find.ta.text(), "x", "j went to the files view");
}

/// The menu's agent rows keep their agent when the order changes before the pick.
#[test]
fn menu_agent_rows_keep_their_agent_after_a_join() {
    let mut value = view("agentsbox-0007");

    value.open_menu();

    // nova joins at the front while the menu is open
    value.agents.members.insert(0, "nova".into());
    value.agents.order.insert(0, "nova".into());
    value.menu_action(crate::screens::quick_menu::Action::Command(AGENT_ITEMS + 4));
    let fo = value.agents.agents_focus.clone().unwrap();

    assert_eq!(
        value.agents.order[fo.at], "zed",
        "row 2 of the menu: zed, not the new 2nd"
    );
}

#[test]
fn ctrl_m_gives_the_prompt_back_from_the_search_bar_and_another_view() {
    let mut value = view("agentsbox-0003");

    key(&mut value, KeyCode::Char('f'), KeyModifiers::CONTROL);

    assert!(value.find.focus);

    key(&mut value, KeyCode::Char('m'), KeyModifiers::CONTROL);

    assert!(
        !value.find.focus && value.find.open,
        "the bar stays, the prompt has the keys"
    );

    key(&mut value, KeyCode::Char('a'), KeyModifiers::NONE);

    assert_eq!(value.prompt.input.text(), "a");

    key(&mut value, KeyCode::F(4), KeyModifiers::NONE);

    assert!(!value.panes.prompt_focus);

    key(&mut value, KeyCode::Char('m'), KeyModifiers::CONTROL);

    assert!(value.panes.prompt_focus);
}
