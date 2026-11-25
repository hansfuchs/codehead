use super::*;
use ratatui::layout::Rect;

fn buf(lines: &[&str]) -> Buffer {
    let width = lines.iter().map(|line| line.chars().count()).max().unwrap_or(0) as u16;
    let mut screen = Buffer::empty(Rect::new(0, 0, width, lines.len() as u16));

    for (y, line) in lines.iter().enumerate() {
        screen.set_string(0, y as u16, line, ratatui::style::Style::default());
    }

    screen
}

#[test]
fn a_url_in_text_in_parens_and_cut_over_rows() {
    let screen = buf(&["see (https://x.dev/a_b?c=1). ok", "plain words here"]);

    assert_eq!(
        at(&screen, Position { x: 10, y: 0 }).as_deref(),
        Some("https://x.dev/a_b?c=1")
    );
    assert_eq!(at(&screen, Position { x: 2, y: 0 }), None);
    assert_eq!(at(&screen, Position { x: 2, y: 1 }), None);

    // a URL cut at the row end, the rest on the next row after a margin
    let wrapped = buf(&["│ go to https://github.com/owner/re", "│ po/pull/12 now"]);

    assert_eq!(
        at(&wrapped, Position { x: 12, y: 0 }).as_deref(),
        Some("https://github.com/owner/repo/pull/12")
    );
    assert_eq!(
        at(&wrapped, Position { x: 4, y: 1 }).as_deref(),
        Some("https://github.com/owner/repo/pull/12")
    );

    // the next row: a new sentence, 0 join
    let short = buf(&["read https://x.dev", "and more"]);

    assert_eq!(at(&short, Position { x: 8, y: 0 }).as_deref(), Some("https://x.dev"));
}
