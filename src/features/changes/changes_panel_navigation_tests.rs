use super::*;

#[test]
fn the_panel_lists_sections_levels_and_letters() {
    let mut view = test_view();

    view.changes.untracked.insert("tests/a_test.rs".into());
    view.changes.staged.insert("src/a.rs".into(), false);
    let text = draw(&mut view, 120, 30);
    let all = text.join("\n");

    assert!(
        all.contains("▾ STAGED  1")
            && all.contains("▾ CHANGES  4")
            && all.contains("▾ COMMENTS  0")
            && all.contains("▾ STASH  0"),
        "{all}"
    );
    assert!(all.contains("▾ medium") && all.contains("▾ low"), "{all}");

    let staged = text.iter().position(|line| line.contains("STAGED")).unwrap();

    assert!(
        text[staged + 1].contains("a.rs") && text[staged + 1].contains('M'),
        "{all}"
    );

    let panel = |line: &String| line.chars().take(30).collect::<String>();

    assert!(
        text.iter()
            .any(|line| panel(line).contains("a_test.rs") && panel(line).trim_end().ends_with('U')),
        "untracked: U: {all}"
    );

    // tab: the keys to the panel; left on a label folds it
    alt_tap(&mut view);

    assert!(view.scm.focus);

    view.scm.at = view
        .scm
        .rows
        .iter()
        .position(|row| *row == PRow::Section(Sec::Changes, 4))
        .unwrap();
    view.key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
    let all = draw(&mut view, 120, 30).join("\n");

    assert!(all.contains("▸ CHANGES") && !all.contains("▾ medium"), "{all}");

    // the context menu hides it
    view.toggle_panel();
    let all = draw(&mut view, 120, 30).join("\n");

    assert!(!all.contains("STAGED") && !view.scm.focus, "{all}");
}

#[test]
fn the_panel_shows_hides_takes_the_keys_and_letters_with_a_modifier_do_nothing() {
    let mut view = test_view();

    draw(&mut view, 120, 30);

    assert!(view.scm.shown && !view.scm.focus);

    // alt+P, ctrl+s: 0 push, 0 stage (only the plain letters act)
    assert_eq!(
        view.key(KeyEvent::new(
            KeyCode::Char('P'),
            KeyModifiers::ALT | KeyModifiers::SHIFT
        )),
        Act::None
    );
    assert_eq!(
        view.key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL)),
        Act::None
    );
    assert!(view.operation_state.op.is_none());

    // ctrl+b: 0 effect (the context menu shows / hides the panel; tab gives it the keys)
    view.key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::CONTROL));

    assert!(view.scm.shown && !view.scm.focus);

    view.focus_panel(true);

    assert!(view.scm.shown && view.scm.focus);

    view.toggle_panel();

    assert!(!view.scm.shown && !view.scm.focus);

    view.toggle_panel();

    assert!(view.scm.shown && !view.scm.focus);
}

#[test]
fn space_picks_files_and_right_jumps_to_one() {
    let mut view = test_view();

    draw(&mut view, 120, 30);
    alt_tap(&mut view);
    let first = view
        .scm
        .rows
        .iter()
        .position(|row| matches!(row, PRow::File(_, false)))
        .unwrap();

    view.scm.at = first;
    view.key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE));

    assert_eq!(view.scm.picked.len(), 1);
    assert_eq!(view.scm.at, first + 1);

    view.key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));

    let PRow::File(i, _) = view.scm.rows[first + 1] else {
        panic!()
    };

    assert!(view.layout.rows[view.at].head && view.layout.rows[view.at].file == i);
}

#[test]
fn space_on_a_section_picks_every_item_in_it_again_unpicks() {
    let mut view = test_view();

    draw(&mut view, 120, 30);
    view.scm.focus = true;
    view.sync_rows();
    let unstaged = view
        .changes
        .files
        .iter()
        .filter(|file| view.changes.staged.get(&file.path) != Some(&true))
        .count();

    assert!(unstaged > 0);

    view.scm.at = view
        .scm
        .rows
        .iter()
        .position(|row| matches!(row, PRow::Section(Sec::Changes, _)))
        .unwrap();

    assert!(matches!(press(&mut view, KeyCode::Char(' ')), Act::Toast(toast) if toast == format!("{unstaged} picked")));
    assert_eq!(view.scm.picked.len(), unstaged);
    assert!(
        matches!(view.scm.rows[view.scm.at], PRow::Section(Sec::Changes, _)),
        "the cursor stays"
    );

    // folded: space still takes the hidden files
    press(&mut view, KeyCode::Char(' '));

    assert!(view.scm.picked.is_empty());

    press(&mut view, KeyCode::Left);
    press(&mut view, KeyCode::Char(' '));

    assert_eq!(view.scm.picked.len(), unstaged);
}

#[test]
fn arrows_and_page_keys_move_between_the_list_and_the_commit_message() {
    let mut view = test_view();

    draw(&mut view, 120, 30);
    view.scm.focus = true;
    view.sync_rows();
    let last = view.scm.rows.len() - 1;

    assert!(last > 0);

    // page down / up: the list's bottom / top
    press(&mut view, KeyCode::PageDown);

    assert_eq!(view.scm.at, last);

    press(&mut view, KeyCode::PageUp);

    assert_eq!(view.scm.at, 0);

    // ↑ on the 1st row: the message box; typing goes there
    press(&mut view, KeyCode::Up);

    assert!(view.scm.msg_focus);

    press(&mut view, KeyCode::Char('k'));

    assert_eq!(view.scm.msg.text(), "k");

    // ↓ on its last row: the list's 1st row
    press(&mut view, KeyCode::Down);

    assert!(!view.scm.msg_focus);
    assert_eq!(view.scm.at, 0);

    // page up on the 1st row: the message box; page down there: the list's bottom
    press(&mut view, KeyCode::PageUp);

    assert!(view.scm.msg_focus);

    press(&mut view, KeyCode::PageDown);

    assert!(!view.scm.msg_focus);
    assert_eq!(view.scm.at, last);
}
