use super::{
    Block, BorderType, Borders, Clear, FlowBar, Frame, Line, Mode, Paragraph, Rect, Span, flow_rail, flows, rail_row,
    theme,
};
use std::ops::ControlFlow;

impl FlowBar {
    pub(crate) fn render(&mut self, frame: &mut Frame, area: Rect, focus: bool) {
        render_flow_bar(self, frame, area, focus);
    }
}

fn finish_render(frame: &mut Frame<'_>, focus: bool, inner: Rect, view_state: &mut FlowBar) {
    match view_state.popup_state.mode.clone() {
        Mode::Steps => view_state.draw_steps(frame, inner, focus),
        Mode::Pick(query) => {
            render_mode_pick_query(frame, focus, inner, &query, view_state);
        }
        Mode::How(slug) => {
            render_mode_how_slug(frame, focus, inner, &slug, view_state);
        }
        Mode::Start(slug) => {
            if let ControlFlow::Break(()) = finish_render_mode_start_slug(frame, focus, inner, view_state, &slug) {}
        }
    }
}

fn finish_render_mode_start_slug(
    frame: &mut Frame<'_>,
    focus: bool,
    inner: Rect,
    view_state: &mut FlowBar,
    slug: &str,
) -> ControlFlow<()> {
    let Some(fl) = view_state.lib.flow(slug).cloned() else {
        return ControlFlow::Break(());
    };

    frame.render_widget(
        Paragraph::new(Line::styled(format!("{}: start at", fl.name), theme::bold())),
        Rect { height: 1, ..inner },
    );
    let nodes = flows::resolve(&fl, &view_state.lib);
    let rows: Vec<flow_rail::Row> = nodes.iter().map(|count| rail_row(count, "○", theme::muted())).collect();
    let arcs = flow_rail::arcs(&nodes, |_| 0);
    let list = Rect {
        y: inner.y + 2,
        height: inner.height.saturating_sub(2),
        ..inner
    };

    view_state.popup_state.offset = crate::ui::dropdown::fit(
        view_state.popup_state.offset,
        view_state.popup_state.cur,
        rows.len(),
        usize::from(list.height),
    );
    view_state.popup_state.rows = flow_rail::draw(
        frame.buffer_mut(),
        list,
        &rows,
        &arcs,
        Some(view_state.popup_state.cur),
        focus,
        view_state.popup_state.offset,
    );

    ControlFlow::Continue(())
}

fn render_mode_how_slug(frame: &mut Frame<'_>, focus: bool, inner: Rect, slug: &str, view_state: &mut FlowBar) {
    let name = view_state
        .lib
        .flow(slug)
        .map_or(slug.to_string(), |file| file.name.clone());
    let cur = view_state
        .state
        .as_ref()
        .map_or(String::new(), |text_2| text_2.title.clone());
    let left = view_state
        .state
        .as_ref()
        .map_or(0, |text_3| view_state.nodes.len().saturating_sub(text_3.node));

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

    view_state.list(
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

fn render_mode_pick_query(frame: &mut Frame<'_>, focus: bool, inner: Rect, query: &str, view_state: &mut FlowBar) {
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

    let rows: Vec<(String, String)> = view_state
        .picks(query)
        .into_iter()
        .map(|i| {
            let fl = &view_state.lib.flows[i];
            let cur = view_state
                .state
                .as_ref()
                .is_some_and(|text_value| text_value.flow == fl.slug);

            (
                format!("{}{}", fl.name, if cur { " (current)" } else { "" }),
                format!("{} steps · {}", fl.steps.len(), fl.description),
            )
        })
        .collect();

    view_state.list(frame, list, &rows, focus);
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

    let area_value = Rect {
        height: area.height - 1,
        ..area
    };

    state.popup_state.popup = area_value;
    frame.render_widget(Clear, area_value);
    let (title, keys) = state.frame_text();
    let block = Block::new()
        .borders(Borders::ALL)
        .border_type(BorderType::Plain)
        .border_style(theme::accent())
        .title(Span::styled(format!(" {title} "), theme::accent_bold()))
        .title_bottom(Line::styled(format!(" {keys} "), theme::muted()).right_aligned());
    let inner = block.inner(area_value);

    frame.render_widget(block, area_value);
    let inner = Rect {
        x: inner.x + 1,
        width: inner.width.saturating_sub(2),
        ..inner
    };

    if inner.height == 0 {
        return;
    }

    finish_render(frame, focus, inner, state);
}
