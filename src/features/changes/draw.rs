use super::{Body, Frame, Line, PANEL_MIN, Paragraph, Rect, Span, View, code_bg, theme};

impl View {
    /// The view in AREA: a read's new diffs dissolve in.
    pub(crate) fn draw(&mut self, frame: &mut Frame, area: Rect) {
        self.draw_now(frame, area);
        self.layout.fx.apply(frame, area, self.layout.version);
    }

    pub(super) fn draw_now(&mut self, frame: &mut Frame, area: Rect) {
        draw_now_view(self, frame, area);
    }
}

/// The summary row and the filter row (they stay while the rows scroll).
fn draw_bars(frame: &mut Frame, view: &mut View) {
    frame.render_widget(Paragraph::new(view.summary()), view.layout.summary_rect);
    let filtered = !view.filter_state.filter.trim().is_empty();

    if view.filter_state.filtering {
        // pill, the input, the hint at the right
        let result = view.layout.filter_rect;
        let hint = "  enter: 1st hit · esc: clear";
        let hw = (hint.chars().count() as u16).min(result.width.saturating_sub(16));

        frame.render_widget(
            Paragraph::new(Line::from(vec![Span::styled(" f ", theme::pill()), Span::raw(" ")])),
            result,
        );
        let input = Rect {
            x: result.x + 4,
            width: result.width.saturating_sub(4 + hw),
            ..result
        };

        view.filter_state.filter_ta.render(frame, input, true);
        frame.render_widget(
            Paragraph::new(Span::styled(hint, theme::muted())),
            Rect {
                x: input.right(),
                width: hw,
                ..result
            },
        );
        return;
    }

    let line = if filtered {
        Line::from(vec![
            Span::styled(" f ", theme::muted()),
            Span::styled(format!(" {}", view.filter_state.filter), theme::accent()),
            Span::styled("  ctrl+f: edit · esc: clear", theme::muted()),
        ])
    } else {
        Line::styled(" f  filter by name or content", theme::muted())
    };

    frame.render_widget(Paragraph::new(line), view.layout.filter_rect);
}

/// The rows pinned on top while row TOP shows: its box's head (+ the why under it), once
/// the head scrolled out of view. 0: TOP is a head, between the boxes or after one.
fn pinned(top: usize, view: &View) -> Vec<usize> {
    let Some(row) = view.layout.rows.get(top) else {
        return Vec::new();
    };

    if row.head
        || matches!(
            row.body,
            Body::Blank | Body::Group(..) | Body::Hits(_) | Body::Conflict | Body::Foot
        )
    {
        return Vec::new();
    }

    let Some(hi) = view.layout.rows[..top]
        .iter()
        .rposition(|x| x.head && x.file == row.file)
    else {
        return Vec::new();
    };

    let mut value = vec![hi];

    if view.layout.rows.get(hi + 1).is_some_and(|x| x.body == Body::Why) {
        value.push(hi + 1);
    }

    value
}

/// Row I of the rows in RECT: its line, the selection's tint, the code background, the cursor.
fn draw_row(frame: &mut Frame, rect: Rect, i: usize, x0: u16, width: u16, view: &View) {
    frame.render_widget(Paragraph::new(view.line(i)), rect);
    let buf = frame.buffer_mut();

    if view.selected(i) {
        // a selected line: a bar in the gutter, its text on the selection tint (the sign
        // column keeps its color)
        let area = Rect {
            x: rect.x + 1,
            width: rect.width.saturating_sub(3),
            ..rect
        };

        theme::picked(buf, area, Some(rect.x + 1), true);
    }

    // inside a box: the code's dark background
    if view.layout.rows.get(i).is_some_and(|comment| {
        matches!(
            comment.body,
            Body::Why
                | Body::Note(_)
                | Body::Hunk(_)
                | Body::Diff(..)
                | Body::Side(..)
                | Body::Finding(..)
                | Body::Comment(..)
                | Body::Gh(..)
        )
    }) {
        let bg = code_bg();

        tint_code_background(rect, buf, bg);
    }

    // the cursor line: on the accent, as the cursor row of every list
    if i == view.at && !view.scm.focus {
        // inside the box: its borders keep their color (the picked box: the accent)
        theme::cursor_row(
            frame.buffer_mut(),
            Rect {
                x: x0 + 1,
                width: width.saturating_sub(2),
                ..rect
            },
        );
        frame.render_widget(
            Paragraph::new(Span::styled("▌", theme::accent())),
            Rect {
                x: x0 + width,
                width: 1,
                ..rect
            },
        );
    }
}

fn tint_code_background(rect: Rect, buf: &mut ratatui::prelude::Buffer, bg: ratatui::prelude::Color) {
    for x in rect.x + 1..rect.right().saturating_sub(1) {
        if let Some(character) = buf.cell_mut((x, rect.y))
            && character.bg == ratatui::style::Color::Reset
        {
            character.set_bg(bg);
        }
    }
}

fn layout_review_rows(area: Rect, view: &mut View) -> (bool, Rect) {
    let bars = area.height >= 6;

    view.layout.summary_rect = if bars {
        Rect { height: 1, ..area }
    } else {
        Rect::default()
    };

    view.layout.filter_rect = if bars {
        Rect {
            y: area.y + 1,
            height: 1,
            ..area
        }
    } else {
        Rect::default()
    };

    let area = if bars {
        Rect {
            y: area.y + 2,
            height: area.height - 2,
            ..area
        }
    } else {
        area
    };

    view.layout.area = area;
    view.layout.shown_rows.clear();

    (bars, area)
}

fn draw_review_rows(frame: &mut Frame<'_>, bars: bool, area: Rect, msg: Option<String>, width: u16, view: &mut View) {
    if let Some(message) = msg {
        frame.render_widget(Paragraph::new(Line::styled(message, theme::muted())), area);
    } else {
        view.build(width);
        view.show_wanted();
        view.build(width);

        if bars {
            draw_bars(frame, view);
        }

        let height = usize::from(area.height);

        // the cursor in view; a box header 1 row below the top when it can
        if view.at < view.top + 1 {
            view.top = view.at.saturating_sub(1);
        } else if view.at >= view.top + height {
            view.top = view.at + 1 - height;
        }

        view.top = view.top.min(view.layout.rows.len().saturating_sub(height));

        // the cursor's box head pinned on top: the cursor stays below it
        let pin = pinned(view.top, view).len();

        if pin > 0 && view.at < view.top + pin && !view.layout.rows[view.at].head {
            view.top = view.at.saturating_sub(pin);
        }

        let pinned = pinned(view.top, view);

        // only the rows in view get made: O(rows on screen) per frame
        for y in view.top..(view.top + height).min(view.layout.rows.len()) {
            let key = y - view.top;
            let row = pinned.get(key).copied().unwrap_or(y);
            let rect = Rect {
                x: area.x,
                y: area.y + key as u16,
                width,
                height: 1,
            };

            draw_row(frame, rect, row, area.x, width, view);
            view.layout.shown_rows.push(row);
        }

        view.see_replies();
    }
}

fn poll_before_draw(view: &mut View) {
    if view.poll_review() {
        view.layout.built = None;
    }

    if view.poll_ship() {
        view.layout.built = None;
    }

    if view.poll_op() {
        view.layout.built = None;
    }

    if view.poll_push_lock() {
        view.layout.built = None;
    }

    if view.poll_rank() {
        view.layout.built = None;
    }

    if view.poll() {
        view.layout.built = None;
        view.layout.version += 1;
    }

    view.sync_stash(false);
    view.sync_comments();
    view.stale |= view.git_changed();

    if view.stale && view.loading.is_none() {
        view.stale = false;
        view.refresh(true);
    }

    view.refresh(false);
}

fn draw_now_view(view: &mut View, frame: &mut Frame, area: Rect) {
    let full = area;

    poll_before_draw(view);

    // the panel left (ctrl+b; room permitting), a line, the review right
    let panel = view.scm.shown && area.width >= PANEL_MIN;

    let (panel_rect, area) = if panel {
        let pw = (area.width / 4).clamp(30, 46);

        for y in area.y..area.bottom() {
            frame.buffer_mut()[(area.x + pw, y)]
                .set_symbol("│")
                .set_style(theme::line());
        }

        (
            Rect { width: pw, ..area },
            Rect {
                x: area.x + pw + 1,
                width: area.width - pw - 1,
                ..area
            },
        )
    } else {
        view.scm.focus = false;
        view.scm.rect = Rect::default();
        (Rect::default(), area)
    };

    // the panel's commit in the review's place
    if let Some(path) = view.peek.as_mut() {
        path.draw_now(frame, area);

        if panel {
            view.draw_panel(frame, panel_rect);
        }

        view.draw_modal(frame, full);
        return;
    }

    // the summary row and the filter row on top (room permitting), the rows below
    let (bars, area) = layout_review_rows(area, view);

    let msg = if let Some(error) = &view.error {
        Some(format!("  changes: {error}"))
    } else if view.changes.files.is_empty() {
        Some(if view.loaded.is_none() {
            "  reading changes…".to_string()
        } else {
            format!("  0 changes · {}", view.against())
        })
    } else {
        None
    };

    let width = area.width.saturating_sub(1);

    draw_review_rows(frame, bars, area, msg, width, view);

    if panel {
        view.draw_panel(frame, panel_rect);
    }

    view.draw_modal(frame, full);
}
