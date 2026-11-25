use super::*;

fn shortcut(name: &str, text: &str) -> Shortcut {
    Shortcut {
        name: name.into(),
        text: text.into(),
    }
}

#[test]
fn names_expand_at_word_starts_only() {
    let list = vec![shortcut("review", "line 1\nline 2"), shortcut("fix", "fix it")];

    assert_eq!(expand_with(&list, "please #review now"), "please line 1\nline 2 now");
    assert_eq!(expand_with(&list, "#fix"), "fix it");

    // 0 shortcut, a # in a word, a longer name: as typed
    assert_eq!(expand_with(&list, "issue #12 a#fix #fixes"), "issue #12 a#fix #fixes");
    assert_eq!(matches(&list, "fi"), [1]);
    assert_eq!(matches(&list, "e"), [0]);
}

#[test]
fn the_file_round_trips_multiline_texts() {
    let list = vec![shortcut("a", "x\ny 'z'"), shortcut("b-2", "has ''' in it")];

    assert_eq!(parse(&render(&list)).unwrap(), list);
}

#[test]
fn the_dropdown_previews_the_name_and_tab_puts_the_text_in() {
    let _g = crate::engine::ENV_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);

    save(&[
        shortcut("review", "review it\nall"),
        shortcut("rebase", "rebase on main"),
    ])
    .unwrap();
    let mut pick = Pick::default();
    let mut ta = TextArea::new(false, "");

    ta.set_text("go #re");
    ta.move_end();
    let press = |code| KeyEvent::new(code, KeyModifiers::NONE);

    // ↓: the 2nd row's name in the text at once; the typed query keeps both rows
    assert!(pick.key(press(KeyCode::Down), &mut ta));
    assert_eq!(ta.text(), "go #review");
    assert!(pick.key(press(KeyCode::Down), &mut ta));
    assert_eq!(ta.text(), "go #rebase");

    // a name char: the typed query again, the char after it
    assert!(!pick.key(press(KeyCode::Char('v')), &mut ta));
    assert_eq!(ta.text(), "go #re");

    ta.insert("v");

    assert!(pick.key(press(KeyCode::Tab), &mut ta));
    assert_eq!(ta.text(), "go review it\nall");

    let _ = std::fs::remove_file(path());
}
