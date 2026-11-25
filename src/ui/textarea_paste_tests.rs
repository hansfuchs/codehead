use super::*;

fn ta(text: &str) -> TextArea {
    let mut thread = TextArea::new(false, "");

    thread.set_text(text);
    thread
}

fn press(thread: &mut TextArea, character: KeyCode, message: KeyModifiers) -> bool {
    matches!(thread.key(KeyEvent::new(character, message)), Outcome::Handled)
}

#[test]
fn code_paste_gets_a_fence_with_its_tag_once() {
    let mut thread = ta("see ");

    thread.line_edge(true, false);
    thread.paste("fn main() {\n    let v: Vec<u8> = vec![];\n}");

    assert_eq!(
        thread.text(),
        "see \n```rs\nfn main() {\n    let v: Vec<u8> = vec![];\n}\n```"
    );

    // inside the fence: as is
    let mut user = TextArea::new(false, "");

    user.insert("```\n");
    user.paste("fn a() {\n}");

    assert_eq!(user.text(), "```\nfn a() {\n}");

    // prose: as is; unknown code: 0 tag
    let mut path = TextArea::new(false, "");

    path.paste("hello there\nhow are you");

    assert_eq!(path.text(), "hello there\nhow are you");

    let mut query = TextArea::new(false, "");

    query.paste(";;;\n{}");

    assert_eq!(query.text(), "```\n;;;\n{}\n```");
}

#[test]
fn paste_replaces_selection_and_keeps_lines() {
    let mut thread = ta("abc");

    thread.select_all();
    thread.insert("x\r\ny");

    assert_eq!(thread.text(), "x\ny");

    let mut text_value = TextArea::new(true, "");

    text_value.insert("a\nb");

    assert_eq!(text_value.text(), "a b");
}

/// ⌥⌫ / ctrl+⌫ / ctrl+h / ctrl+w: the string left of the cursor; in a path 1 segment.
/// ⌘⌫ (alt+⌫ off macOS): to the row's start. ⌘←→ (alt off macOS): row start / end.
#[test]
fn word_and_row_deletes_in_every_terminal_encoding() {
    let word = [
        (KeyCode::Backspace, KeyModifiers::CONTROL),
        (KeyCode::Char('h'), KeyModifiers::CONTROL),
        (KeyCode::Char('w'), KeyModifiers::CONTROL),
    ];

    for (character, message) in word {
        let mut thread = ta("git commit -m foo.bar-baz");

        thread.move_end();
        press(&mut thread, character, message);

        assert_eq!(thread.text(), "git commit -m ", "{character:?} {message:?}");

        // a path: 1 segment, its `/` stays
        let mut thread_value = ta("cd ~/projects/code-head");

        thread_value.move_end();
        press(&mut thread_value, character, message);

        assert_eq!(thread_value.text(), "cd ~/projects/");

        press(&mut thread_value, character, message);

        assert_eq!(thread_value.text(), "cd ~/");

        press(&mut thread_value, character, message);

        assert_eq!(thread_value.text(), "cd ");
    }

    // ⌥ (alt) on macOS: the string; off macOS: the row
    let mut thread_2 = ta("one two-three");

    thread_2.move_end();
    press(&mut thread_2, KeyCode::Backspace, KeyModifiers::ALT);

    assert_eq!(thread_2.text(), if cfg!(target_os = "macos") { "one " } else { "" });

    // ⌘⌫ (super) and ctrl+u (⌘⌫ of macOS terminals): to the row's start
    for (character_value, message_value) in [
        (KeyCode::Backspace, KeyModifiers::SUPER),
        (KeyCode::Char('u'), KeyModifiers::CONTROL),
    ] {
        let mut thread_3 = ta("first\nsecond row");

        thread_3.render_for_test(40);
        thread_3.move_end();
        press(&mut thread_3, character_value, message_value);

        assert_eq!(thread_3.text(), "first\n", "{character_value:?}");
    }

    // ⌘← row start, ⌘→ row end; ctrl+← a word
    let mut thread_4 = ta("alpha beta gamma");

    thread_4.render_for_test(40);
    thread_4.move_end();
    press(&mut thread_4, KeyCode::Left, KeyModifiers::SUPER);

    assert_eq!(thread_4.cur.col, 0);

    press(&mut thread_4, KeyCode::Right, KeyModifiers::SUPER);

    assert_eq!(thread_4.cur.col, 16);

    press(&mut thread_4, KeyCode::Left, KeyModifiers::CONTROL);

    assert_eq!(thread_4.cur.col, 11);

    press(&mut thread_4, KeyCode::Left, KeyModifiers::ALT);

    assert_eq!(thread_4.cur.col, if cfg!(target_os = "macos") { 6 } else { 0 });
}

#[test]
fn delete_key_of_a_string_filter() {
    let key_2 = |key, key_value| KeyEvent::new(key, key_value);
    let mut text = String::from("src/app.rs");

    assert!(delete_key(&mut text, &key_2(KeyCode::Backspace, KeyModifiers::CONTROL)));
    assert_eq!(text, "src/");
    assert!(delete_key(&mut text, &key_2(KeyCode::Backspace, KeyModifiers::NONE)));
    assert_eq!(text, "src");

    text = "a b".into();

    assert!(delete_key(&mut text, &key_2(KeyCode::Backspace, KeyModifiers::SUPER)));
    assert_eq!(text, "");
    assert!(!delete_key(&mut text, &key_2(KeyCode::Char('u'), KeyModifiers::NONE)));
}
