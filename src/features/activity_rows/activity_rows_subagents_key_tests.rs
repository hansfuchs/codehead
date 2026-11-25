use super::*;
use ratatui::crossterm::event::KeyModifiers;

#[test]
fn another_key_closes_the_box_and_goes_on_d_asks_first() {
    let mut subagents = Subagents {
        open: true,
        ..Default::default()
    };

    assert!(
        subagents
            .key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE), &[])
            .is_none(),
        "x: not the box's"
    );
    assert!(!subagents.open);

    subagents.open = true;

    assert!(matches!(
        subagents.key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE), &[]),
        Some(Act::None)
    ));
    assert!(subagents.armed && subagents.open);
}
