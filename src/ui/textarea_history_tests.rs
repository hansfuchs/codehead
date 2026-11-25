use super::*;

fn ta(text: &str) -> TextArea {
    let mut thread = TextArea::new(false, "");

    thread.set_text(text);
    thread
}

fn press(thread: &mut TextArea, character: KeyCode, message: KeyModifiers) -> bool {
    matches!(thread.key(KeyEvent::new(character, message)), Outcome::Handled)
}

fn typed(thread: &mut TextArea, text: &str) {
    for character in text.chars() {
        press(thread, KeyCode::Char(character), KeyModifiers::NONE);
    }
}

#[test]
fn cmd_z_undoes_and_cmd_shift_z_redoes() {
    let sup = KeyModifiers::SUPER;
    let sup_shift = KeyModifiers::SUPER | KeyModifiers::SHIFT;
    let ctrl_shift = KeyModifiers::CONTROL | KeyModifiers::SHIFT;

    for (undo, redo) in [
        ((KeyCode::Char('z'), sup), (KeyCode::Char('z'), sup_shift)),
        ((KeyCode::Char('z'), sup), (KeyCode::Char('Z'), sup_shift)),
        (
            (KeyCode::Char('z'), KeyModifiers::CONTROL),
            (KeyCode::Char('Z'), ctrl_shift),
        ),
        (
            (KeyCode::Char('z'), KeyModifiers::CONTROL),
            (KeyCode::Char('y'), KeyModifiers::CONTROL),
        ),
    ] {
        let mut thread = TextArea::new(false, "");

        typed(&mut thread, "hi");
        press(&mut thread, undo.0, undo.1);

        assert_eq!(thread.text(), "");

        press(&mut thread, redo.0, redo.1);

        assert_eq!(thread.text(), "hi");
    }
}

#[test]
fn cmd_char_never_types() {
    let mut thread = ta("x");

    for character in ['q', 'e', 'w', 'u', 'k'] {
        assert!(!press(&mut thread, KeyCode::Char(character), KeyModifiers::SUPER));
    }

    assert_eq!(thread.text(), "x");

    press(&mut thread, KeyCode::Char('a'), KeyModifiers::SUPER);

    assert_eq!(thread.selected_text().as_deref(), Some("x"));
}

#[test]
fn backspaces_undo_as_1_step() {
    let mut thread = ta("hello world");

    thread.move_end();

    for _ in 0..5 {
        press(&mut thread, KeyCode::Backspace, KeyModifiers::NONE);
    }

    assert_eq!(thread.text(), "hello ");

    press(&mut thread, KeyCode::Char('z'), KeyModifiers::CONTROL);

    assert_eq!(thread.text(), "hello world");
    assert!(thread.undo.is_empty());
}

#[test]
fn noop_backspace_adds_0_steps() {
    let mut thread = TextArea::new(false, "");

    press(&mut thread, KeyCode::Backspace, KeyModifiers::NONE);
    press(&mut thread, KeyCode::Delete, KeyModifiers::NONE);
    press(&mut thread, KeyCode::Char('u'), KeyModifiers::CONTROL);

    assert!(thread.undo.is_empty());
}

#[test]
fn set_text_resets_history() {
    let mut thread = TextArea::new(false, "");

    typed(&mut thread, "draft");
    thread.set_text("");
    press(&mut thread, KeyCode::Char('z'), KeyModifiers::CONTROL);

    assert_eq!(thread.text(), "");
}

#[test]
fn load_text_is_1_undo_step() {
    let mut thread = TextArea::new(false, "");

    typed(&mut thread, "draft");
    thread.load_text("older prompt");
    typed(&mut thread, "!");

    assert_eq!(thread.text(), "older prompt!");

    press(&mut thread, KeyCode::Char('z'), KeyModifiers::CONTROL);

    assert_eq!(thread.text(), "older prompt");

    press(&mut thread, KeyCode::Char('z'), KeyModifiers::CONTROL);

    assert_eq!(thread.text(), "draft");
}
