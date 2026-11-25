use crate::ui::theme;
use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
};

pub(super) fn draw_frame(frame: &mut Frame, area: Rect, title: &str, hint: &str) -> Rect {
    frame.render_widget(Clear, area);
    let block = Block::new()
        .borders(Borders::ALL)
        .border_type(BorderType::Plain)
        .border_style(theme::accent())
        .title(Span::styled(title, theme::accent_bold()))
        .title_bottom(Line::styled(hint, theme::muted()).right_aligned());
    let inner = block.inner(area);

    frame.render_widget(block, area);
    Rect {
        x: inner.x + 1,
        width: inner.width.saturating_sub(2),
        ..inner
    }
}

pub(super) fn draw_lines(frame: &mut Frame, area: Rect, lines: Vec<Line<'static>>, scroll: &mut usize, style: Style) {
    *scroll = (*scroll).min(lines.len().saturating_sub(usize::from(area.height)));
    frame.render_widget(Paragraph::new(lines).scroll((*scroll as u16, 0)).style(style), area);
}

pub(super) fn truncate(text: String, width: usize) -> String {
    if text.chars().count() > width {
        text.chars().take(width.saturating_sub(1)).collect::<String>() + "…"
    } else {
        text
    }
}
