use super::*;

#[test]
fn e_opens_the_line_under_the_cursor() {
    let mut value = view(parse(DIFF));

    value.build(100);
    let i = (0..value.layout.rows.len())
        .find(|&i| text_of(&value, i).contains("let c = 4;"))
        .unwrap();

    value.at = i;

    assert_eq!(
        value.key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE)),
        Act::Open("src/a.rs".into(), 12)
    );
    assert_eq!(
        value.key(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::NONE)),
        Act::None,
        "o, i: 0 IDE (e on every screen)"
    );

    // a removed line: the worktree line it sat before (its old number: 11 as well here)
    let del = (0..value.layout.rows.len())
        .find(|&i| text_of(&value, i).contains("- let b = 2;"))
        .unwrap();

    value.at = del;

    assert_eq!(
        value.key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE)),
        Act::Open("src/a.rs".into(), 11)
    );

    // the selection wins: its 1st line, also with the cursor in an other file
    select(&mut value, "+ let c = 4;");
    select(&mut value, "+ fn t() {}");
    value.at = (0..value.layout.rows.len())
        .find(|&i| text_of(&value, i).contains("+ fn t() {}"))
        .unwrap();

    assert_eq!(
        value.key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE)),
        Act::Open("tests/a_test.rs".into(), 2)
    );

    value.at = value.layout.rows.iter().position(|result| result.head).unwrap();

    assert_eq!(
        value.key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE)),
        Act::Open("src/a.rs".into(), 12)
    );

    value.selection_state.sel.clear();

    // j: the next box head
    value.at = value
        .layout
        .rows
        .iter()
        .position(|result_value| result_value.head)
        .unwrap();
    value.key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));

    assert!(value.layout.rows[value.at].head && value.changes.files[value.layout.rows[value.at].file].path == "new.rs");
}

#[test]
fn arrows_go_1_line_j_k_and_ctrl_arrows_1_file_the_box_border_lights_up() {
    let mut value = view(parse(DIFF));
    let text = screen(&mut value, 100, 30);

    // the cursor (▌ at the right edge) on the 1st box's head
    let head = row_of(&text, "☐ src/a.rs");
    let next = row_of(&text, "☐ old.rs → new.rs");

    assert!(text[head].ends_with('▌'), "{text:#?}");

    press(&mut value, KeyCode::Down);
    let text_value = screen(&mut value, 100, 30);

    assert!(
        text_value[head + 1].ends_with('▌') && text_value[head + 1].contains("⋯ fn run() {"),
        "↓: 1 down: {text_value:#?}"
    );

    press(&mut value, KeyCode::Down);
    press(&mut value, KeyCode::Up);
    let text_2 = screen(&mut value, 100, 30);

    assert!(text_2[head + 1].ends_with('▌'), "↑: 1 up: {text_2:#?}");

    // the focused box: accent border; the other box: the line color
    let mut thread = ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 30)).unwrap();

    thread.draw(|file| value.draw(file, file.area())).unwrap();
    let buf = thread.backend().buffer().clone();

    assert_eq!(buf[(0, head as u16 + 1)].fg, theme::pal().accent);
    assert_ne!(buf[(0, next as u16 + 1)].fg, theme::pal().accent);

    press(&mut value, KeyCode::Char('j'));
    let text_3 = screen(&mut value, 100, 30);

    assert!(text_3[next].ends_with('▌'), "j: 1 file down: {text_3:#?}");

    press(&mut value, KeyCode::Char('k'));
    let text_4 = screen(&mut value, 100, 30);

    assert!(text_4[head].ends_with('▌'), "k: 1 file up: {text_4:#?}");

    value.key(KeyEvent::new(KeyCode::Down, KeyModifiers::CONTROL));
    let text_5 = screen(&mut value, 100, 30);

    assert!(text_5[next].ends_with('▌'), "ctrl+↓: 1 file down: {text_5:#?}");

    value.key(KeyEvent::new(KeyCode::Up, KeyModifiers::CONTROL));
    let text_6 = screen(&mut value, 100, 30);

    assert!(text_6[head].ends_with('▌'), "ctrl+↑: 1 file up: {text_6:#?}");
}

#[test]
fn space_selects_lines_apart_and_c_comments_on_them() {
    let mut value = prepare_space_selection();

    let Some(Modal::Comment { spots, quote, .. }) = &value.modal_state.modal else {
        panic!()
    };

    assert_eq!(
        spots,
        &[Spot {
            path: "src/a.rs".into(),
            line: 12,
            lines: "12 · removed 11".into()
        }]
    );
    assert_eq!(spots[0].label(), "src/a.rs:12 · removed 11");
    assert_eq!(
        quote,
        &[QLine::Code("- let b = 2;".into()), QLine::Code("+ let c = 4;".into())]
    );
    assert!(value.typing());

    let text_3 = screen(&mut value, 100, 30);

    assert!(
        text_3
            .iter()
            .any(|line_value| line_value.contains("comment on src/a.rs:12 · removed 11"))
            && text_3.iter().any(|line_2| line_2.contains("- let b = 2;")),
        "{text_3:#?}"
    );

    for character in "both wrong".chars() {
        press(&mut value, KeyCode::Char(character));
    }

    value.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL));

    assert!(value.modal_state.modal.is_none() && value.comment_state.comments[0].quote == "- let b = 2;\n+ let c = 4;");

    let text_4 = screen(&mut value, 100, 30);
    let character_value = row_of(&text_4, "12 · removed 11 · you: both wrong");

    assert!(
        text_4[character_value - 1].contains("+ let c = 4;"),
        "under the last selected line: {text_4:#?}"
    );
    assert!(
        !text_4.iter().any(|line_3| line_3.chars().nth(1) == Some('▌')),
        "the selection clears: {text_4:#?}"
    );

    let Act::Send(key_value) = value.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL)) else {
        panic!()
    };

    assert!(
        key_value.ends_with("- c1 [medium] `src/a.rs:12 · removed 11` by you: both wrong"),
        "{key_value}"
    );

    // esc (the view's while a selection is on) clears it
    value.at = (0..value.layout.rows.len())
        .find(|&i| text_of(&value, i).contains("+ let b = 3;"))
        .unwrap();
    press(&mut value, KeyCode::Char(' '));

    assert!(value.takes_esc());

    press(&mut value, KeyCode::Esc);

    assert!(!value.takes_esc() && value.selection_state.sel.is_empty());

    let _ = std::fs::remove_dir_all(crate::engine::store::thread_dir("changes-select-0000"));
}

fn prepare_space_selection() -> View {
    let mut value = view(parse(DIFF));

    value.id = "changes-select-0000".into();
    value.comment_state.comments.clear();
    let text = screen(&mut value, 100, 30);
    let b2 = row_of(&text, "- let b = 2;");

    // the cursor to `- let b = 2;`, space: selected, the cursor 1 down
    while !screen(&mut value, 100, 30)[b2].ends_with('▌') {
        press(&mut value, KeyCode::Down);
    }

    press(&mut value, KeyCode::Char(' '));
    let text_value = screen(&mut value, 100, 30);

    assert!(text_value[b2 + 1].ends_with('▌'), "1 down: {text_value:#?}");

    // skip `+ let b = 3;`, select `+ let c = 4;`: 2 lines apart
    press(&mut value, KeyCode::Down);
    press(&mut value, KeyCode::Char(' '));
    let text_2 = screen(&mut value, 100, 30);
    let mark = |y: usize| text_2[y].chars().nth(1) == Some('▌');

    assert!(
        mark(b2) && !mark(b2 + 1) && mark(b2 + 2),
        "gutter marks on the selected lines: {text_2:#?}"
    );

    // the selection's tint on its text, 0 on the other lines
    let mut thread = ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 30)).unwrap();

    thread.draw(|file| value.draw(file, file.area())).unwrap();
    let buf = thread.backend().buffer().clone();
    let _ = &buf;
    let text_x = text_2[b2]
        .find("let")
        .map(|x| text_2[b2][..x].chars().count() as u16)
        .unwrap();

    assert_ne!(buf[(text_x, b2 as u16)].bg, code_bg());
    assert_eq!(
        buf[(text_x, b2 as u16 + 1)].bg,
        code_bg(),
        "0 selection: the code background"
    );

    // c: the modal, 1 comment on both lines; the modal shows them
    press(&mut value, KeyCode::Char('c'));

    value
}

#[test]
fn left_gives_the_keys_to_the_panel_right_back_0_fold() {
    let mut value = view(parse(DIFF));

    screen(&mut value, 120, 30);
    press(&mut value, KeyCode::Down);
    press(&mut value, KeyCode::Down);
    press(&mut value, KeyCode::Right);

    assert!(!value.scm.focus);

    press(&mut value, KeyCode::Left);

    assert!(value.scm.shown && value.scm.focus);

    let text = screen(&mut value, 120, 30);

    assert!(text.iter().any(|line| line.contains("let b = 3;")), "0 fold: {text:#?}");

    press(&mut value, KeyCode::Right);

    assert!(!value.scm.focus);

    let text_value = screen(&mut value, 120, 30);

    assert!(
        text_value.iter().any(|line_value| line_value.contains("let b = 3;")),
        "0 fold: {text_value:#?}"
    );
}

#[test]
fn f_filters_by_name_and_content_with_marks_esc_clears_enter_jumps() {
    let mut value = view(parse(DIFF));

    screen(&mut value, 100, 30);
    value.key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL));

    assert!(value.typing() && value.takes_esc());

    // `fn t()`: content of the test file (the low tab) only: a hit across the tabs
    for character in "fn t()".chars() {
        press(&mut value, KeyCode::Char(character));
    }

    let text = screen(&mut value, 100, 30);

    assert!(text[1].contains(" f  fn t()"), "{}", text[1]);
    assert!(text.iter().any(|line| line.contains("1 files match")), "{text:#?}");
    assert!(
        text.iter().any(|line_value| line_value.contains("tests/a_test.rs"))
            && !text.iter().any(|line_2| line_2.contains("src/a.rs")),
        "content hit across tabs: {text:#?}"
    );

    press(&mut value, KeyCode::Esc);

    assert!(!value.typing() && value.filter_state.filter.is_empty());

    // a name hit: its chars marked
    value.key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL));

    for character_value in "test".chars() {
        press(&mut value, KeyCode::Char(character_value));
    }

    let mut thread = ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 30)).unwrap();

    thread
        .draw(|file_value| value.draw(file_value, file_value.area()))
        .unwrap();
    let buf = thread.backend().buffer().clone();
    let text_value: Vec<String> = (0..30)
        .map(|y| (0..100).map(|x| buf[(x, y)].symbol().to_string()).collect())
        .collect();
    let y = row_of(&text_value, "tests/a_test.rs");
    let x0 = text_value[y]
        .find("tests/")
        .map(|bytes| text_value[y][..bytes].chars().count() as u16)
        .unwrap();

    // a mark: accent text, on the cursor row underlined (the row is on the accent)
    let marked = |x: u16| {
        buf[(x, y as u16)].fg == theme::pal().accent
            || buf[(x, y as u16)]
                .modifier
                .contains(ratatui::style::Modifier::UNDERLINED)
    };

    assert!(marked(x0), "a marked char");
    assert!(!marked(x0 + 4), "`s` of `tests`: 0 mark");

    // enter: the cursor on the 1st hit, the keys back to the list; esc clears the filter
    press(&mut value, KeyCode::Enter);

    assert!(
        !value.typing()
            && value.layout.rows[value.at].head
            && value.changes.files[value.layout.rows[value.at].file].path == "tests/a_test.rs"
    );
    assert!(value.takes_esc());

    press(&mut value, KeyCode::Esc);

    assert_eq!(value.filter_state.filter, "");
}

#[test]
fn a_mouse_drag_selects_the_lines_it_passes() {
    let mut value = view(parse(DIFF));
    let text = screen(&mut value, 100, 30);
    let (agent, bytes) = (row_of(&text, "- let b = 2;"), row_of(&text, "+ let c = 4;"));
    let ev = |kind, row: usize| MouseEvent {
        kind,
        column: 20,
        row: row as u16,
        modifiers: KeyModifiers::NONE,
    };

    value.mouse(ev(MouseEventKind::Down(MouseButton::Left), agent));
    value.mouse(ev(MouseEventKind::Drag(MouseButton::Left), agent + 1));
    value.mouse(ev(MouseEventKind::Drag(MouseButton::Left), bytes));
    value.mouse(ev(MouseEventKind::Up(MouseButton::Left), bytes));
    let text_value = screen(&mut value, 100, 30);

    assert!(
        (agent..=bytes).all(|y| text_value[y].chars().nth(1) == Some('▌')),
        "{text_value:#?}"
    );
    assert!(text_value[bytes + 1].chars().nth(1) != Some('▌'), "{text_value:#?}");
    assert_eq!(value.selection_state.sel["src/a.rs"].len(), 3);

    // a click adds its line; a right click takes 1 out; 2 right clicks: 0 selection
    value.mouse(ev(MouseEventKind::Down(MouseButton::Left), bytes + 1));
    value.mouse(ev(MouseEventKind::Up(MouseButton::Left), bytes + 1));

    assert_eq!(value.selection_state.sel["src/a.rs"].len(), 4);

    value.mouse(ev(MouseEventKind::Down(MouseButton::Right), agent));
    value.mouse(ev(MouseEventKind::Up(MouseButton::Right), agent));

    assert_eq!(value.selection_state.sel["src/a.rs"].len(), 3);

    value.mouse(ev(MouseEventKind::Down(MouseButton::Right), bytes));

    assert!(value.selection_state.sel.is_empty(), "a double right click clears");

    // shift+click: the rows from the last click
    value.mouse(ev(MouseEventKind::Down(MouseButton::Left), agent));
    value.mouse(ev(MouseEventKind::Up(MouseButton::Left), agent));
    value.mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 20,
        row: bytes as u16,
        modifiers: KeyModifiers::SHIFT,
    });

    assert_eq!(value.selection_state.sel["src/a.rs"].len(), 3);

    // a right drag takes the rows it passes out
    value.selection_state.right_at = None;
    value.mouse(ev(MouseEventKind::Down(MouseButton::Right), agent));
    value.mouse(ev(MouseEventKind::Drag(MouseButton::Right), bytes));
    value.mouse(ev(MouseEventKind::Up(MouseButton::Right), bytes));

    assert!(value.selection_state.sel.is_empty());
}
