use super::*;

fn ta(text: &str) -> TextArea {
    let mut thread = TextArea::new(false, "");

    thread.set_text(text);
    thread
}

#[test]
fn sent_text_drops_trailing_empty_list_items() {
    // the owner's message: a stray `7. ` line an Enter left at the end
    assert_eq!(
        without_empty_items("points:\n7. set auto-allow\n8. yes, v1\n7. "),
        "points:\n7. set auto-allow\n8. yes, v1"
    );
    assert_eq!(without_empty_items("a\n- x\n- \n  - [ ] \n\n"), "a\n- x");
    assert_eq!(without_empty_items("a\n3)"), "a");

    // items with text, plain lines, a lone marker as the whole message: kept
    assert_eq!(without_empty_items("a\n1. b  "), "a\n1. b");
    assert_eq!(without_empty_items("a\nv1."), "a\nv1.");
    assert_eq!(without_empty_items("-"), "-");
}

#[test]
fn markdown_lists_continue_and_outdent() {
    let key_value = |character: KeyCode| KeyEvent::new(character, KeyModifiers::NONE);
    let mut thread = ta("- a");

    thread.move_end();
    thread.key(key_value(KeyCode::Enter));

    assert_eq!(thread.text(), "- a\n- ");

    let mut thread_value = ta("- a\n  - b");

    thread_value.move_end();
    thread_value.key(key_value(KeyCode::Enter));

    assert_eq!(thread_value.text(), "- a\n  - b\n  - ");

    // backspace on the empty item: 1 layer out, then the marker goes in 1 press
    thread_value.key(key_value(KeyCode::Backspace));

    assert_eq!(thread_value.text(), "- a\n  - b\n- ");

    thread_value.key(key_value(KeyCode::Backspace));

    assert_eq!(thread_value.text(), "- a\n  - b\n");

    // undo: 1 step per press
    thread_value.key(KeyEvent::new(KeyCode::Char('z'), KeyModifiers::CONTROL));

    assert_eq!(thread_value.text(), "- a\n  - b\n- ");

    // enter on an empty item: 1 layer out too
    let mut thread_2 = ta("    - ");

    thread_2.move_end();
    thread_2.key(key_value(KeyCode::Enter));

    assert_eq!(thread_2.text(), "  - ");

    // numbers count up, check boxes stay open, text after the cursor moves along
    let mut thread_3 = ta("9. x");

    thread_3.move_end();
    thread_3.key(key_value(KeyCode::Enter));

    assert_eq!(thread_3.text(), "9. x\n10. ");

    let mut thread_4 = ta("* [x] done");

    thread_4.move_end();
    thread_4.newline();

    assert_eq!(thread_4.text(), "* [x] done\n* [ ] ");

    let mut thread_5 = ta("- ab");

    thread_5.move_end();
    thread_5.key(key_value(KeyCode::Left));
    thread_5.key(key_value(KeyCode::Enter));

    assert_eq!(thread_5.text(), "- a\n- b");

    // plain text, a cursor inside the marker, `-x`: plain new line
    for (text, want) in [("ab", "ab\n"), ("-x", "-x\n")] {
        let mut thread_6 = ta(text);

        thread_6.move_end();
        thread_6.key(key_value(KeyCode::Enter));

        assert_eq!(thread_6.text(), want);
    }

    let mut thread_7 = ta("- a");

    thread_7.move_start();
    thread_7.key(key_value(KeyCode::Enter));

    assert_eq!(thread_7.text(), "\n- a");

    // backspace after text of an item: 1 char
    let mut thread_8 = ta("- a");

    thread_8.move_end();
    thread_8.key(key_value(KeyCode::Backspace));

    assert_eq!(thread_8.text(), "- ");

    // backspace on an empty item after a click (anchor = cursor): 0 panic
    let mut thread_9 = ta("- a\n- ");

    thread_9.move_end();
    thread_9.anchor = Some(thread_9.cur);
    thread_9.key(key_value(KeyCode::Backspace));
    thread_9.key(key_value(KeyCode::Char('x')));

    assert_eq!(thread_9.text(), "- a\nx");
}

#[test]
fn space_on_empty_list_item_indents() {
    let key_value = |character: KeyCode| KeyEvent::new(character, KeyModifiers::NONE);
    let mut thread = ta("- a");

    thread.move_end();
    thread.key(key_value(KeyCode::Enter));

    // space on the empty item: 1 layer in, backspace: 1 layer out again
    thread.key(key_value(KeyCode::Char(' ')));

    assert_eq!(thread.text(), "- a\n  - ");

    thread.key(key_value(KeyCode::Char(' ')));

    assert_eq!(thread.text(), "- a\n    - ");

    thread.key(key_value(KeyCode::Backspace));

    assert_eq!(thread.text(), "- a\n  - ");

    // the next key goes after the marker
    thread.key(key_value(KeyCode::Char('b')));

    assert_eq!(thread.text(), "- a\n  - b");

    // undo: the char, then the indent
    thread.key(KeyEvent::new(KeyCode::Char('z'), KeyModifiers::CONTROL));

    assert_eq!(thread.text(), "- a\n  - ");

    // numbers + check boxes too
    let mut thread_value = ta("1. ");

    thread_value.move_end();
    thread_value.key(key_value(KeyCode::Char(' ')));

    assert_eq!(thread_value.text(), "  1. ");

    // an item with text, a cursor inside the marker: plain space
    let mut thread_2 = ta("- a");

    thread_2.move_end();
    thread_2.key(key_value(KeyCode::Char(' ')));

    assert_eq!(thread_2.text(), "- a ");

    let mut thread_3 = ta("- ");

    thread_3.move_start();
    thread_3.key(key_value(KeyCode::Char(' ')));

    assert_eq!(thread_3.text(), " - ");
}

#[test]
fn space_and_backspace_right_after_the_marker_of_an_item_with_text() {
    let key_value = |character: KeyCode| KeyEvent::new(character, KeyModifiers::NONE);
    let undo = KeyEvent::new(KeyCode::Char('z'), KeyModifiers::CONTROL);
    let redo = KeyEvent::new(KeyCode::Char('y'), KeyModifiers::CONTROL);

    // cursor right after `- ` of `- foo`: space → 1 layer in, the cursor stays before `foo`
    let mut thread = ta("- a\n- foo");

    thread.move_end();

    for _ in 0..3 {
        thread.key(key_value(KeyCode::Left));
    }

    thread.key(key_value(KeyCode::Char(' ')));

    assert_eq!(thread.text(), "- a\n  - foo");

    thread.key(key_value(KeyCode::Char(' ')));

    assert_eq!(thread.text(), "- a\n    - foo");

    thread.key(key_value(KeyCode::Char('x')));

    assert_eq!(thread.text(), "- a\n    - xfoo");

    thread.key(key_value(KeyCode::Backspace));

    // backspace right after the marker: 1 layer out
    thread.key(key_value(KeyCode::Backspace));

    assert_eq!(thread.text(), "- a\n  - foo");

    thread.key(key_value(KeyCode::Backspace));

    assert_eq!(thread.text(), "- a\n- foo");

    // indent 0 + text: backspace as before (1 char)
    thread.key(key_value(KeyCode::Backspace));

    assert_eq!(thread.text(), "- a\n-foo");

    // undo: 1 step per press, back through the layers; redo forward
    thread.key(undo);

    assert_eq!(thread.text(), "- a\n- foo");

    thread.key(undo);

    assert_eq!(thread.text(), "- a\n  - foo");

    thread.key(undo);

    assert_list_marker_edits(key_value, redo, thread);
}

fn assert_list_marker_edits(key_value: impl Fn(KeyCode) -> KeyEvent, redo: KeyEvent, mut thread: TextArea) {
    assert_eq!(thread.text(), "- a\n    - foo");

    thread.key(redo);

    assert_eq!(thread.text(), "- a\n  - foo");

    // numbers and check boxes: right after the whole marker
    let mut thread_value = ta("1. one");

    thread_value.move_start();

    for _ in 0..3 {
        thread_value.key(key_value(KeyCode::Right));
    }

    thread_value.key(key_value(KeyCode::Char(' ')));

    assert_eq!(thread_value.text(), "  1. one");

    let mut thread_2 = ta("* [ ] task");

    thread_2.move_start();

    for _ in 0..6 {
        thread_2.key(key_value(KeyCode::Right));
    }

    thread_2.key(key_value(KeyCode::Char(' ')));

    assert_eq!(thread_2.text(), "  * [ ] task");

    thread_2.key(key_value(KeyCode::Backspace));

    assert_eq!(thread_2.text(), "* [ ] task");

    // inside the text: plain space / backspace
    let mut thread_3 = ta("  - foo");

    thread_3.move_end();
    thread_3.key(key_value(KeyCode::Left));
    thread_3.key(key_value(KeyCode::Char(' ')));

    assert_eq!(thread_3.text(), "  - fo o");

    thread_3.key(key_value(KeyCode::Backspace));

    assert_eq!(thread_3.text(), "  - foo");

    // a single-line input: 0 list keys
    let mut thread_4 = TextArea::new(true, "");

    thread_4.set_text("- foo");
    thread_4.move_start();
    thread_4.key(key_value(KeyCode::Right));
    thread_4.key(key_value(KeyCode::Right));
    thread_4.key(key_value(KeyCode::Char(' ')));

    assert_eq!(thread_4.text(), "-  foo");
}
