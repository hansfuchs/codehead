use super::*;
#[test]
fn decode_resolves() {
    use ratatui::{style::Style, text::Span};
    let line = || vec![Span::raw("  GIGAS")];

    let text = |value: Vec<Span<'static>>| {
        value
            .iter()
            .map(|text_value| text_value.content.to_string())
            .collect::<String>()
    };

    assert_eq!(text(decode(line(), 1.0, Style::default(), Style::default())), "  GIGAS");
    assert_eq!(text(decode(line(), 0.0, Style::default(), Style::default())), "  ·····");
}
