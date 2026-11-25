use super::{FlowBar, Frame, Line, Paragraph, Rect, Span, Style, Wrap, dur, flow_rail, rail_row, theme};

impl FlowBar {
    pub(super) fn draw_steps(&mut self, frame: &mut Frame, inner: Rect, focus: bool) {
        draw_steps_flow_bar(self, frame, inner, focus);
    }
}

fn finish_draw_steps(frame: &mut Frame<'_>, inner: Rect, detail_h: u16, mut y: u16, view_state: &mut FlowBar) {
    if detail_h > 0
        && let Some(count) = view_state.nodes.get(view_state.popup_state.cur)
    {
        let area = Rect {
            y,
            height: detail_h,
            ..inner
        };
        let mut lines = vec![Line::styled("─".repeat(usize::from(area.width)), theme::line())];
        let head = format!(
            "{} · {}{}",
            count.title,
            count
                .step
                .as_deref()
                .map_or("one-off".to_string(), |text_value| format!("step {text_value}")),
            if count.gate { " · gate" } else { "" }
        );

        lines.push(Line::styled(head, theme::bold()));

        for line_value in crate::domain::shortcuts::expand(&count.text).lines() {
            lines.push(Line::styled(line_value.to_string(), theme::text()));
        }

        if !count.done.is_empty() {
            lines.push(Line::styled(format!("done when: {}", count.done), theme::muted()));
        }

        frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), area);
        y += detail_h;
    }

    for (i, (node, width_2)) in view_state.warnings.iter().take(4).enumerate() {
        let area_value = Rect {
            y: y + i as u16,
            height: 1,
            ..inner
        };

        frame.render_widget(
            Paragraph::new(Line::styled(
                crate::ui::thread_card::fit(&format!("! {}: {width_2}", node + 1), usize::from(area_value.width)),
                Style::default().fg(theme::pal().error()),
            )),
            area_value,
        );
    }
}

fn draw_steps_last(
    st: &crate::engine::flows::run::FlowState,
    i: usize,
    style: &crate::engine::flows::Resolved,
    warnings: &[(usize, String)],
) -> flow_rail::Row {
    let last = st.history.iter().rev().find(|path| path.node == i);

    let (mark, ms) = if i == st.node && !st.finished {
        if st.wait.is_some() {
            ("⏸", theme::bold().fg(theme::pal().attention()))
        } else {
            ("●", theme::bold().fg(theme::pal().progress()))
        }
    } else {
        match last.map(|path_value| path_value.how.as_str()) {
            Some("skipped") => ("⤼", theme::muted()),
            Some("left") => ("·", theme::muted()),
            Some(_) => ("✓", Style::default().fg(theme::pal().success())),
            None => ("○", theme::muted()),
        }
    };

    let mut row = rail_row(style, mark, ms);

    if let Some(path_2) = last.filter(|_| i != st.node || st.finished) {
        row.tags.push((dur(path_2.ms), theme::muted()));
    }

    if warnings.iter().any(|(width_value, _)| *width_value == i) {
        row.tags.push(("!".into(), Style::default().fg(theme::pal().error())));
    }

    row
}

fn draw_steps_flow_bar(receiver_state: &mut FlowBar, frame: &mut Frame, inner: Rect, focus: bool) {
    let Some(st) = receiver_state.state.clone() else { return };

    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("every step runs on ", theme::muted()),
            Span::styled(format!("@{}", receiver_state.agent), theme::text()),
        ])),
        Rect { height: 1, ..inner },
    );
    let warn_h = receiver_state.warnings.len().min(4) as u16;

    let detail_h = if receiver_state.detail {
        8.min(inner.height / 2)
    } else {
        0
    };

    let list = Rect {
        y: inner.y + 2,
        height: inner.height.saturating_sub(2 + warn_h + detail_h),
        ..inner
    };

    let rows: Vec<flow_rail::Row> = receiver_state
        .nodes
        .iter()
        .enumerate()
        .map(|(i, style)| draw_steps_last(&st, i, style, &receiver_state.warnings))
        .collect();

    let arcs = flow_rail::arcs(&receiver_state.nodes, |id| st.round(id));

    receiver_state.popup_state.offset = crate::ui::dropdown::fit(
        receiver_state.popup_state.offset,
        receiver_state.popup_state.cur,
        rows.len(),
        usize::from(list.height),
    );
    receiver_state.popup_state.rows = flow_rail::draw(
        frame.buffer_mut(),
        list,
        &rows,
        &arcs,
        Some(receiver_state.popup_state.cur),
        focus,
        receiver_state.popup_state.offset,
    );
    let y = list.bottom();

    finish_draw_steps(frame, inner, detail_h, y, receiver_state);
}
