use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
pub(crate) fn key_name(key: &KeyEvent) -> String {
    let mut text = String::new();

    if key.modifiers.contains(KeyModifiers::CONTROL) {
        text.push_str("ctrl+");
    }

    if key.modifiers.contains(KeyModifiers::ALT) {
        text.push_str("alt+");
    }

    match key.code {
        // the case stays: D is not d (shift+d never deletes)
        KeyCode::Char(character) => text.push(character),
        KeyCode::Esc => text.push_str("esc"),
        KeyCode::Enter => text.push_str("enter"),
        _ => text.push('?'),
    }

    text
}
