use super::*;

fn ta(text: &str) -> TextArea {
    let mut input = TextArea::new(false, "");

    input.set_text(text);
    input
}

fn press(input: &mut TextArea, code: KeyCode, modifiers: KeyModifiers) -> bool {
    matches!(input.key(KeyEvent::new(code, modifiers)), Outcome::Handled)
}

#[test]
fn code_paste_gets_a_fence_with_its_tag_once() {
    let mut input = ta("see ");

    input.line_edge(true, false);
    input.paste("fn main() {\n    let v: Vec<u8> = vec![];\n}");

    assert_eq!(
        input.text(),
        "see \n```rs\nfn main() {\n    let v: Vec<u8> = vec![];\n}\n```"
    );

    // inside the fence: as is
    let mut input = TextArea::new(false, "");

    input.insert("```\n");
    input.paste("fn a() {\n}");

    assert_eq!(input.text(), "```\nfn a() {\n}");

    // prose: as is; unknown code: 0 tag
    let mut input = TextArea::new(false, "");

    input.paste("hello there\nhow are you");

    assert_eq!(input.text(), "hello there\nhow are you");

    let mut input = TextArea::new(false, "");

    input.paste(";;;\n{}");

    assert_eq!(input.text(), "```\n;;;\n{}\n```");
}

#[test]
fn paste_replaces_selection_and_keeps_lines() {
    let mut input = ta("abc");

    input.select_all();
    input.insert("x\r\ny");

    assert_eq!(input.text(), "x\ny");

    let mut input = TextArea::new(true, "");

    input.insert("a\nb");

    assert_eq!(input.text(), "a b");
}

/// ⌥⌫ / ctrl+⌫ / ctrl+h / ctrl+w: the string left of the cursor; in a path 1 segment.
/// ⌘⌫ (alt+⌫ off macOS): to the row's start. ⌘←→ (alt off macOS): row start / end.
#[test]
fn word_and_row_deletes_in_every_terminal_encoding() {
    let word_keys = [
        (KeyCode::Backspace, KeyModifiers::CONTROL),
        (KeyCode::Char('h'), KeyModifiers::CONTROL),
        (KeyCode::Char('w'), KeyModifiers::CONTROL),
    ];

    for (code, modifiers) in word_keys {
        let mut input = ta("git commit -m foo.bar-baz");

        input.move_end();
        press(&mut input, code, modifiers);

        assert_eq!(input.text(), "git commit -m ", "{code:?} {modifiers:?}");

        // a path: 1 segment, its `/` stays
        let mut input = ta("cd ~/projects/code-head");

        input.move_end();
        press(&mut input, code, modifiers);

        assert_eq!(input.text(), "cd ~/projects/");

        press(&mut input, code, modifiers);

        assert_eq!(input.text(), "cd ~/");

        press(&mut input, code, modifiers);

        assert_eq!(input.text(), "cd ");
    }

    // ⌥ (alt) on macOS: the string; off macOS: the row
    let mut input = ta("one two-three");

    input.move_end();
    press(&mut input, KeyCode::Backspace, KeyModifiers::ALT);

    assert_eq!(input.text(), if cfg!(target_os = "macos") { "one " } else { "" });

    // ⌘⌫ (super) and ctrl+u (⌘⌫ of macOS terminals): to the row's start
    for (code, modifiers) in [
        (KeyCode::Backspace, KeyModifiers::SUPER),
        (KeyCode::Char('u'), KeyModifiers::CONTROL),
    ] {
        let mut input = ta("first\nsecond row");

        input.render_for_test(40);
        input.move_end();
        press(&mut input, code, modifiers);

        assert_eq!(input.text(), "first\n", "{code:?}");
    }

    // ⌘← row start, ⌘→ row end; ctrl+← a word
    let mut input = ta("alpha beta gamma");

    input.render_for_test(40);
    input.move_end();
    press(&mut input, KeyCode::Left, KeyModifiers::SUPER);

    assert_eq!(input.cur.col, 0);

    press(&mut input, KeyCode::Right, KeyModifiers::SUPER);

    assert_eq!(input.cur.col, 16);

    press(&mut input, KeyCode::Left, KeyModifiers::CONTROL);

    assert_eq!(input.cur.col, 11);

    press(&mut input, KeyCode::Left, KeyModifiers::ALT);

    assert_eq!(input.cur.col, if cfg!(target_os = "macos") { 6 } else { 0 });
}

#[test]
fn delete_key_of_a_string_filter() {
    let event = |key, modifiers| KeyEvent::new(key, modifiers);
    let mut text = String::from("src/app.rs");

    assert!(delete_key(&mut text, &event(KeyCode::Backspace, KeyModifiers::CONTROL)));
    assert_eq!(text, "src/");
    assert!(delete_key(&mut text, &event(KeyCode::Backspace, KeyModifiers::NONE)));
    assert_eq!(text, "src");

    text = "a b".into();

    assert!(delete_key(&mut text, &event(KeyCode::Backspace, KeyModifiers::SUPER)));
    assert_eq!(text, "");
    assert!(!delete_key(&mut text, &event(KeyCode::Char('u'), KeyModifiers::NONE)));
}
