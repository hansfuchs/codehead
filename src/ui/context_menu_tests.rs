use super::*;

fn key_event(code: KeyCode, message: KeyModifiers) -> KeyEvent {
    KeyEvent::new(code, message)
}

#[test]
fn the_key_is_ctrl_dot_shift_f10_or_the_menu_key() {
    assert!(is_key(&key_event(KeyCode::Char('.'), KeyModifiers::CONTROL)));
    assert!(is_key(&key_event(KeyCode::Char('.'), KeyModifiers::SUPER)));
    assert!(is_key(&key_event(KeyCode::F(10), KeyModifiers::SHIFT)));
    assert!(is_key(&key_event(KeyCode::Menu, KeyModifiers::NONE)));
    assert!(!is_key(&key_event(KeyCode::Char('.'), KeyModifiers::NONE)));
    assert!(!is_key(&key_event(KeyCode::F(10), KeyModifiers::NONE)));
}

#[test]
fn enter_a_digit_or_a_hint_key_runs_a_row() {
    let items = vec![
        Item::new("open in the IDE", "e", 1),
        Item::new("delete", "d d", 2),
        Item::new("resolve", "", 3),
    ];
    let mut message = Menu::new("file", items).expect("items");

    assert_eq!(message.key(key_event(KeyCode::Down, KeyModifiers::NONE)), Out::None);
    assert_eq!(message.key(key_event(KeyCode::Enter, KeyModifiers::NONE)), Out::Run(2));
    assert_eq!(
        message.key(key_event(KeyCode::Char('3'), KeyModifiers::NONE)),
        Out::Run(3)
    );
    assert_eq!(
        message.key(key_event(KeyCode::Char('e'), KeyModifiers::NONE)),
        Out::Run(1)
    );
    assert_eq!(
        message.key(key_event(KeyCode::Char('d'), KeyModifiers::NONE)),
        Out::Run(2)
    );
    assert_eq!(
        message.key(key_event(KeyCode::Char('z'), KeyModifiers::NONE)),
        Out::None
    );
    assert_eq!(message.key(key_event(KeyCode::Esc, KeyModifiers::NONE)), Out::Close);
    assert_eq!(
        message.key(key_event(KeyCode::Char('.'), KeyModifiers::CONTROL)),
        Out::Close
    );
    assert!(Menu::<u8>::new("x", Vec::new()).is_none());
}

#[test]
fn an_outward_row_asks_a_2nd_enter_a_grayed_one_never_runs() {
    let mut push = Item::new("push", "", 1).in_section("ship");

    push.asks = true;
    let mut off = Item::new("merge", "", 2).in_section("ship");

    off.off = true;
    let mut message = Menu::new("panel", vec![Item::new("commit message", "m", 0), push, off]).expect("items");
    let enter = key_event(KeyCode::Enter, KeyModifiers::NONE);

    message.key(key_event(KeyCode::Down, KeyModifiers::NONE));

    assert_eq!(message.key(enter), Out::None, "1st enter: armed");
    assert_eq!(message.key(enter), Out::Run(1));

    message.key(key_event(KeyCode::Down, KeyModifiers::NONE));

    assert_eq!(message.key(enter), Out::None, "grayed");

    // a move between disarms
    message.key(key_event(KeyCode::Up, KeyModifiers::NONE));
    message.key(enter);
    message.key(key_event(KeyCode::Down, KeyModifiers::NONE));
    message.key(key_event(KeyCode::Up, KeyModifiers::NONE));

    assert_eq!(message.key(enter), Out::None);
    assert_eq!(
        message.lines(),
        vec![Some(0), None, Some(1), Some(2)],
        "a rule before the ship section"
    );
}

#[test]
fn beside_opens_right_of_the_panel_at_opens_above_its_row() {
    let mut message = Menu::new("x", vec![Item::new("a", "", 0), Item::new("b", "", 1)]).expect("items");
    let mut thread = ratatui::Terminal::new(ratatui::backend::TestBackend::new(120, 30)).unwrap();
    let panel = Rect {
        x: 0,
        y: 2,
        width: 30,
        height: 26,
    };

    thread
        .draw(|file| message.render(file, Place::Beside(panel, 10), file.area()))
        .unwrap();

    assert_eq!((message.area.x, message.area.y), (30, 10));

    let row = Rect {
        x: 40,
        y: 20,
        width: 60,
        height: 1,
    };

    thread
        .draw(|file_value| message.render(file_value, Place::At(row), file_value.area()))
        .unwrap();

    assert!(
        message.area.bottom() <= 20 && message.area.x == 40,
        "{:?}",
        message.area
    );

    let row = Rect {
        x: 40,
        y: 1,
        width: 60,
        height: 1,
    };

    thread
        .draw(|file_2| message.render(file_2, Place::At(row), file_2.area()))
        .unwrap();

    assert!(message.area.y >= 2, "0 room above: below the row {:?}", message.area);
}
