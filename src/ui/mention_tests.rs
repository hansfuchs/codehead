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

fn key(mention: &mut Mention, ta: &mut TextArea, agents: &[Agent], code: KeyCode) -> bool {
    mention.key(KeyEvent::new(code, KeyModifiers::NONE), ta, agents)
}

fn screen(mention: &mut Mention, ta: &mut TextArea, agents: &[Agent]) -> String {
    let mut term = ratatui::Terminal::new(ratatui::backend::TestBackend::new(70, 30)).unwrap();

    term.draw(|frame| {
        ta.render(frame, Rect::new(2, 28, 60, 1), true);
        mention.render(frame, ta, agents, frame.area());
    })
    .unwrap();

    let buf = term.backend().buffer().clone();
    let text = (0..30)
        .map(|y| (0..70).map(|x| buf[(x, y)].symbol().to_string()).collect::<String>())
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
    let mut mention = Mention::default();

    assert!(mention.is_open(&ta, &agents));

    let shown = screen(&mut mention, &mut ta, &agents);

    assert!(
        shown.contains("claude · 2")
            && shown.contains("codex · 1")
            && shown.contains("amon")
            && shown.contains("claude"),
        "{shown}"
    );

    // claude: amon, ramona; codex: nova
    assert!(key(&mut mention, &mut ta, &agents, KeyCode::Down));
    assert!(key(&mut mention, &mut ta, &agents, KeyCode::Down));
    assert!(key(&mut mention, &mut ta, &agents, KeyCode::Tab));
    assert_eq!(ta.text(), "ask @nova ");
    assert_eq!(mention.picked.take(), Some(("nova".into(), None)));
    assert!(!mention.is_open(&ta, &agents));

    // ↓ previews `@ramona` in the text: a draw keeps every card, enter picks ramona
    ta.set_text("ask @");
    ta.move_end();

    assert!(key(&mut mention, &mut ta, &agents, KeyCode::Down));

    let shown = screen(&mut mention, &mut ta, &agents);

    assert!(shown.contains("nova") && shown.contains("amon"), "{shown}");
    assert!(key(&mut mention, &mut ta, &agents, KeyCode::Enter));
    assert_eq!(ta.text(), "ask @ramona ");

    // 0 matches: closed, keys pass
    ta.insert("@zz");

    assert!(!key(&mut mention, &mut ta, &agents, KeyCode::Enter));
}

#[test]
fn esc_closes_until_the_at_is_gone() {
    let agents = [agent("amon", "claude")];
    let mut ta = TextArea::new(false, "");
    let mut mention = Mention::default();

    ta.set_text("@");
    ta.move_end();

    assert!(key(&mut mention, &mut ta, &agents, KeyCode::Esc));
    assert!(!mention.is_open(&ta, &agents));

    // ⌫ deletes the `@`, a new `@` at the same place opens again
    assert!(!key(&mut mention, &mut ta, &agents, KeyCode::Backspace));

    ta.key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));

    assert!(!key(&mut mention, &mut ta, &agents, KeyCode::Char('@')));

    ta.insert("@");

    assert!(mention.is_open(&ta, &agents));
    assert!(screen(&mut mention, &mut ta, &agents).contains("amon"));
}

#[test]
fn right_opens_replace_enter_picks_left_goes_back() {
    let agents = [
        agent("amon", "claude"),
        agent("lilith", "claude"),
        agent("nova", "codex"),
    ];
    let mut ta = TextArea::new(false, "");
    let mut mention = Mention {
        replaceable: vec!["amon".into(), "lilith".into()],
        ..Default::default()
    };

    ta.set_text("x @no");
    ta.move_end();

    assert!(key(&mut mention, &mut ta, &agents, KeyCode::Right));

    let shown = screen(&mut mention, &mut ta, &agents);

    assert!(
        shown.contains("nova replaces")
            && shown.contains("@amon")
            && shown.contains("@lilith")
            && shown.contains("replace"),
        "{shown}"
    );

    // ← back to add, → again, ↓ lilith, enter
    assert!(key(&mut mention, &mut ta, &agents, KeyCode::Left));
    assert!(!screen(&mut mention, &mut ta, &agents).contains("nova replaces"));
    assert!(key(&mut mention, &mut ta, &agents, KeyCode::Right));
    assert!(key(&mut mention, &mut ta, &agents, KeyCode::Down));
    assert!(key(&mut mention, &mut ta, &agents, KeyCode::Enter));
    assert_eq!(ta.text(), "x @nova (replaces @lilith) ");
    assert_eq!(mention.picked.take(), Some(("nova".into(), Some("lilith".into()))));
    assert!(!mention.is_open(&ta, &agents));

    // amon can replace only lilith; the only agent of a thread: add only, ←→ pass
    mention.replaceable = vec!["amon".into()];
    ta.set_text("x @am");
    ta.move_end();

    assert!(!key(&mut mention, &mut ta, &agents, KeyCode::Right));
}

#[test]
fn replace_notes_strip() {
    assert_eq!(strip_replaces("@nova (replaces @amon) go on"), "@nova go on");
    assert_eq!(strip_replaces("a (replaces @x y) b"), "a (replaces @x y) b");
    assert_eq!(strip_replaces("plain"), "plain");
}
