use super::*;
use ratatui::crossterm::event::KeyModifiers;

fn ev(kind: MouseEventKind, x: u16, y: u16) -> MouseEvent {
    MouseEvent {
        kind,
        column: x,
        row: y,
        modifiers: KeyModifiers::NONE,
    }
}

#[test]
fn a_drag_copies_the_cells_and_clicks_a_word_or_the_row() {
    let area = Rect::new(0, 0, 20, 3);
    let mut buf = Buffer::empty(area);

    buf.set_string(0, 0, "hello world", ratatui::style::Style::default());
    buf.set_string(0, 1, "second row here", ratatui::style::Style::default());
    let mut text = ScreenSel::default();

    assert_eq!(
        text.mouse(ev(MouseEventKind::Down(MouseButton::Left), 6, 0), area, &buf),
        None
    );

    text.mouse(ev(MouseEventKind::Drag(MouseButton::Left), 5, 1), area, &buf);

    assert_eq!(
        text.mouse(ev(MouseEventKind::Up(MouseButton::Left), 5, 1), area, &buf)
            .as_deref(),
        Some("world\nsecond")
    );

    // 2 clicks: the word
    text.mouse(ev(MouseEventKind::Down(MouseButton::Left), 9, 1), area, &buf);
    text.mouse(ev(MouseEventKind::Up(MouseButton::Left), 9, 1), area, &buf);

    assert_eq!(
        text.mouse(ev(MouseEventKind::Down(MouseButton::Left), 9, 1), area, &buf)
            .as_deref(),
        Some("row")
    );

    // 3 clicks: the row
    assert_eq!(
        text.mouse(ev(MouseEventKind::Down(MouseButton::Left), 9, 1), area, &buf)
            .as_deref(),
        Some("second row here")
    );

    // the paint: accent cells of the row
    text.paint(area, &mut buf);

    assert_eq!(buf[(0, 1)].bg, theme::pal().accent);
    assert_ne!(buf[(0, 0)].bg, theme::pal().accent);
}
