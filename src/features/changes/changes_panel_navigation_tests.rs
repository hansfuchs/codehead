use super::*;

#[test]
fn the_panel_lists_sections_levels_and_letters() {
    let mut value = test_view();

    value.changes.untracked.insert("tests/a_test.rs".into());
    value.changes.staged.insert("src/a.rs".into(), false);
    let text = draw(&mut value, 120, 30);
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

    let panel = |line_value: &String| line_value.chars().take(30).collect::<String>();

    assert!(
        text.iter()
            .any(|line_2| panel(line_2).contains("a_test.rs") && panel(line_2).trim_end().ends_with('U')),
        "untracked: U: {all}"
    );

    // tab: the keys to the panel; left on a label folds it
    alt_tap(&mut value);

    assert!(value.scm.focus);

    value.scm.at = value
        .scm
        .rows
        .iter()
        .position(|changes_value| *changes_value == PRow::Section(Sec::Changes, 4))
        .unwrap();
    value.key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
    let all = draw(&mut value, 120, 30).join("\n");

    assert!(all.contains("▸ CHANGES") && !all.contains("▾ medium"), "{all}");

    // the context menu hides it
    value.toggle_panel();
    let all = draw(&mut value, 120, 30).join("\n");

    assert!(!all.contains("STAGED") && !value.scm.focus, "{all}");
}

#[test]
fn the_panel_shows_hides_takes_the_keys_and_letters_with_a_modifier_do_nothing() {
    let mut value = test_view();

    draw(&mut value, 120, 30);

    assert!(value.scm.shown && !value.scm.focus);

    // alt+P, ctrl+s: 0 push, 0 stage (only the plain letters act)
    assert_eq!(
        value.key(KeyEvent::new(
            KeyCode::Char('P'),
            KeyModifiers::ALT | KeyModifiers::SHIFT
        )),
        Act::None
    );
    assert_eq!(
        value.key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL)),
        Act::None
    );
    assert!(value.operation_state.op.is_none());

    // ctrl+b: 0 effect (the context menu shows / hides the panel; tab gives it the keys)
    value.key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::CONTROL));

    assert!(value.scm.shown && !value.scm.focus);

    value.focus_panel(true);

    assert!(value.scm.shown && value.scm.focus);

    value.toggle_panel();

    assert!(!value.scm.shown && !value.scm.focus);

    value.toggle_panel();

    assert!(value.scm.shown && !value.scm.focus);
}

#[test]
fn space_picks_files_and_right_jumps_to_one() {
    let mut value = test_view();

    draw(&mut value, 120, 30);
    alt_tap(&mut value);
    let first = value
        .scm
        .rows
        .iter()
        .position(|result| matches!(result, PRow::File(_, false)))
        .unwrap();

    value.scm.at = first;
    value.key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE));

    assert_eq!(value.scm.picked.len(), 1);
    assert_eq!(value.scm.at, first + 1);

    value.key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));

    let PRow::File(i, _) = value.scm.rows[first + 1] else {
        panic!()
    };

    assert!(value.layout.rows[value.at].head && value.layout.rows[value.at].file == i);
}

#[test]
fn space_on_a_section_picks_every_item_in_it_again_unpicks() {
    let mut value = test_view();

    draw(&mut value, 120, 30);
    value.scm.focus = true;
    value.sync_rows();
    let count_value = value
        .changes
        .files
        .iter()
        .filter(|file| value.changes.staged.get(&file.path) != Some(&true))
        .count();

    assert!(count_value > 0);

    value.scm.at = value
        .scm
        .rows
        .iter()
        .position(|changes_value| matches!(changes_value, PRow::Section(Sec::Changes, _)))
        .unwrap();

    assert!(
        matches!(press(&mut value, KeyCode::Char(' ')), Act::Toast(thread) if thread == format!("{count_value} picked"))
    );
    assert_eq!(value.scm.picked.len(), count_value);
    assert!(
        matches!(value.scm.rows[value.scm.at], PRow::Section(Sec::Changes, _)),
        "the cursor stays"
    );

    // folded: space still takes the hidden files
    press(&mut value, KeyCode::Char(' '));

    assert!(value.scm.picked.is_empty());

    press(&mut value, KeyCode::Left);
    press(&mut value, KeyCode::Char(' '));

    assert_eq!(value.scm.picked.len(), count_value);
}

#[test]
fn arrows_and_page_keys_move_between_the_list_and_the_commit_message() {
    let mut value = test_view();

    draw(&mut value, 120, 30);
    value.scm.focus = true;
    value.sync_rows();
    let last = value.scm.rows.len() - 1;

    assert!(last > 0);

    // page down / up: the list's bottom / top
    press(&mut value, KeyCode::PageDown);

    assert_eq!(value.scm.at, last);

    press(&mut value, KeyCode::PageUp);

    assert_eq!(value.scm.at, 0);

    // ↑ on the 1st row: the message box; typing goes there
    press(&mut value, KeyCode::Up);

    assert!(value.scm.msg_focus);

    press(&mut value, KeyCode::Char('k'));

    assert_eq!(value.scm.msg.text(), "k");

    // ↓ on its last row: the list's 1st row
    press(&mut value, KeyCode::Down);

    assert!(!value.scm.msg_focus);
    assert_eq!(value.scm.at, 0);

    // page up on the 1st row: the message box; page down there: the list's bottom
    press(&mut value, KeyCode::PageUp);

    assert!(value.scm.msg_focus);

    press(&mut value, KeyCode::PageDown);

    assert!(!value.scm.msg_focus);
    assert_eq!(value.scm.at, last);
}
