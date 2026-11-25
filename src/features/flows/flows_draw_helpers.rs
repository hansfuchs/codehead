use super::{
    Ask, Block, BorderType, Borders, Clear, Edit, FlowsTab, Focus, Frame, Layer, Library, Line, Modifier, Paragraph,
    PickRow, Rect, Span, Step, Style, Sub, Target, UnicodeWidthStr, flow_rail, flows, list_names, rail_rows, theme,
};

impl FlowsTab {
    pub(super) fn draw_list(&mut self, frame: &mut Frame, list: Rect, active: bool) {
        let indices = self.list();
        let shown = usize::from(list.height);
        let offset = crate::ui::dropdown::fit(0, self.at, indices.len(), shown);

        for (row, &i) in indices.iter().enumerate().skip(offset).take(shown) {
            draw_list_row(frame, list, active, offset, row, i, self);
        }

        if indices.is_empty() {
            frame.render_widget(
                Paragraph::new(Line::styled(" 0 match · n: new", theme::muted())),
                Rect { height: 1, ..list },
            );
        }
    }

    pub(super) fn draw_flow(&mut self, frame: &mut Frame, area: Rect, active: bool) {
        draw_flow_flows_tab(self, frame, area, active);
    }

    pub(super) fn draw_step(&mut self, frame: &mut Frame, area: Rect, active: bool) {
        draw_step_flows_tab(self, frame, area, active);
    }
}

fn draw_step_details(frame: &mut Frame<'_>, area: Rect, step: &Step, users: &[String], mut lines: Vec<Line<'_>>) {
    let field_line = |key: &str, value: String| {
        Line::from(vec![
            Span::styled(format!("{key:<10}"), theme::muted()),
            Span::styled(value, theme::text()),
        ])
    };

    lines.push(field_line("done when", step.done.clone()));

    if !step.needs.is_empty() {
        lines.push(field_line("needs", step.needs.join(", ")));
    }

    lines.push(field_line(
        "used in",
        if users.is_empty() {
            "0 flows".into()
        } else {
            users.join(", ")
        },
    ));
    lines.push(Line::raw(""));

    for line in step.text.lines() {
        lines.push(Line::styled(line.to_string(), theme::text()));
    }

    frame.render_widget(Paragraph::new(lines).wrap(ratatui::widgets::Wrap { trim: false }), area);
}

fn draw_list_row(
    frame: &mut Frame<'_>,
    list: Rect,
    active: bool,
    offset: usize,
    row: usize,
    i: usize,
    tab: &mut FlowsTab,
) {
    let area = Rect {
        y: list.y + (row - offset) as u16,
        height: 1,
        ..list
    };
    let on = row == tab.at;

    let (label, detail) = match tab.sub {
        Sub::Flows => {
            let flow = &tab.lib.flows[i];

            let tag = match (flow.layer, flow.edited) {
                (Layer::Repo, _) => "repo",
                (_, true) => "✎",
                (Layer::User, _) => "★",
                _ => "",
            };

            (flow.name.clone(), format!("{tag} {}", flow.steps.len()))
        }
        Sub::Steps => {
            let step = &tab.lib.steps[i];
            let used_in = tab.lib.users_of(&step.name).len();

            let tag = match (step.layer, step.edited) {
                (Layer::Repo, _) => "repo ",
                (_, true) => "✎ ",
                _ => "",
            };

            (step.name.clone(), format!("{tag}{used_in}"))
        }
    };

    let style = match (on, active && tab.focus == Focus::List && !tab.filtering) {
        (true, true) => theme::pill().add_modifier(Modifier::BOLD),
        (true, false) => theme::panel().add_modifier(Modifier::BOLD),
        _ => theme::text(),
    };

    let detail_w = detail.width() + 1;
    let label = crate::ui::thread_card::fit(
        &format!(" {label}"),
        usize::from(area.width).saturating_sub(detail_w + 1),
    );
    let pad = usize::from(area.width).saturating_sub(label.width() + detail_w);
    let line = Line::from(vec![
        Span::styled(format!("{label}{}", " ".repeat(pad)), style),
        Span::styled(format!("{detail} "), if on { style } else { theme::muted() }),
    ]);

    frame.render_widget(Paragraph::new(line), area);
    tab.hit_regions.rows.push((area, row));
}

struct EditorLayout<'frame, 'buffer, 'rects, 'y, 'cur> {
    frame: &'frame mut Frame<'buffer>,
    active: bool,
    focus: Focus,
    field_rects: &'rects mut Vec<(Rect, usize)>,
    field: usize,
    inner: Rect,
    y: &'y mut u16,
    multi_h: u16,
    cur_rect: &'cur mut Rect,
}

#[path = "flows_editor_draw.rs"]
mod editor_draw;

fn mark_rail_row(errs: &[(usize, String)], i: usize, mut row: flow_rail::Row, tab: &mut FlowsTab) -> flow_rail::Row {
    if errs.iter().any(|(j, _)| *j == i) {
        row.tags.push(("!".into(), Style::default().fg(theme::pal().error())));
    }

    if tab.target.is_some() && i == tab.node {
        row.title_style = theme::accent_bold();
    }

    row
}

fn draw_flow_flows_tab(state: &mut FlowsTab, frame: &mut Frame, area: Rect, active: bool) {
    let Some(flow) = state.flow.clone() else {
        frame.render_widget(
            Paragraph::new(Line::styled("n: a new flow", theme::muted())),
            Rect { height: 1, ..area },
        );
        return;
    };

    let nodes = flows::resolve(&flow, &state.lib);

    draw_flow_header(frame, area, &flow, nodes.len());

    let editor_h = if state.editor.is_some() {
        (area.height / 2).max(10).min(area.height.saturating_sub(6))
    } else {
        0
    };

    let errs = flow_errors(&nodes);

    let errors_h = errs.len().min(3) as u16;
    let rail = Rect {
        y: area.y + 3,
        height: area
            .height
            .saturating_sub(3 + editor_h + errors_h + u16::from(editor_h > 0)),
        ..area
    };

    let rows: Vec<flow_rail::Row> = rail_rows(&nodes)
        .into_iter()
        .enumerate()
        .map(|(i, row)| mark_rail_row(&errs, i, row, state))
        .collect();

    let arcs = flow_rail::arcs(&nodes, |_| 0);

    if rows.is_empty() {
        frame.render_widget(
            Paragraph::new(Line::styled("0 steps · a: add a step", theme::muted())),
            Rect { height: 1, ..rail },
        );
    }

    state.rail_off = crate::ui::dropdown::fit(state.rail_off, state.node, rows.len(), usize::from(rail.height));
    let focus = active && matches!(state.focus, Focus::Rail);
    let cur = (state.focus != Focus::List).then_some(state.node);

    state.hit_regions.rail_rows = flow_rail::draw(frame.buffer_mut(), rail, &rows, &arcs, cur, focus, state.rail_off);

    if let Some(target) = state.target {
        draw_target_hint(frame, area, target, &nodes);
    }

    let y = draw_flow_errors(frame, area, rail.bottom(), &errs);

    if editor_h > 0 {
        let editor_area = Rect {
            y: y + 1,
            height: editor_h,
            ..area
        };

        state.draw_editor(frame, editor_area, active);
    }
}

fn draw_step_flows_tab(state: &mut FlowsTab, frame: &mut Frame, area: Rect, active: bool) {
    if state.editor.is_some() {
        state.draw_editor(frame, area, active);
        return;
    }

    let Some(step) = state.cur_step().cloned() else {
        frame.render_widget(
            Paragraph::new(Line::styled("n: a new step", theme::muted())),
            Rect { height: 1, ..area },
        );
        return;
    };

    let users: Vec<String> = state
        .lib
        .users_of(&step.name)
        .iter()
        .map(|flow| flow.name.clone())
        .collect();
    let lines = vec![
        Line::from(vec![
            Span::styled(step.name.clone(), theme::bold()),
            Span::styled(
                format!(
                    " · {}{}",
                    if step.edited {
                        "edited built-in"
                    } else {
                        step.layer.label()
                    },
                    if step.gate { " · gate" } else { "" }
                ),
                theme::muted(),
            ),
        ]),
        Line::styled(step.description.clone(), theme::muted()),
        Line::raw(""),
    ];

    draw_step_details(frame, area, &step, &users, lines);
}

fn draw_flow_header(frame: &mut Frame, area: Rect, flow: &crate::engine::flows::Flow, node_count: usize) {
    let layer = match (flow.layer, flow.edited) {
        (_, true) => "edited built-in · ctrl+d: reset".to_string(),
        (layer, _) => layer.label().to_string(),
    };

    let head = Line::from(vec![
        Span::styled(flow.name.clone(), theme::bold()),
        Span::styled(format!(" · {node_count} steps · {layer}"), theme::muted()),
    ]);

    frame.render_widget(Paragraph::new(head), Rect { height: 1, ..area });

    if !flow.description.is_empty() {
        frame.render_widget(
            Paragraph::new(Line::styled(
                crate::ui::thread_card::fit(&flow.description, usize::from(area.width)),
                theme::muted(),
            )),
            Rect {
                y: area.y + 1,
                height: 1,
                ..area
            },
        );
    }
}

fn flow_errors(nodes: &[crate::engine::flows::Resolved]) -> Vec<(usize, String)> {
    nodes
        .iter()
        .enumerate()
        .flat_map(|(i, node)| node.errors.iter().map(move |error| (i, error.clone())))
        .chain(nodes.iter().enumerate().flat_map(|(i, node)| {
            flows::unknown_shortcuts(&node.text)
                .into_iter()
                .map(move |shortcut| (i, format!("#{shortcut}: 0 shortcut")))
        }))
        .collect()
}

fn draw_target_hint(frame: &mut Frame, area: Rect, target: Target, nodes: &[crate::engine::flows::Resolved]) {
    let what = match target {
        Target::Loop(i) => format!(
            "loop of {}: back to which step? ↑↓ enter",
            nodes.get(i).map_or("", |node| node.title.as_str())
        ),
        Target::Outcome(i) => format!(
            "gate outcome of {}: to which step? ↑↓ enter",
            nodes.get(i).map_or("", |node| node.title.as_str())
        ),
    };

    frame.render_widget(
        Paragraph::new(Line::styled(what, theme::accent_bold())),
        Rect {
            y: area.y + 2,
            height: 1,
            ..area
        },
    );
}

fn draw_flow_errors(frame: &mut Frame, area: Rect, mut y: u16, errs: &[(usize, String)]) -> u16 {
    for (i, error) in errs.iter().take(3) {
        frame.render_widget(
            Paragraph::new(Line::styled(
                crate::ui::thread_card::fit(&format!("! {}: {error}", i + 1), usize::from(area.width)),
                Style::default().fg(theme::pal().error()),
            )),
            Rect { y, height: 1, ..area },
        );
        y += 1;
    }

    y
}
