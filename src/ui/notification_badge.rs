use ratatui::widgets::Paragraph;

use crate::domain::notifications::{Kind, Note};
use crate::ui::theme;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::Span;
/// Header counts of NOTES: `✓ done 2  ⏸ blocked 1  ctrl+n`; empty: 0 notes.
pub(crate) fn badge(notes: &[Note]) -> Vec<Span<'static>> {
    if notes.is_empty() {
        return Vec::new();
    }

    let mut by: Vec<(Kind, usize)> = Vec::new();

    for note in notes {
        match by.iter_mut().find(|entry| entry.0 == note.kind) {
            Some(entry) => entry.1 += note.count(),
            None => by.push((note.kind, note.count())),
        }
    }

    by.sort_by_key(|entry| entry.0);
    let mut spans = Vec::new();

    for (kind, count) in by {
        spans.push(Span::styled(
            format!("{} {} {count}", kind.icon(), kind.label()),
            Style::default().fg(kind.color()).add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::raw("  "));
    }

    spans.push(Span::styled("ctrl+n ", theme::muted()));

    spans
}

pub(crate) fn width(spans: &[Span]) -> u16 {
    spans
        .iter()
        .map(|span| unicode_width::UnicodeWidthStr::width(span.content.as_ref()) as u16)
        .sum()
}

/// The note of the highest kind of thread ID, on the top border of its card R (the threads tab).
pub(crate) fn card_badge(buf: &mut ratatui::buffer::Buffer, area: Rect, notes: &[Note], id: &str) {
    use ratatui::widgets::Widget;

    let Some(top) = notes
        .iter()
        .filter(|note| note.thread == id)
        .min_by_key(|note| note.kind)
    else {
        return;
    };

    let more = notes.iter().filter(|note| note.thread == id).count() - 1;

    let text = if more > 0 {
        format!(" {} {} +{more} ", top.kind.icon(), top.kind.label())
    } else {
        format!(" {} {} ", top.kind.icon(), top.kind.label())
    };

    let width = unicode_width::UnicodeWidthStr::width(text.as_str()) as u16;

    if area.width < width + 8 {
        return;
    }

    let at = Rect {
        x: area.right() - width - 5,
        y: area.y,
        width,
        height: 1,
    };

    Paragraph::new(Span::styled(
        text,
        Style::default().fg(top.kind.color()).add_modifier(Modifier::BOLD),
    ))
    .render(at, buf);
}
