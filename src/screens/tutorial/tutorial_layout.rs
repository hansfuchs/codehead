use super::{
    BACK, Duration, Line, MIN_H, MIN_W, PANEL_FROM, PANEL_MAX, PANEL_MIN, Paragraph, QUIT, Rect, Span, Style, Terminal,
    VIEW_MAX, theme,
};
use ratatui::crossterm::event::{Event as Ev, KeyCode, KeyEventKind, KeyModifiers};
use std::ops::ControlFlow;

/// The rects in a terminal of W x H: the view (inside its frame), the quest panel (None: too
/// narrow).
pub(super) fn layout(width_value: u16, height_value: u16) -> (Rect, Option<Rect>) {
    let pw = if width_value >= PANEL_FROM {
        (width_value / 3).clamp(PANEL_MIN, PANEL_MAX)
    } else {
        0
    };

    let room = width_value.saturating_sub(4 + 2 + if pw > 0 { pw + 2 } else { 0 });
    let vw = room.min(VIEW_MAX);
    let group = vw + 2 + if pw > 0 { pw + 2 } else { 0 };
    let x0 = width_value.saturating_sub(group) / 2;
    let view = Rect {
        x: x0 + 1,
        y: 4,
        width: vw,
        height: height_value.saturating_sub(7),
    };
    let side = (pw > 0).then_some(Rect {
        x: x0 + vw + 4,
        y: 3,
        width: pw,
        height: view.height + 2,
    });

    (view, side)
}

/// The terminal's size now.
pub(super) fn size() -> (u16, u16) {
    ratatui::crossterm::terminal::size().unwrap_or((120, 40))
}

/// The whole screen blank, TOP's buffers reset to the size now. Not `Terminal::clear`: it asks
/// the terminal for the cursor (up to 2 s without an answer, keys typed then get lost).
pub(super) fn wipe(top: &mut Terminal<ratatui::backend::CrosstermBackend<crate::ui::frames::Frames>>) {
    let (width, height) = size();
    let _ = top.resize(Rect::new(0, 0, width, height));
}

/// A window under `MIN_W` x `MIN_H`: a screen that asks for a bigger one, until it is (the size is
/// read again on each resize). None: go on (big enough, or enter: anyway); Some(code): esc
/// back, ctrl+c quit.
pub(super) fn size_gate(
    top: &mut Terminal<ratatui::backend::CrosstermBackend<crate::ui::frames::Frames>>,
) -> Option<i32> {
    loop {
        let (width_value, height_value) = size();

        if width_value >= MIN_W && height_value >= MIN_H {
            return None;
        }

        let _ = top.draw(|area_value| {
            let full = area_value.area();

            area_value.render_widget(ratatui::widgets::Clear, full);
            let lines = vec![
                Line::from(Span::styled("the tutorial needs room", theme::accent_bold())),
                Line::from(""),
                Line::from(vec![
                    Span::styled("this window: ", theme::muted()),
                    Span::styled(
                        format!("{width_value} × {height_value}"),
                        theme::fg(theme::pal().caution()),
                    ),
                    Span::styled(format!(" · best from {MIN_W} × {MIN_H}"), theme::muted()),
                ]),
                Line::from(Span::styled("make the terminal bigger, or full screen.", theme::text())),
                Line::from(Span::styled("it goes on by itself once it fits.", theme::muted())),
                Line::from(""),
                Line::from(vec![
                    Span::styled("enter", theme::bold()),
                    Span::styled(" go on anyway  ", theme::muted()),
                    Span::styled("esc", theme::bold()),
                    Span::styled(" back  ", theme::muted()),
                    Span::styled("ctrl+c", theme::bold()),
                    Span::styled(" quit", theme::muted()),
                ]),
            ];
            let bw = lines
                .iter()
                .map(|line| line.width() as u16)
                .max()
                .unwrap_or(0)
                .min(full.width);
            let bh = (lines.len() as u16).min(full.height);
            let area_2 = Rect {
                x: full.width.saturating_sub(bw) / 2,
                y: full.height.saturating_sub(bh) / 2,
                width: bw,
                height: bh,
            };

            area_value.render_widget(Paragraph::new(lines), area_2);
        });

        // codehead's own reader (the one stdin reader of the process). Err: a signal or the
        // terminal gone, the tutorial ends
        let Ok(batch) = crate::infra::terminal::read_ready(Duration::from_millis(200)) else {
            return Some(QUIT);
        };

        if let ControlFlow::Break(value) = batch
            .into_iter()
            .try_for_each(|(event, _)| handle_size_event(top, &event))
        {
            return value;
        }
    }
}

fn handle_size_event(
    top: &mut Terminal<ratatui::prelude::CrosstermBackend<crate::ui::frames::Frames>>,
    ev: &ratatui::crossterm::event::Event,
) -> ControlFlow<Option<i32>> {
    match ev {
        Ev::Key(key) if key.kind != KeyEventKind::Release => match key.code {
            KeyCode::Enter => return ControlFlow::Break(None),
            KeyCode::Esc => return ControlFlow::Break(Some(BACK)),
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                return ControlFlow::Break(Some(QUIT));
            }
            _ => {}
        },
        Ev::Resize(..) => {
            wipe(top);
        }
        _ => {}
    }

    ControlFlow::Continue(())
}

/// TEXT in STYLE wrapped at word ends to WIDTH columns: HEAD before the 1st row, blanks of its
/// width before the rest (a hanging indent; a `│` head repeats).
pub(super) fn wrap(head: &Span<'static>, text: &str, style: Style, width: usize) -> Vec<Line<'static>> {
    use unicode_width::UnicodeWidthStr;
    let hw = head.content.width();

    let rest = if head.content.trim() == "│" {
        head.clone()
    } else {
        Span::raw(" ".repeat(hw))
    };

    let room = width.saturating_sub(hw).max(8);
    let mut rows: Vec<String> = vec![String::new()];

    for width_value in text.split(' ') {
        append_wrapped_word(room, &mut rows, width_value);
    }

    rows.into_iter()
        .enumerate()
        .map(|(i, result)| {
            Line::from(vec![
                if i == 0 { head.clone() } else { rest.clone() },
                Span::styled(result, style),
            ])
        })
        .collect()
}

fn append_wrapped_word(room: usize, rows: &mut Vec<String>, width_value: &str) {
    use unicode_width::UnicodeWidthStr;
    let cur = rows.last_mut().expect("1 row");

    if !cur.is_empty() && cur.width() + 1 + width_value.width() > room {
        rows.push(width_value.to_string());
    } else {
        if !cur.is_empty() {
            cur.push(' ');
        }

        cur.push_str(width_value);
    }
}
