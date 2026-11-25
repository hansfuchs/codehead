use super::*;

fn ta(text: &str) -> TextArea {
    let mut input = TextArea::new(false, "");

    input.set_text(text);
    input
}

fn press(input: &mut TextArea, code: KeyCode, modifiers: KeyModifiers) -> bool {
    matches!(input.key(KeyEvent::new(code, modifiers)), Outcome::Handled)
}

fn typed(input: &mut TextArea, text: &str) {
    for character in text.chars() {
        press(input, KeyCode::Char(character), KeyModifiers::NONE);
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
        let mut input = TextArea::new(false, "");

        typed(&mut input, "hi");
        press(&mut input, undo.0, undo.1);

        assert_eq!(input.text(), "");

        press(&mut input, redo.0, redo.1);

        assert_eq!(input.text(), "hi");
    }
}

#[test]
fn cmd_char_never_types() {
    let mut input = ta("x");

    for character in ['q', 'e', 'w', 'u', 'k'] {
        assert!(!press(&mut input, KeyCode::Char(character), KeyModifiers::SUPER));
    }

    assert_eq!(input.text(), "x");

    press(&mut input, KeyCode::Char('a'), KeyModifiers::SUPER);

    assert_eq!(input.selected_text().as_deref(), Some("x"));
}

#[test]
fn backspaces_undo_as_1_step() {
    let mut input = ta("hello world");

    input.move_end();

    for _ in 0..5 {
        press(&mut input, KeyCode::Backspace, KeyModifiers::NONE);
    }

    assert_eq!(input.text(), "hello ");

    press(&mut input, KeyCode::Char('z'), KeyModifiers::CONTROL);

    assert_eq!(input.text(), "hello world");
    assert!(input.undo.is_empty());
}

#[test]
fn noop_backspace_adds_0_steps() {
    let mut input = TextArea::new(false, "");

    press(&mut input, KeyCode::Backspace, KeyModifiers::NONE);
    press(&mut input, KeyCode::Delete, KeyModifiers::NONE);
    press(&mut input, KeyCode::Char('u'), KeyModifiers::CONTROL);

    assert!(input.undo.is_empty());
}

#[test]
fn set_text_resets_history() {
    let mut input = TextArea::new(false, "");

    typed(&mut input, "draft");
    input.set_text("");
    press(&mut input, KeyCode::Char('z'), KeyModifiers::CONTROL);

    assert_eq!(input.text(), "");
}

#[test]
fn load_text_is_1_undo_step() {
    let mut input = TextArea::new(false, "");

    typed(&mut input, "draft");
    input.load_text("older prompt");
    typed(&mut input, "!");

    assert_eq!(input.text(), "older prompt!");

    press(&mut input, KeyCode::Char('z'), KeyModifiers::CONTROL);

    assert_eq!(input.text(), "older prompt");

    press(&mut input, KeyCode::Char('z'), KeyModifiers::CONTROL);

    assert_eq!(input.text(), "draft");
}
