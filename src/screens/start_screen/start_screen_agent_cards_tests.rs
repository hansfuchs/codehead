use super::*;

#[test]
fn task_reuses_agent_groups_with_selection_and_scroll() {
    let mut app = test_app();
    let ag = |name: &str, harness: &str| Agent {
        name: name.into(),
        harness: harness.into(),
        ..Default::default()
    };

    app.agent_list.agents = vec![
        ag("paimon", "codex"),
        ag("lilith", "claude"),
        ag("amon", "claude"),
        ag("bael", "claude"),
        ag("zagan", "amp"),
    ];
    app.compose.picks.clear();
    app.compose.pick_cursor = 4;
    app.compose.field = Field::Agents;

    // An agents-tab filter must not hide task choices.
    app.agent_list.afilter = Some("paimon".into());
    let mut terminal = Terminal::new(TestBackend::new(200, 48)).unwrap();

    terminal.draw(|frame| app.draw(frame)).unwrap();

    assert_eq!(
        app.compose.pick_places.iter().map(|place| place.0).collect::<Vec<_>>(),
        [4, 2, 3, 1, 0]
    );
    assert_eq!(app.compose.pick_places, app.agent_grid(198).places);

    let claude: Vec<_> = app
        .compose
        .pick_places
        .iter()
        .filter(|(i, _, _)| app.agent_list.agents[*i].harness == "claude")
        .collect();

    assert!(claude.iter().all(|place| place.2 == claude[0].2));

    key(&mut app, KeyCode::Char(' '), KeyModifiers::NONE);
    key(&mut app, KeyCode::Down, KeyModifiers::NONE);

    assert_eq!(app.agent_list.agents[app.compose.pick_cursor].name, "amon");

    key(&mut app, KeyCode::Right, KeyModifiers::NONE);
    key(&mut app, KeyCode::Right, KeyModifiers::NONE);

    assert_eq!(app.agent_list.agents[app.compose.pick_cursor].name, "lilith");

    key(&mut app, KeyCode::Char(' '), KeyModifiers::NONE);
    key(&mut app, KeyCode::Down, KeyModifiers::NONE);

    assert_eq!(app.agent_list.agents[app.compose.pick_cursor].name, "paimon");

    task_reuses_agent_groups_with_selection_and_scroll_verify_narrow(app, terminal);
}

fn task_reuses_agent_groups_with_selection_and_scroll_verify_narrow(mut app: App, mut terminal: Terminal<TestBackend>) {
    key(&mut app, KeyCode::Char(' '), KeyModifiers::NONE);

    assert_eq!(app.compose.picks, ["zagan", "lilith", "paimon"]);

    terminal.draw(|frame| app.draw(frame)).unwrap();

    assert!(app.compose.pick_offset > 0);
    assert!(
        app.preview
            .regions
            .menu_rows
            .iter()
            .any(|(_, i)| *i == app.compose.pick_cursor)
    );

    // Mouse selection uses the displayed card's agent, not its sorted slot.
    let (row, _) = *app.preview.regions.menu_rows.iter().find(|(_, i)| *i == 2).unwrap();

    app.mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: row.x + 1,
        row: row.y + 1,
        modifiers: KeyModifiers::NONE,
    });

    assert_eq!(app.compose.picks, ["zagan", "lilith", "paimon", "amon"]);

    app.compose.pick_cursor = 0;
    key(&mut app, KeyCode::Down, KeyModifiers::NONE);

    assert!(app.compose.field == Field::Notes);

    key(&mut app, KeyCode::Up, KeyModifiers::NONE);

    assert!(app.compose.field == Field::Agents);

    // A narrow terminal creates vertical card rows and retains the cursor.
    let mut narrow = Terminal::new(TestBackend::new(50, 30)).unwrap();

    narrow.draw(|frame| app.draw(frame)).unwrap();

    assert!(app.compose.pick_places.iter().all(|place| place.1 == 0));
    assert!(
        app.preview
            .regions
            .menu_rows
            .iter()
            .any(|(_, i)| *i == app.compose.pick_cursor)
    );

    key(&mut app, KeyCode::Up, KeyModifiers::NONE);

    assert_eq!(app.agent_list.agents[app.compose.pick_cursor].name, "lilith");

    app.compose.pick_cursor = 4;
    key(&mut app, KeyCode::Up, KeyModifiers::NONE);

    assert!(
        app.compose.field == Field::Opts || app.compose.field == Field::Repo,
        "↑ on the top row of cards: the field above"
    );

    app.agent_list.agents.clear();
    narrow.draw(|frame| app.draw(frame)).unwrap();

    assert!(app.compose.pick_places.is_empty() && app.preview.regions.menu_rows.is_empty());
}

#[test]
fn agents_tab_groups_cards_by_harness_filters_and_moves_in_the_grid() {
    let app = test_app();

    // 1 draw, 0 tick: the machine's agents never replace the fixture
    let once = |app: &mut App, width: u16, height: u16| {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();

        terminal.draw(|frame| app.draw(frame)).unwrap();
        let buffer = terminal.backend().buffer().clone();

        (0..height)
            .map(|y| {
                (0..width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect::<Vec<_>>()
            .join("\n")
    };

    agents_tab_groups_cards_by_harness_filters_and_moves_in_the_grid_verify_order(app, once);
}

fn agents_tab_groups_cards_by_harness_filters_and_moves_in_the_grid_verify_order(
    mut app: App,
    once: impl Fn(&mut App, u16, u16) -> String,
) {
    let ag = |name: &str, harness: &str| Agent {
        name: name.into(),
        harness: harness.into(),
        ..Default::default()
    };

    app.agent_list.agents = vec![
        ag("paimon", "codex"),
        ag("lilith", "claude"),
        ag("amon", "claude"),
        ag("bael", "claude"),
        ag("zagan", "amp"),
    ];
    app.switch_tab(Tab::Agents);

    // 0 read of the machine's agents: the fixture stays
    app.agent_list.agent_reads = Reads::default();

    let order: Vec<(String, Vec<String>)> = app
        .avisible()
        .into_iter()
        .map(|(harness, group)| {
            (
                harness,
                group
                    .into_iter()
                    .map(|i| app.agent_list.agents[i].name.clone())
                    .collect(),
            )
        })
        .collect();

    assert_eq!(
        order,
        vec![
            ("amp".to_string(), vec!["zagan".to_string()]),
            ("claude".into(), vec!["amon".into(), "bael".into(), "lilith".into()]),
            ("codex".into(), vec!["paimon".into()]),
        ]
    );

    let screen = once(&mut app, 200, 40);

    println!("--- agents\n{screen}");

    for head in ["amp · 1", "claude · 3", "codex · 1"] {
        assert!(screen.contains(head), "{head}\n{screen}");
    }

    // a wide list: the claude group takes 1 row of 3 cards
    let claude: Vec<_> = app
        .agent_list
        .aplace
        .iter()
        .filter(|(i, _, _)| app.agent_list.agents[*i].harness == "claude")
        .collect();

    assert!(claude.iter().all(|(_, _, y)| *y == claude[0].2), "{claude:?}");
    agents_tab_groups_cards_by_harness_filters_and_moves_in_the_grid_verify_name(app, once);
}

fn agents_tab_groups_cards_by_harness_filters_and_moves_in_the_grid_verify_name(
    mut app: App,
    once: impl Fn(&mut App, u16, u16) -> String,
) {
    let name = |app: &App| app.agent_list.agents[app.agent_list.acursor].name.clone();

    app.agent_list.acursor = 4;

    // ↓ zagan → the claude row (nearest x), → → along the row, ↓ → paimon, ↑↑↑ → tabs
    key(&mut app, KeyCode::Down, KeyModifiers::NONE);

    assert_eq!(name(&app), "amon");

    key(&mut app, KeyCode::Right, KeyModifiers::NONE);
    key(&mut app, KeyCode::Right, KeyModifiers::NONE);

    assert_eq!(name(&app), "lilith");

    key(&mut app, KeyCode::Down, KeyModifiers::NONE);

    assert_eq!(name(&app), "paimon");

    for _ in 0..2 {
        key(&mut app, KeyCode::Up, KeyModifiers::NONE);
    }

    assert_eq!(name(&app), "zagan");

    key(&mut app, KeyCode::Up, KeyModifiers::NONE);

    assert!(app.tabs_focus);

    // narrow: 1 card per row
    app.tabs_focus = false;
    let _ = once(&mut app, 60, 60);
    let claude: Vec<_> = app
        .agent_list
        .aplace
        .iter()
        .filter(|(i, _, _)| app.agent_list.agents[*i].harness == "claude")
        .map(|place| place.1)
        .collect();

    assert!(claude.iter().all(|&x| x == 0), "{claude:?}");

    // the filter: f, type, enter; esc clears it
    key(&mut app, KeyCode::Char('f'), KeyModifiers::CONTROL);

    assert!(app.agent_list.afilter_editing);

    app.paste("lil");
    key(&mut app, KeyCode::Enter, KeyModifiers::NONE);

    assert!(!app.agent_list.afilter_editing);
    assert_eq!(app.avisible().iter().map(|(_, group)| group.len()).sum::<usize>(), 1);
    assert_eq!(name(&app), "lilith");

    let screen = once(&mut app, 120, 30);

    assert!(screen.contains("filter › lil") && screen.contains("1 of 5"), "{screen}");

    key(&mut app, KeyCode::Esc, KeyModifiers::NONE);

    assert!(app.agent_list.afilter.is_none() && app.outcome.is_none());
}

#[test]
fn hover_paints_clickable_items() {
    let mut app = test_app();
    let mut terminal = Terminal::new(TestBackend::new(100, 20)).unwrap();

    terminal.draw(|frame| app.draw(frame)).unwrap();
    let bg = |terminal: &Terminal<TestBackend>, x: u16, y: u16| terminal.backend().buffer()[(x, y)].bg;
    let moved = |x: u16, y: u16| MouseEvent {
        kind: MouseEventKind::Moved,
        column: x,
        row: y,
        modifiers: KeyModifiers::NONE,
    };

    // tab "threads" in the header
    let tab = app
        .preview
        .regions
        .tabs
        .iter()
        .find(|(_, tab)| *tab == Tab::Threads)
        .unwrap()
        .0;

    app.mouse(moved(tab.x + 1, tab.y));
    terminal.draw(|frame| app.draw(frame)).unwrap();

    assert_eq!(bg(&terminal, tab.x + 1, tab.y), theme::pal().surface1);

    // the repo row of the task view
    let repo_row = app.preview.regions.repo_row;

    app.mouse(moved(repo_row.x + 2, repo_row.y));
    terminal.draw(|frame| app.draw(frame)).unwrap();

    assert_eq!(bg(&terminal, repo_row.x + 2, repo_row.y), theme::pal().surface1);
    assert_ne!(
        bg(&terminal, tab.x + 1, tab.y),
        theme::pal().surface1,
        "1 item at a time"
    );

    // agents tab: a card takes the hover, the free room right of it and its heading do not
    app.switch_tab(Tab::Agents);
    app.agent_list.agents.truncate(1);
    terminal.draw(|frame| app.draw(frame)).unwrap();
    let card = app.agent_list.acards[0].0;

    app.mouse(moved(card.right() + 1, card.y + 1));
    terminal.draw(|frame| app.draw(frame)).unwrap();

    assert_ne!(bg(&terminal, card.right() + 1, card.y + 1), theme::pal().surface1);

    app.mouse(moved(card.x + 1, card.y - 1));
    terminal.draw(|frame| app.draw(frame)).unwrap();

    assert_ne!(bg(&terminal, card.x + 1, card.y - 1), theme::pal().surface1);

    app.mouse(moved(card.x + 2, card.y + 1));
    terminal.draw(|frame| app.draw(frame)).unwrap();

    assert_eq!(bg(&terminal, card.x + 2, card.y + 1), theme::pal().surface1);
}

#[test]
fn undo_redo_in_all_fields() {
    let mut app = test_app();

    app.compose.field = Field::Title;
    undo_redo(&mut app, |app| app.compose.title.text());
    app.compose.field = Field::Prompt;
    undo_redo(&mut app, |app| app.compose.prompt.text());
    app.run(Act::NewAgent);

    for field in [F_NAME, F_HARNESS, F_VOICE, F_PROMPT] {
        if let Some(Modal::Agent(form)) = &mut app.overlays.modal {
            form.field = field;
        }

        undo_redo(&mut app, |app| match &app.overlays.modal {
            Some(Modal::Agent(form)) => {
                [&form.name, &form.harness, &form.voice, &form.prompt][[F_NAME, F_HARNESS, F_VOICE, F_PROMPT]
                    .iter()
                    .position(|x| *x == form.field)
                    .unwrap()]
                .text()
            }
            _ => panic!("agent form closed"),
        });
    }

    app.overlays.modal = None;
    app.thread_list.worktrees = Some(Vec::new());
    app.switch_tab(Tab::Threads);
    key(&mut app, KeyCode::Char('f'), KeyModifiers::CONTROL);
    undo_redo(&mut app, |app| app.thread_list.tlist.query());
    app.switch_tab(Tab::Notes);
    key(&mut app, KeyCode::Char('f'), KeyModifiers::CONTROL);
    undo_redo(&mut app, |app| app.note_list.filter.clone().unwrap_or_default());

    assert_eq!(app.note_list.rows.len(), 0, "filter \"hi\" matches 0 notes");

    key(&mut app, KeyCode::Char('z'), KeyModifiers::CONTROL);

    assert!(!app.note_list.rows.is_empty(), "undo rebuilds the list");
}

#[test]
fn agent_form_enter_opens_the_avatar_picker() {
    let mut app = test_app();

    app.run(Act::NewAgent);
    let form = |app: &App| match &app.overlays.modal {
        Some(Modal::Agent(agent_form)) => (agent_form.picker.is_some(), agent_form.sprite.avatar),
        _ => panic!("agent form closed"),
    };

    if let Some(Modal::Agent(agent_form)) = &mut app.overlays.modal {
        agent_form.field = F_SPRITE;
        agent_form.sprite = sprites::Sprite::of(crate::domain::avatar::Avatar::default());
    }

    key(&mut app, KeyCode::Enter, KeyModifiers::NONE);

    assert!(form(&app).0, "enter: the picker");

    dump(&mut app, 120, 42);

    // tab: geo; → : the 2nd; f: half speed; enter: taken
    key(&mut app, KeyCode::Tab, KeyModifiers::NONE);
    key(&mut app, KeyCode::Right, KeyModifiers::NONE);
    key(&mut app, KeyCode::Char('f'), KeyModifiers::NONE);
    key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    let (open, av) = form(&app);

    assert!(!open);
    assert_eq!((av.cat, av.idx, av.speed), (crate::domain::avatar::Cat::Geo, 1, 0.5));

    // esc in the picker: the avatar stays
    key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    key(&mut app, KeyCode::Right, KeyModifiers::NONE);
    key(&mut app, KeyCode::Esc, KeyModifiers::NONE);

    assert_eq!(form(&app), (false, av));

    for _ in 0..10 {
        key(&mut app, KeyCode::Char('r'), KeyModifiers::NONE);

        assert_eq!(
            form(&app).1.cat,
            crate::domain::avatar::Cat::Geo,
            "r: random in the category"
        );
    }
}

#[test]
fn agent_form_arrows_leave_text_boxes() {
    let mut app = test_app();

    app.run(Act::NewAgent);
    let field = |app: &App| match &app.overlays.modal {
        Some(Modal::Agent(agent_form)) => agent_form.field,
        _ => usize::MAX,
    };

    if let Some(Modal::Agent(agent_form)) = &mut app.overlays.modal {
        agent_form.field = F_VOICE;
        agent_form.voice.set_text("line one\nline two");
    }

    dump(&mut app, 120, 42);

    // cursor on the last line: ↑ moves inside first, then leaves to the sprite
    key(&mut app, KeyCode::Up, KeyModifiers::NONE);

    assert_eq!(field(&app), F_VOICE);

    key(&mut app, KeyCode::Up, KeyModifiers::NONE);

    assert_eq!(field(&app), F_SPRITE);

    key(&mut app, KeyCode::Down, KeyModifiers::NONE);

    assert_eq!(field(&app), F_VOICE);

    // ↓ inside the box first, then out to the system prompt
    key(&mut app, KeyCode::Down, KeyModifiers::NONE);

    assert_eq!(field(&app), F_VOICE);

    key(&mut app, KeyCode::Down, KeyModifiers::NONE);

    assert_eq!(field(&app), F_PROMPT);

    key(&mut app, KeyCode::Down, KeyModifiers::NONE);

    assert_eq!(field(&app), F_BUTTONS, "empty system prompt: 1 row, ↓ leaves");
}
