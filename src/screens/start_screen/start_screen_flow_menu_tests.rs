use super::start_screen_tests::app_with;
use super::*;
use ratatui::backend::TestBackend;

fn press(app: &mut App, code: KeyCode) {
    app.key(KeyEvent::new(code, KeyModifiers::NONE));
}

fn screen(app: &mut App) -> String {
    let mut terminal = ratatui::Terminal::new(TestBackend::new(140, 50)).unwrap();

    terminal.draw(|frame| app.draw(frame)).unwrap();
    let buffer = terminal.backend().buffer().clone();

    (0..50)
        .map(|y| {
            (0..140)
                .map(|x| buffer[(x, y)].symbol().to_string())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// `f` on an agent card: that agent's flows, a flow its steps (the start step); the card shows
/// the flow under its effort row; each agent keeps its own flow.
#[test]
fn the_context_menu_on_a_card_opens_its_flows_and_a_flow_its_steps() {
    let mut app = app_with(None);

    app.flow_selection.flow_lib = crate::engine::flows::Library::load(None);
    app.agent_list.agents.push(Agent {
        name: "zed".into(),
        harness: "claude".into(),
        ..Default::default()
    });
    app.flow_selection.flow_default = None;

    assert!(app.flow_selection.flow_lib.flows.len() > 1 && app.agent_list.agents.len() > 1);

    app.compose.screen = Screen::Compose;
    app.tabs_focus = false;
    app.compose.field = Field::Agents;
    let _ = screen(&mut app);

    app.compose.pick_cursor = 1;
    the_context_menu_on_a_card_opens_its_flows_and_a_flow_its_steps_verify_steps(app);
}

fn the_context_menu_on_a_card_opens_its_flows_and_a_flow_its_steps_verify_steps(mut app: App) {
    let (agent, other) = (
        app.agent_list.agents[1].name.clone(),
        app.agent_list.agents[0].name.clone(),
    );

    // ctrl+. → flow of this agent: the flows of the card under the cursor (f: 0 key)
    press(&mut app, KeyCode::Char('f'));

    assert!(app.flow_selection.flow_menu.is_none());

    app.key(KeyEvent::new(KeyCode::Char('.'), KeyModifiers::CONTROL));
    press(&mut app, KeyCode::Enter);

    assert!(
        app.flow_selection
            .flow_menu
            .as_ref()
            .is_some_and(|menu| menu.sub.is_none() && menu.cursor == 0 && menu.agent == agent)
    );

    let text = screen(&mut app);
    let first = app.flow_selection.flow_lib.flows[0].name.clone();

    assert!(
        text.contains(&format!("flow of {agent}")) && text.contains(&format!("◆ {first}")) && text.contains("0 flow"),
        "{text}"
    );

    // ↓ to the 1st flow: its preview beside the flows at once
    press(&mut app, KeyCode::Down);
    let steps = app.flow_steps(1);
    let preview = screen(&mut app);

    assert!(
        preview.contains("preview") && preview.contains(&format!("{first} · {} steps", steps.len())),
        "{preview}"
    );

    // enter: the rail takes the keys; ← back, → in, ↓ enter: flow 1 from step 2
    press(&mut app, KeyCode::Enter);

    assert_eq!(
        app.flow_selection
            .flow_menu
            .as_ref()
            .and_then(|menu| menu.sub)
            .map(|sub| sub.0),
        Some(0)
    );

    press(&mut app, KeyCode::Left);

    assert!(
        app.flow_selection
            .flow_menu
            .as_ref()
            .is_some_and(|menu| menu.sub.is_none())
    );

    press(&mut app, KeyCode::Right);
    press(&mut app, KeyCode::Down);
    press(&mut app, KeyCode::Enter);

    assert!(app.flow_selection.flow_menu.is_none());
    the_context_menu_on_a_card_opens_its_flows_and_a_flow_its_steps_verify_cards(app, &agent, &other, &first);
}

fn the_context_menu_on_a_card_opens_its_flows_and_a_flow_its_steps_verify_cards(
    mut app: App,
    agent: &str,
    other: &str,
    first: &str,
) {
    assert_eq!(app.flow_at(agent), (1, 1));
    assert!(
        app.compose.picks.iter().any(|pick| pick == agent),
        "a flow picks the agent"
    );
    assert_eq!(app.flow_at(other), (0, 0), "the other card keeps 0 flow");

    let cards = screen(&mut app);

    println!("--- cards with flows\n{cards}");

    assert!(
        cards.contains(&format!("◆ {first} › 2")) && cards.contains("0 flow"),
        "{cards}"
    );

    // esc in the dropdown closes it only
    app.key(KeyEvent::new(KeyCode::Char('.'), KeyModifiers::CONTROL));
    press(&mut app, KeyCode::Enter);
    press(&mut app, KeyCode::Esc);

    assert!(app.flow_selection.flow_menu.is_none() && app.outcome.is_none());

    // a click on a flow opens its steps, a click on a step picks it
    app.key(KeyEvent::new(KeyCode::Char('.'), KeyModifiers::CONTROL));
    press(&mut app, KeyCode::Enter);
    let _ = screen(&mut app);
    let (row, _) = *app.preview.regions.flow_rows.iter().find(|(_, i)| *i == 2).unwrap();

    app.mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: row.x + 2,
        row: row.y,
        modifiers: KeyModifiers::NONE,
    });

    assert!(
        app.flow_selection
            .flow_menu
            .as_ref()
            .is_some_and(|menu| menu.cursor == 2 && menu.sub.is_some())
    );

    let _ = screen(&mut app);
    let (result, _) = *app.preview.regions.flow_steps.iter().find(|(_, i)| *i == 0).unwrap();

    app.mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: result.x + 2,
        row: result.y,
        modifiers: KeyModifiers::NONE,
    });

    assert_eq!(
        (app.flow_at(agent), app.flow_selection.flow_menu.is_none()),
        ((2, 0), true)
    );

    // 0: 0 flow at once
    app.key(KeyEvent::new(KeyCode::Char('.'), KeyModifiers::CONTROL));
    press(&mut app, KeyCode::Enter);
    press(&mut app, KeyCode::Char('0'));

    assert_eq!(app.flow_at(agent), (0, 0));
}

/// The lead's default: the repo's last flow, until a pick; other agents: 0 flow.
#[test]
fn the_repos_last_flow_is_the_leads_default_only() {
    let mut app = app_with(None);

    app.flow_selection.flow_lib = crate::engine::flows::Library::load(None);
    app.agent_list.agents.push(Agent {
        name: "zed".into(),
        harness: "claude".into(),
        ..Default::default()
    });
    let slug = app.flow_selection.flow_lib.flows[0].slug.clone();

    app.flow_selection.flow_default = Some(slug.clone());
    app.compose.picks = vec![
        app.agent_list.agents[1].name.clone(),
        app.agent_list.agents[0].name.clone(),
    ];
    let (lead, other) = (
        app.agent_list.agents[0].name.clone(),
        app.agent_list.agents[1].name.clone(),
    );

    assert_eq!(
        app.task_lead().as_deref(),
        Some(lead.as_str()),
        "the 1st pick in the cards' order"
    );
    assert_eq!(app.flow_of(&lead).0, slug);
    assert_eq!(app.flow_of(&other).0, "");

    app.flow_selection.agent_flows.insert(lead.clone(), (String::new(), 0));

    assert!(app.flow_of(&lead).0.is_empty(), "0 flow picked: 0 default");
}
