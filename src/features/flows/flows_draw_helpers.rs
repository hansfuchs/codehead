use super::{
    Ask, Block, BorderType, Borders, Clear, Edit, FlowsTab, Focus, Frame, Layer, Library, Line, Modifier, Paragraph,
    PickRow, Rect, Span, Step, Style, Sub, Target, UnicodeWidthStr, flow_rail, flows, list_names, rail_rows, theme,
};

impl FlowsTab {
    pub(super) fn draw_list(&mut self, frame: &mut Frame, list: Rect, active: bool) {
        let idx = self.list();
        let shown = usize::from(list.height);
        let off = crate::ui::dropdown::fit(0, self.at, idx.len(), shown);

        for (key, &i) in idx.iter().enumerate().skip(off).take(shown) {
            draw_list_area(frame, list, active, off, key, i, self);
        }

        if idx.is_empty() {
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

fn finish_draw_step(frame: &mut Frame<'_>, area: Rect, text_value: &Step, users: &[String], mut lines: Vec<Line<'_>>) {
    let kv = |key: &str, value: String| {
        Line::from(vec![
            Span::styled(format!("{key:<10}"), theme::muted()),
            Span::styled(value, theme::text()),
        ])
    };

    lines.push(kv("done when", text_value.done.clone()));

    if !text_value.needs.is_empty() {
        lines.push(kv("needs", text_value.needs.join(", ")));
    }

    lines.push(kv(
        "used in",
        if users.is_empty() {
            "0 flows".into()
        } else {
            users.join(", ")
        },
    ));
    lines.push(Line::raw(""));

    for line in text_value.text.lines() {
        lines.push(Line::styled(line.to_string(), theme::text()));
    }

    frame.render_widget(Paragraph::new(lines).wrap(ratatui::widgets::Wrap { trim: false }), area);
}

fn draw_list_area(
    frame: &mut Frame<'_>,
    list: Rect,
    active: bool,
    off: usize,
    key: usize,
    i: usize,
    view_state: &mut FlowsTab,
) {
    let area = Rect {
        y: list.y + (key - off) as u16,
        height: 1,
        ..list
    };
    let on = key == view_state.at;

    let (label, detail) = match view_state.sub {
        Sub::Flows => {
            let fl = &view_state.lib.flows[i];

            let tag = match (fl.layer, fl.edited) {
                (Layer::Repo, _) => "repo",
                (_, true) => "✎",
                (Layer::User, _) => "★",
                _ => "",
            };

            (fl.name.clone(), format!("{tag} {}", fl.steps.len()))
        }
        Sub::Steps => {
            let text_value = &view_state.lib.steps[i];
            let count = view_state.lib.users_of(&text_value.name).len();

            let tag = match (text_value.layer, text_value.edited) {
                (Layer::Repo, _) => "repo ",
                (_, true) => "✎ ",
                _ => "",
            };

            (text_value.name.clone(), format!("{tag}{count}"))
        }
    };

    let st = match (on, active && view_state.focus == Focus::List && !view_state.filtering) {
        (true, true) => theme::pill().add_modifier(Modifier::BOLD),
        (true, false) => theme::panel().add_modifier(Modifier::BOLD),
        _ => theme::text(),
    };

    let dw = detail.width() + 1;
    let label = crate::ui::thread_card::fit(&format!(" {label}"), usize::from(area.width).saturating_sub(dw + 1));
    let pad = usize::from(area.width).saturating_sub(label.width() + dw);
    let line = Line::from(vec![
        Span::styled(format!("{label}{}", " ".repeat(pad)), st),
        Span::styled(format!("{detail} "), if on { st } else { theme::muted() }),
    ]);

    frame.render_widget(Paragraph::new(line), area);
    view_state.hit_regions.rows.push((area, key));
}

struct EditorLayout<'reference_0, 'reference_1, 'reference_2, 'reference_3, 'reference_4> {
    frame: &'reference_0 mut Frame<'reference_1>,
    active: bool,
    focus: Focus,
    field_rects: &'reference_2 mut Vec<(Rect, usize)>,
    field: usize,
    inner: Rect,
    y: &'reference_3 mut u16,
    multi_h: u16,
    cur_rect: &'reference_4 mut Rect,
}

#[path = "flows_editor_draw.rs"]
mod editor_draw;

fn draw_flow_branch(
    errs: &[(usize, String)],
    i: usize,
    mut row: flow_rail::Row,
    view_state: &mut FlowsTab,
) -> flow_rail::Row {
    if errs.iter().any(|(j, _)| *j == i) {
        row.tags.push(("!".into(), Style::default().fg(theme::pal().error())));
    }

    if view_state.target.is_some() && i == view_state.node {
        row.title_style = theme::accent_bold();
    }

    row
}

fn draw_flow_flows_tab(state: &mut FlowsTab, frame: &mut Frame, area: Rect, active: bool) {
    let Some(fl) = state.flow.clone() else {
        frame.render_widget(
            Paragraph::new(Line::styled("n: a new flow", theme::muted())),
            Rect { height: 1, ..area },
        );
        return;
    };

    let nodes = flows::resolve(&fl, &state.lib);

    draw_flow_header(frame, area, &fl, nodes.len());

    let ed_h = if state.editor.is_some() {
        (area.height / 2).max(10).min(area.height.saturating_sub(6))
    } else {
        0
    };

    let errs = flow_errors(&nodes);

    let err_h = errs.len().min(3) as u16;
    let rail = Rect {
        y: area.y + 3,
        height: area.height.saturating_sub(3 + ed_h + err_h + u16::from(ed_h > 0)),
        ..area
    };

    let rows: Vec<flow_rail::Row> = rail_rows(&nodes)
        .into_iter()
        .enumerate()
        .map(|(i, row)| draw_flow_branch(&errs, i, row, state))
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

    if let Some(thread) = state.target {
        draw_target_hint(frame, area, thread, &nodes);
    }

    let y = draw_flow_errors(frame, area, rail.bottom(), &errs);

    if ed_h > 0 {
        let area_value = Rect {
            y: y + 1,
            height: ed_h,
            ..area
        };

        state.draw_editor(frame, area_value, active);
    }
}

fn draw_step_flows_tab(state: &mut FlowsTab, frame: &mut Frame, area: Rect, active: bool) {
    if state.editor.is_some() {
        state.draw_editor(frame, area, active);
        return;
    }

    let Some(text_value) = state.cur_step().cloned() else {
        frame.render_widget(
            Paragraph::new(Line::styled("n: a new step", theme::muted())),
            Rect { height: 1, ..area },
        );
        return;
    };

    let users: Vec<String> = state
        .lib
        .users_of(&text_value.name)
        .iter()
        .map(|file| file.name.clone())
        .collect();
    let lines = vec![
        Line::from(vec![
            Span::styled(text_value.name.clone(), theme::bold()),
            Span::styled(
                format!(
                    " · {}{}",
                    if text_value.edited {
                        "edited built-in"
                    } else {
                        text_value.layer.label()
                    },
                    if text_value.gate { " · gate" } else { "" }
                ),
                theme::muted(),
            ),
        ]),
        Line::styled(text_value.description.clone(), theme::muted()),
        Line::raw(""),
    ];

    finish_draw_step(frame, area, &text_value, &users, lines);
}

fn draw_flow_header(frame: &mut Frame, area: Rect, fl: &crate::engine::flows::Flow, node_count: usize) {
    let layer = match (fl.layer, fl.edited) {
        (_, true) => "edited built-in · ctrl+d: reset".to_string(),
        (line, _) => line.label().to_string(),
    };

    let head = Line::from(vec![
        Span::styled(fl.name.clone(), theme::bold()),
        Span::styled(format!(" · {node_count} steps · {layer}"), theme::muted()),
    ]);

    frame.render_widget(Paragraph::new(head), Rect { height: 1, ..area });

    if !fl.description.is_empty() {
        frame.render_widget(
            Paragraph::new(Line::styled(
                crate::ui::thread_card::fit(&fl.description, usize::from(area.width)),
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
        .flat_map(|(i, count)| count.errors.iter().map(move |error_value| (i, error_value.clone())))
        .chain(nodes.iter().enumerate().flat_map(|(i, count_value)| {
            flows::unknown_shortcuts(&count_value.text)
                .into_iter()
                .map(move |text_value| (i, format!("#{text_value}: 0 shortcut")))
        }))
        .collect()
}

fn draw_target_hint(frame: &mut Frame, area: Rect, thread: Target, nodes: &[crate::engine::flows::Resolved]) {
    let what = match thread {
        Target::Loop(i) => format!(
            "loop of {}: back to which step? ↑↓ enter",
            nodes.get(i).map_or("", |count_2| count_2.title.as_str())
        ),
        Target::Outcome(i) => format!(
            "gate outcome of {}: to which step? ↑↓ enter",
            nodes.get(i).map_or("", |count_3| count_3.title.as_str())
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
    for (i, error_2) in errs.iter().take(3) {
        frame.render_widget(
            Paragraph::new(Line::styled(
                crate::ui::thread_card::fit(&format!("! {}: {error_2}", i + 1), usize::from(area.width)),
                Style::default().fg(theme::pal().error()),
            )),
            Rect { y, height: 1, ..area },
        );
        y += 1;
    }

    y
}
