use super::*;

fn agent(name: &str, harness: &str) -> Agent {
    Agent {
        name: name.into(),
        harness: harness.into(),
        model: None,
        color: None,
        mode: String::new(),
        prompt: None,
        last_seen: None,
    }
}

fn key(message: &mut Mention, ta: &mut TextArea, agents: &[Agent], character: KeyCode) -> bool {
    message.key(KeyEvent::new(character, KeyModifiers::NONE), ta, agents)
}

fn screen(message: &mut Mention, ta: &mut TextArea, agents: &[Agent]) -> String {
    let mut thread = ratatui::Terminal::new(ratatui::backend::TestBackend::new(70, 30)).unwrap();

    thread
        .draw(|area_value| {
            ta.render(area_value, Rect::new(2, 28, 60, 1), true);
            message.render(area_value, ta, agents, area_value.area());
        })
        .unwrap();

    let bytes = thread.backend().buffer().clone();
    let text = (0..30)
        .map(|y| (0..70).map(|x| bytes[(x, y)].symbol().to_string()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n");

    if std::env::var_os("SHOW").is_some() {
        eprintln!("{text}");
    }

    text
}

#[test]
fn cards_grouped_by_harness_filter_and_insert() {
    let agents = [
        agent("nova", "codex"),
        agent("amon", "claude"),
        agent("ramona", "claude"),
    ];

    assert_eq!(order(&visible(&agents, "mo")), [1, 2]);
    assert_eq!(order(&visible(&agents, "n")), [0]);
    assert_eq!(order(&visible(&agents, "")), [1, 2, 0]);

    let mut ta = TextArea::new(false, "");

    ta.set_text("ask @");
    ta.move_end();
    let mut message = Mention::default();

    assert!(message.is_open(&ta, &agents));

    let text_value = screen(&mut message, &mut ta, &agents);

    assert!(
        text_value.contains("claude · 2")
            && text_value.contains("codex · 1")
            && text_value.contains("amon")
            && text_value.contains("claude"),
        "{text_value}"
    );

    // claude: amon, ramona; codex: nova
    assert!(key(&mut message, &mut ta, &agents, KeyCode::Down));
    assert!(key(&mut message, &mut ta, &agents, KeyCode::Down));
    assert!(key(&mut message, &mut ta, &agents, KeyCode::Tab));
    assert_eq!(ta.text(), "ask @nova ");
    assert_eq!(message.picked.take(), Some(("nova".into(), None)));
    assert!(!message.is_open(&ta, &agents));

    // ↓ previews `@ramona` in the text: a draw keeps every card, enter picks ramona
    ta.set_text("ask @");
    ta.move_end();

    assert!(key(&mut message, &mut ta, &agents, KeyCode::Down));

    let text_2 = screen(&mut message, &mut ta, &agents);

    assert!(text_2.contains("nova") && text_2.contains("amon"), "{text_2}");
    assert!(key(&mut message, &mut ta, &agents, KeyCode::Enter));
    assert_eq!(ta.text(), "ask @ramona ");

    // 0 matches: closed, keys pass
    ta.insert("@zz");

    assert!(!key(&mut message, &mut ta, &agents, KeyCode::Enter));
}

#[test]
fn esc_closes_until_the_at_is_gone() {
    let agents = [agent("amon", "claude")];
    let mut ta = TextArea::new(false, "");
    let mut message = Mention::default();

    ta.set_text("@");
    ta.move_end();

    assert!(key(&mut message, &mut ta, &agents, KeyCode::Esc));
    assert!(!message.is_open(&ta, &agents));

    // ⌫ deletes the `@`, a new `@` at the same place opens again
    assert!(!key(&mut message, &mut ta, &agents, KeyCode::Backspace));

    ta.key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));

    assert!(!key(&mut message, &mut ta, &agents, KeyCode::Char('@')));

    ta.insert("@");

    assert!(message.is_open(&ta, &agents));
    assert!(screen(&mut message, &mut ta, &agents).contains("amon"));
}

#[test]
fn right_opens_replace_enter_picks_left_goes_back() {
    let agents = [
        agent("amon", "claude"),
        agent("lilith", "claude"),
        agent("nova", "codex"),
    ];
    let mut ta = TextArea::new(false, "");
    let mut message = Mention {
        replaceable: vec!["amon".into(), "lilith".into()],
        ..Default::default()
    };

    ta.set_text("x @no");
    ta.move_end();

    assert!(key(&mut message, &mut ta, &agents, KeyCode::Right));

    let text_value = screen(&mut message, &mut ta, &agents);

    assert!(
        text_value.contains("nova replaces")
            && text_value.contains("@amon")
            && text_value.contains("@lilith")
            && text_value.contains("replace"),
        "{text_value}"
    );

    // ← back to add, → again, ↓ lilith, enter
    assert!(key(&mut message, &mut ta, &agents, KeyCode::Left));
    assert!(!screen(&mut message, &mut ta, &agents).contains("nova replaces"));
    assert!(key(&mut message, &mut ta, &agents, KeyCode::Right));
    assert!(key(&mut message, &mut ta, &agents, KeyCode::Down));
    assert!(key(&mut message, &mut ta, &agents, KeyCode::Enter));
    assert_eq!(ta.text(), "x @nova (replaces @lilith) ");
    assert_eq!(message.picked.take(), Some(("nova".into(), Some("lilith".into()))));
    assert!(!message.is_open(&ta, &agents));

    // amon can replace only lilith; the only agent of a thread: add only, ←→ pass
    message.replaceable = vec!["amon".into()];
    ta.set_text("x @am");
    ta.move_end();

    assert!(!key(&mut message, &mut ta, &agents, KeyCode::Right));
}

#[test]
fn replace_notes_strip() {
    assert_eq!(strip_replaces("@nova (replaces @amon) go on"), "@nova go on");
    assert_eq!(strip_replaces("a (replaces @x y) b"), "a (replaces @x y) b");
    assert_eq!(strip_replaces("plain"), "plain");
}
