use super::{Block, Borders, Edit, Frame, Line, Merge, Paragraph, Rect, Span, Stop, theme};

pub(super) fn draw_overview(merge: &mut Merge, frame: &mut Frame<'_>, focused: bool, focus: Option<Stop>, body: Rect) {
    let Some(tab) = merge.tab_ref() else { return };

    let Some(pull_request) = tab.pr.clone() else {
        let msg = match &tab.error {
            Some(error_2) => format!("  {error_2}"),
            None => "  reading the pull request…".into(),
        };

        frame.render_widget(Paragraph::new(Line::styled(msg, theme::muted())), body);
        return;
    };

    let inner = Rect {
        x: body.x + 1,
        width: body.width.saturating_sub(2),
        ..body
    };
    let rows = merge.rows(&pull_request, inner.width as usize, focused);
    let focused_row = rows
        .iter()
        .position(|result| result.stops.iter().any(|text_value| Some(text_value.2) == focus));
    let visible_height = inner.height as usize;
    let scroll = scroll_overview(merge, focused_row, &rows, visible_height);
    let input_at = draw_overview_rows(merge, frame, inner, &rows, scroll);

    draw_inline_input(merge, frame, focused, input_at);
}

pub(super) fn draw_inline_input(merge: &mut Merge, frame: &mut Frame<'_>, focused: bool, input_at: Option<Rect>) {
    if let Some(area) = input_at
        && let Some((edit_value, input_value)) = merge.tab().and_then(|tab_value| tab_value.input.edit.as_mut())
    {
        let label = match edit_value {
            Edit::Title => " title · enter: save · esc: drop ",
            Edit::Body => " description · ctrl+⏎: save · esc: drop ",
            Edit::Comment => " new comment · ctrl+⏎: post · esc: drop ",
            Edit::Reply(..) => " reply · ctrl+⏎: post · esc: drop ",
            Edit::Change(..) => " edit comment · ctrl+⏎: save · esc: drop ",
        };

        let block = Block::new()
            .borders(Borders::ALL)
            .border_style(theme::accent())
            .title(Span::styled(label, theme::accent()));
        let inside = block.inner(area);

        frame.render_widget(block, area);
        input_value.render(frame, inside, focused);
        merge.layout.edit_rect = inside;
    }

    merge.draw_menu(frame, focused);
}

pub(super) fn scroll_overview(
    merge: &mut Merge,
    focused_row: Option<usize>,
    rows: &[super::super::Row],
    visible_height: usize,
) -> usize {
    let tab_value = merge.tab().expect("a tab");

    if let Some(i) = focused_row.filter(|_| !tab_value.free) {
        let span = rows
            .iter()
            .skip(i + 1)
            .take_while(|result_value| result_value.input > 0)
            .map(|result_2| result_2.input as usize)
            .sum::<usize>()
            + 1;

        if i < tab_value.scroll {
            tab_value.scroll = i.saturating_sub(1);
        } else if i + span > tab_value.scroll + visible_height {
            tab_value.scroll = (i + span).saturating_sub(visible_height);
        }
    }

    let total: usize = rows.iter().map(|result_3| (result_3.input as usize).max(1)).sum();

    tab_value.scroll = tab_value.scroll.min(total.saturating_sub(visible_height));
    tab_value.scroll
}

fn draw_overview_rows(
    merge: &mut Merge,
    frame: &mut Frame,
    inner: Rect,
    rows: &[super::super::Row],
    scroll: usize,
) -> Option<Rect> {
    let mut y = inner.y;
    let mut input_at: Option<Rect> = None;

    for row in rows.iter().skip(scroll) {
        if y >= inner.bottom() {
            break;
        }

        if row.input > 0 {
            let hgt = row.input.min(inner.bottom() - y);

            input_at = Some(Rect {
                x: inner.x,
                y,
                width: inner.width,
                height: hgt,
            });
            y += hgt;
            continue;
        }

        let rect = Rect { y, height: 1, ..inner };

        frame.render_widget(Paragraph::new(row.line.clone()), rect);

        for &(x0, width_3, text_2) in &row.stops {
            merge.layout.hits.push((
                Rect {
                    x: inner.x + x0,
                    y,
                    width: width_3.min(inner.width.saturating_sub(x0)),
                    height: 1,
                },
                text_2,
            ));
        }

        y += 1;
    }

    input_at
}
