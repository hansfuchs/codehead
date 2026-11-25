use super::*;

fn ta(text: &str) -> TextArea {
    let mut input = TextArea::new(false, "");

    input.set_text(text);
    input
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
    let press = |code: KeyCode| KeyEvent::new(code, KeyModifiers::NONE);
    let mut input = ta("- a");

    input.move_end();
    input.key(press(KeyCode::Enter));

    assert_eq!(input.text(), "- a\n- ");

    let mut input = ta("- a\n  - b");

    input.move_end();
    input.key(press(KeyCode::Enter));

    assert_eq!(input.text(), "- a\n  - b\n  - ");

    // backspace on the empty item: 1 layer out, then the marker goes in 1 press
    input.key(press(KeyCode::Backspace));

    assert_eq!(input.text(), "- a\n  - b\n- ");

    input.key(press(KeyCode::Backspace));

    assert_eq!(input.text(), "- a\n  - b\n");

    // undo: 1 step per press
    input.key(KeyEvent::new(KeyCode::Char('z'), KeyModifiers::CONTROL));

    assert_eq!(input.text(), "- a\n  - b\n- ");

    // enter on an empty item: 1 layer out too
    let mut input = ta("    - ");

    input.move_end();
    input.key(press(KeyCode::Enter));

    assert_eq!(input.text(), "  - ");

    // numbers count up, check boxes stay open, text after the cursor moves along
    let mut input = ta("9. x");

    input.move_end();
    input.key(press(KeyCode::Enter));

    assert_eq!(input.text(), "9. x\n10. ");

    let mut input = ta("* [x] done");

    input.move_end();
    input.newline();

    assert_eq!(input.text(), "* [x] done\n* [ ] ");

    let mut input = ta("- ab");

    input.move_end();
    input.key(press(KeyCode::Left));
    input.key(press(KeyCode::Enter));

    assert_eq!(input.text(), "- a\n- b");

    // plain text, a cursor inside the marker, `-x`: plain new line
    for (text, want) in [("ab", "ab\n"), ("-x", "-x\n")] {
        let mut input = ta(text);

        input.move_end();
        input.key(press(KeyCode::Enter));

        assert_eq!(input.text(), want);
    }

    let mut input = ta("- a");

    input.move_start();
    input.key(press(KeyCode::Enter));

    assert_eq!(input.text(), "\n- a");

    // backspace after text of an item: 1 char
    let mut input = ta("- a");

    input.move_end();
    input.key(press(KeyCode::Backspace));

    assert_eq!(input.text(), "- ");

    // backspace on an empty item after a click (anchor = cursor): 0 panic
    let mut input = ta("- a\n- ");

    input.move_end();
    input.anchor = Some(input.cur);
    input.key(press(KeyCode::Backspace));
    input.key(press(KeyCode::Char('x')));

    assert_eq!(input.text(), "- a\nx");
}

#[test]
fn space_on_empty_list_item_indents() {
    let press = |code: KeyCode| KeyEvent::new(code, KeyModifiers::NONE);
    let mut input = ta("- a");

    input.move_end();
    input.key(press(KeyCode::Enter));

    // space on the empty item: 1 layer in, backspace: 1 layer out again
    input.key(press(KeyCode::Char(' ')));

    assert_eq!(input.text(), "- a\n  - ");

    input.key(press(KeyCode::Char(' ')));

    assert_eq!(input.text(), "- a\n    - ");

    input.key(press(KeyCode::Backspace));

    assert_eq!(input.text(), "- a\n  - ");

    // the next key goes after the marker
    input.key(press(KeyCode::Char('b')));

    assert_eq!(input.text(), "- a\n  - b");

    // undo: the char, then the indent
    input.key(KeyEvent::new(KeyCode::Char('z'), KeyModifiers::CONTROL));

    assert_eq!(input.text(), "- a\n  - ");

    // numbers + check boxes too
    let mut input = ta("1. ");

    input.move_end();
    input.key(press(KeyCode::Char(' ')));

    assert_eq!(input.text(), "  1. ");

    // an item with text, a cursor inside the marker: plain space
    let mut input = ta("- a");

    input.move_end();
    input.key(press(KeyCode::Char(' ')));

    assert_eq!(input.text(), "- a ");

    let mut input = ta("- ");

    input.move_start();
    input.key(press(KeyCode::Char(' ')));

    assert_eq!(input.text(), " - ");
}

#[test]
fn space_and_backspace_right_after_the_marker_of_an_item_with_text() {
    let press = |code: KeyCode| KeyEvent::new(code, KeyModifiers::NONE);
    let undo = KeyEvent::new(KeyCode::Char('z'), KeyModifiers::CONTROL);
    let redo = KeyEvent::new(KeyCode::Char('y'), KeyModifiers::CONTROL);

    // cursor right after `- ` of `- foo`: space → 1 layer in, the cursor stays before `foo`
    let mut input = ta("- a\n- foo");

    input.move_end();

    for _ in 0..3 {
        input.key(press(KeyCode::Left));
    }

    input.key(press(KeyCode::Char(' ')));

    assert_eq!(input.text(), "- a\n  - foo");

    input.key(press(KeyCode::Char(' ')));

    assert_eq!(input.text(), "- a\n    - foo");

    input.key(press(KeyCode::Char('x')));

    assert_eq!(input.text(), "- a\n    - xfoo");

    input.key(press(KeyCode::Backspace));

    // backspace right after the marker: 1 layer out
    input.key(press(KeyCode::Backspace));

    assert_eq!(input.text(), "- a\n  - foo");

    input.key(press(KeyCode::Backspace));

    assert_eq!(input.text(), "- a\n- foo");

    // indent 0 + text: backspace as before (1 char)
    input.key(press(KeyCode::Backspace));

    assert_eq!(input.text(), "- a\n-foo");

    // undo: 1 step per press, back through the layers; redo forward
    input.key(undo);

    assert_eq!(input.text(), "- a\n- foo");

    input.key(undo);

    assert_eq!(input.text(), "- a\n  - foo");

    input.key(undo);

    assert_list_marker_edits(press, redo, input);
}

fn assert_list_marker_edits(press: impl Fn(KeyCode) -> KeyEvent, redo: KeyEvent, mut input: TextArea) {
    assert_eq!(input.text(), "- a\n    - foo");

    input.key(redo);

    assert_eq!(input.text(), "- a\n  - foo");

    // numbers and check boxes: right after the whole marker
    let mut input = ta("1. one");

    input.move_start();

    for _ in 0..3 {
        input.key(press(KeyCode::Right));
    }

    input.key(press(KeyCode::Char(' ')));

    assert_eq!(input.text(), "  1. one");

    let mut input = ta("* [ ] task");

    input.move_start();

    for _ in 0..6 {
        input.key(press(KeyCode::Right));
    }

    input.key(press(KeyCode::Char(' ')));

    assert_eq!(input.text(), "  * [ ] task");

    input.key(press(KeyCode::Backspace));

    assert_eq!(input.text(), "* [ ] task");

    // inside the text: plain space / backspace
    let mut input = ta("  - foo");

    input.move_end();
    input.key(press(KeyCode::Left));
    input.key(press(KeyCode::Char(' ')));

    assert_eq!(input.text(), "  - fo o");

    input.key(press(KeyCode::Backspace));

    assert_eq!(input.text(), "  - foo");

    // a single-line input: 0 list keys
    let mut input = TextArea::new(true, "");

    input.set_text("- foo");
    input.move_start();
    input.key(press(KeyCode::Right));
    input.key(press(KeyCode::Right));
    input.key(press(KeyCode::Char(' ')));

    assert_eq!(input.text(), "-  foo");
}
