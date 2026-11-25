use super::{FlowBar, Frame, Line, Paragraph, Rect, Span, Style, Wrap, dur, flow_rail, rail_row, theme};

impl FlowBar {
    pub(super) fn draw_steps(&mut self, frame: &mut Frame, inner: Rect, focus: bool) {
        draw_steps_flow_bar(self, frame, inner, focus);
    }
}

fn draw_detail_and_warnings(frame: &mut Frame<'_>, inner: Rect, detail_height: u16, mut y: u16, bar: &mut FlowBar) {
    if detail_height > 0
        && let Some(node) = bar.nodes.get(bar.popup_state.cur)
    {
        let area = Rect {
            y,
            height: detail_height,
            ..inner
        };
        let mut lines = vec![Line::styled("─".repeat(usize::from(area.width)), theme::line())];
        let head = format!(
            "{} · {}{}",
            node.title,
            node.step
                .as_deref()
                .map_or("one-off".to_string(), |step| format!("step {step}")),
            if node.gate { " · gate" } else { "" }
        );

        lines.push(Line::styled(head, theme::bold()));

        for text_line in crate::domain::shortcuts::expand(&node.text).lines() {
            lines.push(Line::styled(text_line.to_string(), theme::text()));
        }

        if !node.done.is_empty() {
            lines.push(Line::styled(format!("done when: {}", node.done), theme::muted()));
        }

        frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), area);
        y += detail_height;
    }

    for (i, (node, warning)) in bar.warnings.iter().take(4).enumerate() {
        let row_area = Rect {
            y: y + i as u16,
            height: 1,
            ..inner
        };

        frame.render_widget(
            Paragraph::new(Line::styled(
                crate::ui::thread_card::fit(&format!("! {}: {warning}", node + 1), usize::from(row_area.width)),
                Style::default().fg(theme::pal().error()),
            )),
            row_area,
        );
    }
}

fn step_row(
    run: &crate::engine::flows::run::FlowState,
    i: usize,
    node: &crate::engine::flows::Resolved,
    warnings: &[(usize, String)],
) -> flow_rail::Row {
    let last = run.history.iter().rev().find(|entry| entry.node == i);

    let (mark, mark_style) = if i == run.node && !run.finished {
        if run.wait.is_some() {
            ("⏸", theme::bold().fg(theme::pal().attention()))
        } else {
            ("●", theme::bold().fg(theme::pal().progress()))
        }
    } else {
        match last.map(|entry| entry.how.as_str()) {
            Some("skipped") => ("⤼", theme::muted()),
            Some("left") => ("·", theme::muted()),
            Some(_) => ("✓", Style::default().fg(theme::pal().success())),
            None => ("○", theme::muted()),
        }
    };

    let mut row = rail_row(node, mark, mark_style);

    if let Some(entry) = last.filter(|_| i != run.node || run.finished) {
        row.tags.push((dur(entry.ms), theme::muted()));
    }

    if warnings.iter().any(|(node, _)| *node == i) {
        row.tags.push(("!".into(), Style::default().fg(theme::pal().error())));
    }

    row
}

fn draw_steps_flow_bar(bar: &mut FlowBar, frame: &mut Frame, inner: Rect, focus: bool) {
    let Some(run) = bar.state.clone() else { return };

    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("every step runs on ", theme::muted()),
            Span::styled(format!("@{}", bar.agent), theme::text()),
        ])),
        Rect { height: 1, ..inner },
    );
    let warnings_height = bar.warnings.len().min(4) as u16;

    let detail_height = if bar.detail { 8.min(inner.height / 2) } else { 0 };

    let list = Rect {
        y: inner.y + 2,
        height: inner.height.saturating_sub(2 + warnings_height + detail_height),
        ..inner
    };

    let rows: Vec<flow_rail::Row> = bar
        .nodes
        .iter()
        .enumerate()
        .map(|(i, node)| step_row(&run, i, node, &bar.warnings))
        .collect();

    let arcs = flow_rail::arcs(&bar.nodes, |id| run.round(id));

    bar.popup_state.offset = crate::ui::dropdown::fit(
        bar.popup_state.offset,
        bar.popup_state.cur,
        rows.len(),
        usize::from(list.height),
    );
    bar.popup_state.rows = flow_rail::draw(
        frame.buffer_mut(),
        list,
        &rows,
        &arcs,
        Some(bar.popup_state.cur),
        focus,
        bar.popup_state.offset,
    );
    let y = list.bottom();

    draw_detail_and_warnings(frame, inner, detail_height, y, bar);
}
