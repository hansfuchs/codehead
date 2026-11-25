use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};
use unicode_width::UnicodeWidthStr;

use super::{Filter, PrList, RowKind, gh, state_mark, theme};

pub(super) fn draw_now(list: &mut PrList, frame: &mut Frame, area: Rect, tabs_on: bool, focused: bool) {
    list.rows.clear();

    if area.height < 2 {
        return;
    }

    let (rows, requests) = list.layout();

    list.cursor = list.cursor.min(requests.len().saturating_sub(1));
    draw_header(list, frame, area, requests.len());
    draw_tabs(list, frame, area, tabs_on);
    let area = Rect {
        y: area.y + 3,
        height: area.height.saturating_sub(3),
        ..area
    };

    if rows.is_empty() {
        let message = if list.read.receiver.is_some() {
            "  reading…"
        } else {
            "  0 pull requests"
        };

        frame.render_widget(Paragraph::new(Line::styled(message, theme::muted())), area);
        return;
    }

    scroll_to_cursor(list, &rows, &requests, usize::from(area.height));

    for (index, row) in rows.iter().enumerate().skip(list.offset).take(usize::from(area.height)) {
        let y = area.y + (index - list.offset) as u16;
        let rect = Rect { y, height: 1, ..area };

        draw_row(list, frame, row, &requests, rect, focused);
    }
}

fn draw_header(list: &PrList, frame: &mut Frame, area: Rect, count: usize) {
    let state = if list.read.receiver.is_some() {
        "reading GitHub…".to_string()
    } else if let Some(error) = &list.read.error {
        format!("gh: {error}")
    } else {
        list.read
            .at
            .map(|at| format!("read {}s ago", at.elapsed().as_secs()))
            .unwrap_or_default()
    };

    let head = Line::from(vec![
        Span::styled(format!("your pull requests · {count} "), theme::bold()),
        Span::styled(format!("· {state} · r: read again · enter: open"), theme::muted()),
    ]);

    frame.render_widget(Paragraph::new(head), Rect { height: 1, ..area });
}

fn draw_tabs(list: &mut PrList, frame: &mut Frame, area: Rect, active: bool) {
    list.tabs.i = Filter::ALL
        .iter()
        .position(|filter| *filter == list.filter)
        .unwrap_or(0);
    let labels: Vec<String> = Filter::ALL.iter().map(|filter| filter.label().to_string()).collect();

    list.tabs.draw(
        frame,
        Rect {
            y: area.y + 1,
            height: 1,
            ..area
        },
        "state",
        &labels,
        active,
    );
}

fn scroll_to_cursor(list: &mut PrList, rows: &[RowKind], requests: &[usize], height: usize) {
    let cursor = requests
        .get(list.cursor)
        .and_then(|&index| rows.iter().position(|row| *row == RowKind::Pr(index)))
        .unwrap_or(0);

    // a key: the cursor in view; a mouse scroll: the view stays where the wheel put it
    if !list.free {
        if cursor < list.offset + 1 {
            list.offset = cursor.saturating_sub(1);
        } else if cursor >= list.offset + height {
            list.offset = cursor + 1 - height;
        }
    }

    list.offset = list.offset.min(rows.len().saturating_sub(height));
}

fn draw_row(list: &mut PrList, frame: &mut Frame, row: &RowKind, requests: &[usize], rect: Rect, focused: bool) {
    match row {
        RowKind::Repo(name, count) => {
            let line = Line::from(vec![
                Span::styled(format!(" {name}"), theme::bold()),
                Span::styled(format!("  {count}"), theme::muted()),
            ]);

            frame.render_widget(Paragraph::new(line), rect);
        }
        RowKind::Pr(index) => {
            let picked = requests.iter().position(|request| request == index).unwrap_or(0);

            draw_request(list, frame, *index, picked, rect, focused);
        }
    }
}

fn draw_request(list: &mut PrList, frame: &mut Frame, index: usize, picked: usize, rect: Rect, focused: bool) {
    let entry = &list.entries[index];
    let (mark, color) = state_mark(entry.item.state);

    let thread = entry
        .thread
        .as_ref()
        .map(|thread| {
            format!(
                "  ⧉ {}",
                crate::ui::thread_card::fit(thread.title.lines().next().unwrap_or_default(), 28)
            )
        })
        .unwrap_or_default();

    let right = format!("{}{thread} ", gh::stamp(entry.item.created));
    let number = format!("   #{:<6}", entry.item.pr.number);
    let room = usize::from(rect.width).saturating_sub(number.width() + right.width() + 4);
    let title = crate::ui::thread_card::fit(&entry.item.title, room);
    let pad = room.saturating_sub(title.width());
    let current = picked == list.cursor;

    let base = if current && focused {
        theme::pill()
    } else {
        Style::default()
    };

    let line = Line::from(vec![
        Span::styled(number, base.fg(theme::pal().subtext0)),
        Span::styled(format!("{mark} "), base.fg(color)),
        Span::styled(
            title,
            if current {
                base.add_modifier(Modifier::BOLD)
            } else {
                base.fg(theme::pal().text)
            },
        ),
        Span::styled(" ".repeat(pad + 1), base),
        Span::styled(right, base.fg(theme::pal().subtext0)),
    ]);

    frame.render_widget(Paragraph::new(line).style(base), rect);
    list.rows.push((rect, picked));
}
