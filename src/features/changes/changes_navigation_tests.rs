use super::*;

#[test]
fn e_opens_the_line_under_the_cursor() {
    let mut view = view(parse(DIFF));

    view.build(100);
    let i = (0..view.layout.rows.len())
        .find(|&i| text_of(&view, i).contains("let c = 4;"))
        .unwrap();

    view.at = i;

    assert_eq!(
        view.key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE)),
        Act::Open("src/a.rs".into(), 12)
    );
    assert_eq!(
        view.key(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::NONE)),
        Act::None,
        "o, i: 0 IDE (e on every screen)"
    );

    // a removed line: the worktree line it sat before (its old number: 11 as well here)
    let del = (0..view.layout.rows.len())
        .find(|&i| text_of(&view, i).contains("- let b = 2;"))
        .unwrap();

    view.at = del;

    assert_eq!(
        view.key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE)),
        Act::Open("src/a.rs".into(), 11)
    );

    // the selection wins: its 1st line, also with the cursor in an other file
    select(&mut view, "+ let c = 4;");
    select(&mut view, "+ fn t() {}");
    view.at = (0..view.layout.rows.len())
        .find(|&i| text_of(&view, i).contains("+ fn t() {}"))
        .unwrap();

    assert_eq!(
        view.key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE)),
        Act::Open("tests/a_test.rs".into(), 2)
    );

    view.at = view.layout.rows.iter().position(|row| row.head).unwrap();

    assert_eq!(
        view.key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE)),
        Act::Open("src/a.rs".into(), 12)
    );

    view.selection_state.sel.clear();

    // j: the next box head
    view.at = view.layout.rows.iter().position(|row| row.head).unwrap();
    view.key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));

    assert!(view.layout.rows[view.at].head && view.changes.files[view.layout.rows[view.at].file].path == "new.rs");
}

#[test]
fn arrows_go_1_line_j_k_and_ctrl_arrows_1_file_the_box_border_lights_up() {
    let mut view = view(parse(DIFF));
    let text = screen(&mut view, 100, 30);

    // the cursor (▌ at the right edge) on the 1st box's head
    let head = row_of(&text, "☐ src/a.rs");
    let next = row_of(&text, "☐ old.rs → new.rs");

    assert!(text[head].ends_with('▌'), "{text:#?}");

    press(&mut view, KeyCode::Down);
    let after_down = screen(&mut view, 100, 30);

    assert!(
        after_down[head + 1].ends_with('▌') && after_down[head + 1].contains("⋯ fn run() {"),
        "↓: 1 down: {after_down:#?}"
    );

    press(&mut view, KeyCode::Down);
    press(&mut view, KeyCode::Up);
    let after_up = screen(&mut view, 100, 30);

    assert!(after_up[head + 1].ends_with('▌'), "↑: 1 up: {after_up:#?}");

    // the focused box: accent border; the other box: the line color
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 30)).unwrap();

    terminal.draw(|frame| view.draw(frame, frame.area())).unwrap();
    let buf = terminal.backend().buffer().clone();

    assert_eq!(buf[(0, head as u16 + 1)].fg, theme::pal().accent);
    assert_ne!(buf[(0, next as u16 + 1)].fg, theme::pal().accent);

    press(&mut view, KeyCode::Char('j'));
    let after_j = screen(&mut view, 100, 30);

    assert!(after_j[next].ends_with('▌'), "j: 1 file down: {after_j:#?}");

    press(&mut view, KeyCode::Char('k'));
    let after_k = screen(&mut view, 100, 30);

    assert!(after_k[head].ends_with('▌'), "k: 1 file up: {after_k:#?}");

    view.key(KeyEvent::new(KeyCode::Down, KeyModifiers::CONTROL));
    let after_ctrl_down = screen(&mut view, 100, 30);

    assert!(
        after_ctrl_down[next].ends_with('▌'),
        "ctrl+↓: 1 file down: {after_ctrl_down:#?}"
    );

    view.key(KeyEvent::new(KeyCode::Up, KeyModifiers::CONTROL));
    let after_ctrl_up = screen(&mut view, 100, 30);

    assert!(
        after_ctrl_up[head].ends_with('▌'),
        "ctrl+↑: 1 file up: {after_ctrl_up:#?}"
    );
}

#[test]
fn space_selects_lines_apart_and_c_comments_on_them() {
    let mut view = prepare_space_selection();

    let Some(Modal::Comment { spots, quote, .. }) = &view.modal_state.modal else {
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
    assert!(view.typing());

    let modal_screen = screen(&mut view, 100, 30);

    assert!(
        modal_screen
            .iter()
            .any(|line| line.contains("comment on src/a.rs:12 · removed 11"))
            && modal_screen.iter().any(|line| line.contains("- let b = 2;")),
        "{modal_screen:#?}"
    );

    for typed in "both wrong".chars() {
        press(&mut view, KeyCode::Char(typed));
    }

    view.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL));

    assert!(view.modal_state.modal.is_none() && view.comment_state.comments[0].quote == "- let b = 2;\n+ let c = 4;");

    let commented = screen(&mut view, 100, 30);
    let comment_row = row_of(&commented, "12 · removed 11 · you: both wrong");

    assert!(
        commented[comment_row - 1].contains("+ let c = 4;"),
        "under the last selected line: {commented:#?}"
    );
    assert!(
        !commented.iter().any(|line| line.chars().nth(1) == Some('▌')),
        "the selection clears: {commented:#?}"
    );

    let Act::Send(batch) = view.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL)) else {
        panic!()
    };

    assert!(
        batch.ends_with("- c1 [medium] `src/a.rs:12 · removed 11` by you: both wrong"),
        "{batch}"
    );

    // esc (the view's while a selection is on) clears it
    view.at = (0..view.layout.rows.len())
        .find(|&i| text_of(&view, i).contains("+ let b = 3;"))
        .unwrap();
    press(&mut view, KeyCode::Char(' '));

    assert!(view.takes_esc());

    press(&mut view, KeyCode::Esc);

    assert!(!view.takes_esc() && view.selection_state.sel.is_empty());

    let _ = std::fs::remove_dir_all(crate::engine::store::thread_dir("changes-select-0000"));
}

fn prepare_space_selection() -> View {
    let mut view = view(parse(DIFF));

    view.id = "changes-select-0000".into();
    view.comment_state.comments.clear();
    let text = screen(&mut view, 100, 30);
    let b2 = row_of(&text, "- let b = 2;");

    // the cursor to `- let b = 2;`, space: selected, the cursor 1 down
    while !screen(&mut view, 100, 30)[b2].ends_with('▌') {
        press(&mut view, KeyCode::Down);
    }

    press(&mut view, KeyCode::Char(' '));
    let after_space = screen(&mut view, 100, 30);

    assert!(after_space[b2 + 1].ends_with('▌'), "1 down: {after_space:#?}");

    // skip `+ let b = 3;`, select `+ let c = 4;`: 2 lines apart
    press(&mut view, KeyCode::Down);
    press(&mut view, KeyCode::Char(' '));
    let selected = screen(&mut view, 100, 30);
    let mark = |y: usize| selected[y].chars().nth(1) == Some('▌');

    assert!(
        mark(b2) && !mark(b2 + 1) && mark(b2 + 2),
        "gutter marks on the selected lines: {selected:#?}"
    );

    // the selection's tint on its text, 0 on the other lines
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 30)).unwrap();

    terminal.draw(|frame| view.draw(frame, frame.area())).unwrap();
    let buf = terminal.backend().buffer().clone();
    let text_x = selected[b2]
        .find("let")
        .map(|offset| selected[b2][..offset].chars().count() as u16)
        .unwrap();

    assert_ne!(buf[(text_x, b2 as u16)].bg, code_bg());
    assert_eq!(
        buf[(text_x, b2 as u16 + 1)].bg,
        code_bg(),
        "0 selection: the code background"
    );

    // c: the modal, 1 comment on both lines; the modal shows them
    press(&mut view, KeyCode::Char('c'));

    view
}

#[test]
fn left_gives_the_keys_to_the_panel_right_back_0_fold() {
    let mut view = view(parse(DIFF));

    screen(&mut view, 120, 30);
    press(&mut view, KeyCode::Down);
    press(&mut view, KeyCode::Down);
    press(&mut view, KeyCode::Right);

    assert!(!view.scm.focus);

    press(&mut view, KeyCode::Left);

    assert!(view.scm.shown && view.scm.focus);

    let text = screen(&mut view, 120, 30);

    assert!(text.iter().any(|line| line.contains("let b = 3;")), "0 fold: {text:#?}");

    press(&mut view, KeyCode::Right);

    assert!(!view.scm.focus);

    let after_right = screen(&mut view, 120, 30);

    assert!(
        after_right.iter().any(|line| line.contains("let b = 3;")),
        "0 fold: {after_right:#?}"
    );
}

#[test]
fn f_filters_by_name_and_content_with_marks_esc_clears_enter_jumps() {
    let mut view = view(parse(DIFF));

    screen(&mut view, 100, 30);
    view.key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL));

    assert!(view.typing() && view.takes_esc());

    // `fn t()`: content of the test file (the low tab) only: a hit across the tabs
    for typed in "fn t()".chars() {
        press(&mut view, KeyCode::Char(typed));
    }

    let text = screen(&mut view, 100, 30);

    assert!(text[1].contains(" f  fn t()"), "{}", text[1]);
    assert!(text.iter().any(|line| line.contains("1 files match")), "{text:#?}");
    assert!(
        text.iter().any(|line| line.contains("tests/a_test.rs")) && !text.iter().any(|line| line.contains("src/a.rs")),
        "content hit across tabs: {text:#?}"
    );

    press(&mut view, KeyCode::Esc);

    assert!(!view.typing() && view.filter_state.filter.is_empty());

    // a name hit: its chars marked
    view.key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL));

    for typed in "test".chars() {
        press(&mut view, KeyCode::Char(typed));
    }

    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 30)).unwrap();

    terminal.draw(|frame| view.draw(frame, frame.area())).unwrap();
    let buf = terminal.backend().buffer().clone();
    let rows: Vec<String> = (0..30)
        .map(|y| (0..100).map(|x| buf[(x, y)].symbol().to_string()).collect())
        .collect();
    let y = row_of(&rows, "tests/a_test.rs");
    let x0 = rows[y]
        .find("tests/")
        .map(|offset| rows[y][..offset].chars().count() as u16)
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
    press(&mut view, KeyCode::Enter);

    assert!(
        !view.typing()
            && view.layout.rows[view.at].head
            && view.changes.files[view.layout.rows[view.at].file].path == "tests/a_test.rs"
    );
    assert!(view.takes_esc());

    press(&mut view, KeyCode::Esc);

    assert_eq!(view.filter_state.filter, "");
}

#[test]
fn a_mouse_drag_selects_the_lines_it_passes() {
    let mut view = view(parse(DIFF));
    let text = screen(&mut view, 100, 30);
    let (first, last) = (row_of(&text, "- let b = 2;"), row_of(&text, "+ let c = 4;"));
    let event = |kind, row: usize| MouseEvent {
        kind,
        column: 20,
        row: row as u16,
        modifiers: KeyModifiers::NONE,
    };

    view.mouse(event(MouseEventKind::Down(MouseButton::Left), first));
    view.mouse(event(MouseEventKind::Drag(MouseButton::Left), first + 1));
    view.mouse(event(MouseEventKind::Drag(MouseButton::Left), last));
    view.mouse(event(MouseEventKind::Up(MouseButton::Left), last));
    let dragged = screen(&mut view, 100, 30);

    assert!(
        (first..=last).all(|y| dragged[y].chars().nth(1) == Some('▌')),
        "{dragged:#?}"
    );
    assert!(dragged[last + 1].chars().nth(1) != Some('▌'), "{dragged:#?}");
    assert_eq!(view.selection_state.sel["src/a.rs"].len(), 3);

    // a click adds its line; a right click takes 1 out; 2 right clicks: 0 selection
    view.mouse(event(MouseEventKind::Down(MouseButton::Left), last + 1));
    view.mouse(event(MouseEventKind::Up(MouseButton::Left), last + 1));

    assert_eq!(view.selection_state.sel["src/a.rs"].len(), 4);

    view.mouse(event(MouseEventKind::Down(MouseButton::Right), first));
    view.mouse(event(MouseEventKind::Up(MouseButton::Right), first));

    assert_eq!(view.selection_state.sel["src/a.rs"].len(), 3);

    view.mouse(event(MouseEventKind::Down(MouseButton::Right), last));

    assert!(view.selection_state.sel.is_empty(), "a double right click clears");

    // shift+click: the rows from the last click
    view.mouse(event(MouseEventKind::Down(MouseButton::Left), first));
    view.mouse(event(MouseEventKind::Up(MouseButton::Left), first));
    view.mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 20,
        row: last as u16,
        modifiers: KeyModifiers::SHIFT,
    });

    assert_eq!(view.selection_state.sel["src/a.rs"].len(), 3);

    // a right drag takes the rows it passes out
    view.selection_state.right_at = None;
    view.mouse(event(MouseEventKind::Down(MouseButton::Right), first));
    view.mouse(event(MouseEventKind::Drag(MouseButton::Right), last));
    view.mouse(event(MouseEventKind::Up(MouseButton::Right), last));

    assert!(view.selection_state.sel.is_empty());
}
