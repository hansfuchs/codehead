use super::*;
#[test]
fn decode_resolves() {
    use ratatui::{style::Style, text::Span};
    let line = || vec![Span::raw("  GIGAS")];

    let content = |spans: Vec<Span<'static>>| spans.iter().map(|span| span.content.to_string()).collect::<String>();

    assert_eq!(
        content(decode(line(), 1.0, Style::default(), Style::default())),
        "  GIGAS"
    );
    assert_eq!(
        content(decode(line(), 0.0, Style::default(), Style::default())),
        "  ·····"
    );
}
