use super::*;

fn shortcut(count: &str, thread: &str) -> Shortcut {
    Shortcut {
        name: count.into(),
        text: thread.into(),
    }
}

#[test]
fn names_expand_at_word_starts_only() {
    let line = vec![shortcut("review", "line 1\nline 2"), shortcut("fix", "fix it")];

    assert_eq!(expand_with(&line, "please #review now"), "please line 1\nline 2 now");
    assert_eq!(expand_with(&line, "#fix"), "fix it");

    // 0 shortcut, a # in a word, a longer name: as typed
    assert_eq!(expand_with(&line, "issue #12 a#fix #fixes"), "issue #12 a#fix #fixes");
    assert_eq!(matches(&line, "fi"), [1]);
    assert_eq!(matches(&line, "e"), [0]);
}

#[test]
fn the_file_round_trips_multiline_texts() {
    let line = vec![shortcut("a", "x\ny 'z'"), shortcut("b-2", "has ''' in it")];

    assert_eq!(parse(&render(&line)).unwrap(), line);
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
    let mut path_value = Pick::default();
    let mut ta = TextArea::new(false, "");

    ta.set_text("go #re");
    ta.move_end();
    let key_2 = |key_value| KeyEvent::new(key_value, KeyModifiers::NONE);

    // ↓: the 2nd row's name in the text at once; the typed query keeps both rows
    assert!(path_value.key(key_2(KeyCode::Down), &mut ta));
    assert_eq!(ta.text(), "go #review");
    assert!(path_value.key(key_2(KeyCode::Down), &mut ta));
    assert_eq!(ta.text(), "go #rebase");

    // a name char: the typed query again, the char after it
    assert!(!path_value.key(key_2(KeyCode::Char('v')), &mut ta));
    assert_eq!(ta.text(), "go #re");

    ta.insert("v");

    assert!(path_value.key(key_2(KeyCode::Tab), &mut ta));
    assert_eq!(ta.text(), "go review it\nall");

    let _ = std::fs::remove_file(path());
}
