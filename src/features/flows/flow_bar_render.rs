use super::{
    Block, BorderType, Borders, Clear, FlowBar, Frame, Line, Mode, Paragraph, Rect, Span, flow_rail, flows, rail_row,
    theme,
};

impl FlowBar {
    pub(crate) fn render(&mut self, frame: &mut Frame, area: Rect, focus: bool) {
        render_flow_bar(self, frame, area, focus);
    }
}

fn draw_mode(frame: &mut Frame<'_>, focus: bool, inner: Rect, bar: &mut FlowBar) {
    match bar.popup_state.mode.clone() {
        Mode::Steps => bar.draw_steps(frame, inner, focus),
        Mode::Pick(query) => {
            draw_flow_picker(frame, focus, inner, &query, bar);
        }
        Mode::How(slug) => {
            draw_switch_choices(frame, focus, inner, &slug, bar);
        }
        Mode::Start(slug) => {
            draw_start_step(frame, focus, inner, bar, &slug);
        }
    }
}

fn draw_start_step(frame: &mut Frame<'_>, focus: bool, inner: Rect, bar: &mut FlowBar, slug: &str) {
    let Some(flow) = bar.lib.flow(slug).cloned() else {
        return;
    };

    frame.render_widget(
        Paragraph::new(Line::styled(format!("{}: start at", flow.name), theme::bold())),
        Rect { height: 1, ..inner },
    );
    let nodes = flows::resolve(&flow, &bar.lib);
    let rows: Vec<flow_rail::Row> = nodes.iter().map(|node| rail_row(node, "○", theme::muted())).collect();
    let arcs = flow_rail::arcs(&nodes, |_| 0);
    let list = Rect {
        y: inner.y + 2,
        height: inner.height.saturating_sub(2),
        ..inner
    };

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
}

fn draw_switch_choices(frame: &mut Frame<'_>, focus: bool, inner: Rect, slug: &str, bar: &mut FlowBar) {
    let name = bar.lib.flow(slug).map_or(slug.to_string(), |flow| flow.name.clone());
    let cur = bar.state.as_ref().map_or(String::new(), |run| run.title.clone());
    let left = bar
        .state
        .as_ref()
        .map_or(0, |run| bar.nodes.len().saturating_sub(run.node));

    frame.render_widget(
        Paragraph::new(Line::styled(format!("change flow: {cur} → {name}"), theme::bold())),
        Rect { height: 1, ..inner },
    );
    let rows = vec![
        (
            "switch now".to_string(),
            "the running turn stops at its next tool boundary; then the start step".to_string(),
        ),
        ("after current flow".to_string(), format!("{left} steps of {cur} left")),
        ("cancel".to_string(), String::new()),
    ];

    bar.list(
        frame,
        Rect {
            y: inner.y + 2,
            height: inner.height.saturating_sub(2),
            ..inner
        },
        &rows,
        focus,
    );
}

fn draw_flow_picker(frame: &mut Frame<'_>, focus: bool, inner: Rect, query: &str, bar: &mut FlowBar) {
    let head = Rect { height: 1, ..inner };

    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("flow: ", theme::muted()),
            Span::styled(format!("{query}▏"), theme::text()),
        ])),
        head,
    );
    let list = Rect {
        y: inner.y + 2,
        height: inner.height.saturating_sub(2),
        ..inner
    };

    let rows: Vec<(String, String)> = bar
        .picks(query)
        .into_iter()
        .map(|i| {
            let flow = &bar.lib.flows[i];
            let cur = bar.state.as_ref().is_some_and(|run| run.flow == flow.slug);

            (
                format!("{}{}", flow.name, if cur { " (current)" } else { "" }),
                format!("{} steps · {}", flow.steps.len(), flow.description),
            )
        })
        .collect();

    bar.list(frame, list, &rows, focus);
}

fn render_flow_bar(state: &mut FlowBar, frame: &mut Frame, area: Rect, focus: bool) {
    state.popup_state.bar = Rect::default();
    state.popup_state.popup = Rect::default();
    state.popup_state.rows.clear();

    if area.is_empty() {
        return;
    }

    let anchor = Rect {
        y: area.bottom() - 1,
        height: 1,
        ..area
    };

    state.popup_state.bar = anchor;
    frame.render_widget(Clear, anchor);
    let (spans, style) = state.bar_line();

    let style = if state.popup_state.hover {
        style.bg(theme::pal().surface1)
    } else {
        style
    };

    frame.render_widget(Paragraph::new(Line::from(spans)).style(style), anchor);

    if !state.popup_state.open || area.height < 5 {
        return;
    }

    let popup = Rect {
        height: area.height - 1,
        ..area
    };

    state.popup_state.popup = popup;
    frame.render_widget(Clear, popup);
    let (title, keys) = state.frame_text();
    let block = Block::new()
        .borders(Borders::ALL)
        .border_type(BorderType::Plain)
        .border_style(theme::accent())
        .title(Span::styled(format!(" {title} "), theme::accent_bold()))
        .title_bottom(Line::styled(format!(" {keys} "), theme::muted()).right_aligned());
    let inner = block.inner(popup);

    frame.render_widget(block, popup);
    let inner = Rect {
        x: inner.x + 1,
        width: inner.width.saturating_sub(2),
        ..inner
    };

    if inner.height == 0 {
        return;
    }

    draw_mode(frame, focus, inner, state);
}
