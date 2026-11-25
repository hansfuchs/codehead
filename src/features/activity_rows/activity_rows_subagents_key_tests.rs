use super::*;
use ratatui::crossterm::event::KeyModifiers;

#[test]
fn another_key_closes_the_box_and_goes_on_d_asks_first() {
    let mut text = Subagents {
        open: true,
        ..Default::default()
    };

    assert!(
        text.key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE), &[])
            .is_none(),
        "x: not the box's"
    );
    assert!(!text.open);

    text.open = true;

    assert!(matches!(
        text.key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE), &[]),
        Some(Act::None)
    ));
    assert!(text.armed && text.open);
}
