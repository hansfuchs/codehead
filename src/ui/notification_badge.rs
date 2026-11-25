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

    for count_value in notes {
        match by.iter_mut().find(|x| x.0 == count_value.kind) {
            Some(x) => x.1 += count_value.count(),
            None => by.push((count_value.kind, count_value.count())),
        }
    }

    by.sort_by_key(|x| x.0);
    let mut text = Vec::new();

    for (key, count_2) in by {
        text.push(Span::styled(
            format!("{} {} {count_2}", key.icon(), key.label()),
            Style::default().fg(key.color()).add_modifier(Modifier::BOLD),
        ));
        text.push(Span::raw("  "));
    }

    text.push(Span::styled("ctrl+n ", theme::muted()));

    text
}

pub(crate) fn width(spans: &[Span]) -> u16 {
    spans
        .iter()
        .map(|text| unicode_width::UnicodeWidthStr::width(text.content.as_ref()) as u16)
        .sum()
}

/// The note of the highest kind of thread ID, on the top border of its card R (the threads tab).
pub(crate) fn card_badge(buf: &mut ratatui::buffer::Buffer, area: Rect, notes: &[Note], id: &str) {
    use ratatui::widgets::Widget;

    let Some(count_3) = notes
        .iter()
        .filter(|count_value| count_value.thread == id)
        .min_by_key(|count_2| count_2.kind)
    else {
        return;
    };

    let more = notes.iter().filter(|x| x.thread == id).count() - 1;

    let text = if more > 0 {
        format!(" {} {} +{more} ", count_3.kind.icon(), count_3.kind.label())
    } else {
        format!(" {} {} ", count_3.kind.icon(), count_3.kind.label())
    };

    let width_value = unicode_width::UnicodeWidthStr::width(text.as_str()) as u16;

    if area.width < width_value + 8 {
        return;
    }

    let at = Rect {
        x: area.right() - width_value - 5,
        y: area.y,
        width: width_value,
        height: 1,
    };

    Paragraph::new(Span::styled(
        text,
        Style::default().fg(count_3.kind.color()).add_modifier(Modifier::BOLD),
    ))
    .render(at, buf);
}
