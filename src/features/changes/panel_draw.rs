use super::{Block, Frame, Line, PRow, Paragraph, Rect, Span, View, theme};

impl View {
    /// The previous / next section label from the cursor (DOWN: next).
    pub(super) fn jump_section(&mut self, down: bool) {
        let secs: Vec<usize> = (0..self.scm.rows.len())
            .filter(|&i| matches!(self.scm.rows[i], PRow::Section(..)))
            .collect();
        let at = self.scm.at;

        let to = if down {
            secs.iter().copied().find(|&i| i > at)
        } else {
            secs.iter().rev().copied().find(|&i| i < at)
        };

        if let Some(i) = to {
            self.scm.at = i;
        }
    }

    pub(in crate::features::changes) fn draw_panel(&mut self, frame: &mut Frame, area: Rect) {
        draw_panel_view(self, frame, area);
    }
}

/// The labels stuck to the top and to the bottom of a list of HEIGHT rows from TOP.
fn stuck(top: usize, height: usize, view: &View) -> (Vec<usize>, Vec<usize>) {
    let labels: Vec<usize> = (0..view.scm.rows.len())
        .filter(|&i| view.scm.rows[i].label().is_some())
        .collect();
    let (mut above, mut below): (Vec<usize>, Vec<usize>) = (Vec::new(), Vec::new());

    for _ in 0..=labels.len() {
        let next_above: Vec<usize> = labels.iter().copied().filter(|&i| i < top + above.len()).collect();
        let end = (top + height).saturating_sub(below.len());

        // rows top + ABOVE .. END show in place; a label at or after END: the bottom stack
        let next_below: Vec<usize> = labels
            .iter()
            .copied()
            .filter(|&i| i >= end && i >= top + next_above.len())
            .collect();

        if next_above == above && next_below == below {
            break;
        }

        above = next_above;
        below = next_below;

        if above.len() + below.len() >= height {
            break;
        }
    }

    (above, below)
}

/// The cursor row in view (labels: always in view, stuck or not).
fn panel_scroll(height: usize, view: &mut View) {
    let count = view.scm.rows.len();

    view.scm.at = view.scm.at.min(count.saturating_sub(1));
    view.scm.top = view.scm.top.min(count.saturating_sub(1));

    for _ in 0..4 {
        let (above, below) = stuck(view.scm.top, height, view);
        let at = view.scm.at;

        if view.scm.rows.get(at).is_some_and(|row| row.label().is_some())
            && (above.contains(&at) || below.contains(&at))
        {
            break;
        }

        let (lo, hi) = (
            view.scm.top + above.len(),
            (view.scm.top + height).saturating_sub(below.len()),
        );

        if at < lo {
            view.scm.top = at.saturating_sub(above.len());
        } else if at >= hi {
            view.scm.top = (at + 1 + below.len()).saturating_sub(height);
        } else {
            break;
        }
    }

    // 0 empty room below the last row
    view.scm.top = view.scm.top.min(count.saturating_sub(height));
}

/// The panel's list in LIST.
fn draw_rows(frame: &mut Frame, list: Rect, view: &mut View) {
    view.sync_rows();
    let height = usize::from(list.height);

    if height == 0 {
        return;
    }

    panel_scroll(height, view);
    let (above, below) = stuck(view.scm.top, height, view);
    let mut shown: Vec<(usize, bool)> = above.iter().map(|&i| (i, true)).collect();
    let body = height.saturating_sub(above.len() + below.len());

    shown.extend(
        (view.scm.top + above.len()..view.scm.rows.len())
            .take(body)
            .map(|i| (i, false)),
    );
    shown.truncate(height.saturating_sub(below.len()));
    shown.extend(below.iter().map(|&i| (i, true)));
    let width = usize::from(list.width);

    draw_panel_rows(frame, list, view, shown, width);
}

fn draw_panel_rows(frame: &mut Frame<'_>, list: Rect, view: &mut View, shown: Vec<(usize, bool)>, width: usize) {
    for (offset, (i, stuck)) in shown.into_iter().enumerate() {
        draw_panel_row(frame, list, view, width, offset, i, stuck);
    }
}

fn draw_panel_row(
    frame: &mut Frame<'_>,
    list: Rect,
    view: &mut View,
    width: usize,
    offset: usize,
    i: usize,
    stuck: bool,
) {
    let y = list.y + offset as u16;
    let rect = Rect { y, height: 1, ..list };
    let row = view.scm.rows[i].clone();
    let line = view.panel_line(&row, width, rect);

    frame.render_widget(Paragraph::new(line), rect);
    let buf = frame.buffer_mut();

    if stuck {
        for x in rect.x..rect.right() {
            buf[(x, y)].set_bg(theme::pal().surface0);
        }
    }

    let picked = picked(view, &row);

    if picked {
        theme::picked(buf, rect, Some(rect.x), false);
    }

    if i == view.scm.at {
        let area = Rect {
            x: rect.x + 1,
            width: rect.width.saturating_sub(1),
            ..rect
        };

        if view.scm.focus {
            theme::cursor_row(buf, area);
        } else {
            theme::hover(buf, area);
        }

        if view.scm.focus {
            buf[(rect.x, y)].set_symbol("▌").set_fg(theme::pal().accent);
        }
    }

    view.scm.lines.push((y, i, stuck));
}

fn picked(view: &mut View, row: &PRow) -> bool {
    match row {
        PRow::File(fi, _) => view
            .changes
            .files
            .get(*fi)
            .is_some_and(|file| view.scm.picked.contains(&file.path)),
        PRow::Commit(ci) => view
            .changes
            .commits
            .list
            .get(*ci)
            .is_some_and(|commit| view.scm.commit_picks.contains(&commit.oid)),
        PRow::Comment(ci) => view
            .comment_state
            .comments
            .get(*ci)
            .is_some_and(|comment| view.scm.comment_picks.contains(&comment.id)),
        // a group label: every item in it picked
        group if group.label().is_some() || matches!(group, PRow::CommentFile(..)) => view.group_picked(group),
        _ => false,
    }
}

fn poll_draft(view: &mut View) {
    if let Some(rx) = &view.scm.drafting
        && let Ok(draft) = rx.try_recv()
    {
        view.scm.drafting = None;

        match draft {
            Ok(message) => view.scm.msg.set_text(&message),
            Err(error) => view.operation_state.note = Some(format!("draft: {error}")),
        }
    }
}

fn draw_panel_view(view: &mut View, frame: &mut Frame, area: Rect) {
    view.scm.rect = area;
    view.scm.buttons.clear();
    view.scm.lines.clear();

    poll_draft(view);

    if area.height < 6 || area.width < 12 {
        return;
    }

    if view.pr_src.is_some() {
        view.scm.msg_rect = Rect::default();
        draw_rows(frame, area, view);
        view.draw_pick(frame, area);
        return;
    }

    // the commit message box: 1-5 text rows, a hint row under it
    let inner_w = area.width.saturating_sub(2);
    let rows = (view.scm.msg.rows_for(inner_w) as u16).clamp(1, 5);
    let boxr = Rect {
        height: rows + 2,
        ..area
    };
    let edge = theme::border(view.scm.msg_focus);
    let count = view.changes.staged.len();

    let title = if view.scm.drafting.is_some() {
        " drafting… ".to_string()
    } else {
        format!(" commit · {count} staged ")
    };

    frame.render_widget(
        Block::bordered()
            .border_style(edge)
            .title(Span::styled(title, theme::muted())),
        boxr,
    );
    view.scm.msg_rect = Rect {
        x: boxr.x + 1,
        y: boxr.y + 1,
        width: inner_w,
        height: rows,
    };
    let focused = view.scm.msg_focus && view.modal_state.modal.is_none();

    view.scm.msg.render(frame, view.scm.msg_rect, focused);
    let hint = Rect {
        y: boxr.bottom(),
        height: 1,
        ..area
    };

    frame.render_widget(
        Paragraph::new(Line::styled(
            crate::ui::thread_card::fit(" ctrl+⏎ commit · ctrl+g draft", area.width as usize),
            theme::muted(),
        )),
        hint,
    );
    let list = Rect {
        y: hint.bottom() + 1,
        height: area.bottom().saturating_sub(hint.bottom() + 1),
        ..area
    };

    draw_rows(frame, list, view);
}
