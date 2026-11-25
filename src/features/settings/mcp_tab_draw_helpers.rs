use super::{
    Block, BorderType, Borders, Clear, Frame, KINDS, Line, McpTab, Modal, Paragraph, Rect, Span, Style, Wrap,
    harness_label, theme,
};

pub(super) fn draw_modal(frame: &mut Frame, area: Rect, mcp_tab: &mut McpTab) {
    let width = area.width.saturating_sub(8).min(90);
    let place = |height: u16| Rect {
        x: area.x + (area.width - width) / 2,
        y: area.y + 2,
        width,
        height: height.min(area.height.saturating_sub(2)),
    };

    let boxed = |frame: &mut Frame, place: Rect, title: &str| modal_box(frame, area, place, title);

    match mcp_tab.modal.as_mut() {
        Some(Modal::Info { title, lines, login }) => {
            let info_area = place(lines.len() as u16 + 4);
            let inner = boxed(frame, info_area, title);

            let mut text: Vec<Line> = lines.iter().map(|line| info_line(line)).collect();

            text.push(Line::raw(""));
            text.push(Line::styled(
                if login.is_some() {
                    "l: sign in · enter / esc: close"
                } else {
                    "enter / esc: close"
                },
                theme::muted(),
            ));
            frame.render_widget(Paragraph::new(text).wrap(Wrap { trim: false }), inner);
        }
        Some(Modal::Confirm { text, .. }) => {
            let confirm_area = place(6);
            let inner = boxed(frame, confirm_area, "confirm");

            frame.render_widget(
                Paragraph::new(vec![
                    Line::raw(text.clone()),
                    Line::raw(""),
                    Line::styled("y / enter: yes · n / esc: no", theme::muted()),
                ])
                .wrap(Wrap { trim: false }),
                inner,
            );
        }
        Some(Modal::Agents(pick)) => {
            mcp_tab.modal_hits = draw_agent_selection(frame, place, boxed, pick);
        }
        Some(Modal::Edit(form)) => {
            draw_server_form(frame, place, boxed, form);
        }
        None => {}
    }
}

fn info_line(line: &str) -> Line<'_> {
    if line.starts_with("  ") {
        Line::styled(line.to_owned(), theme::accent())
    } else {
        Line::raw(line.to_owned())
    }
}

fn draw_agent_selection(
    frame: &mut Frame<'_>,
    place: impl Fn(u16) -> Rect,
    boxed: impl Fn(&mut Frame<'_>, Rect, &str) -> Rect,
    selection: &mut super::Pick,
) -> Vec<(Rect, usize)> {
    let count = selection.rows.len();
    let pick_area = place(count as u16 + 6);
    let inner = boxed(frame, pick_area, &format!("{} · which agents get it", selection.name));
    let mut y = inner.y;
    let mut last = "";
    let view = inner.height.saturating_sub(1) as usize;
    let offset = selection.cursor.saturating_sub(view.saturating_sub(4));
    let mut hits = Vec::new();

    for (i, (agent, selected)) in selection.rows.iter().enumerate().skip(offset) {
        if y + 1 >= inner.bottom() {
            break;
        }

        draw_harness_heading(frame, selection, inner, &mut y, &mut last, agent);

        let row = Rect {
            y,
            height: 1,
            x: inner.x + 2,
            width: inner.width.saturating_sub(2),
        };
        let highlighted = i == selection.cursor;
        let style = if highlighted { theme::pill() } else { theme::text() };

        frame.render_widget(
            Paragraph::new(Line::styled(
                format!("{} {}", if *selected { "☑" } else { "☐" }, agent.name),
                style,
            ))
            .style(style),
            row,
        );
        hits.push((row, i));
        y += 1;
    }

    if count == 0 {
        frame.render_widget(
            Paragraph::new(Line::styled("0 agents · make one in the agents tab", theme::muted())),
            Rect { height: 1, ..inner },
        );
    }

    frame.render_widget(
        Paragraph::new(Line::styled(
            "space: agent · h: harness · enter: save · esc: cancel",
            theme::muted(),
        )),
        Rect {
            y: inner.bottom() - 1,
            height: 1,
            ..inner
        },
    );

    hits
}

fn draw_harness_heading<'agent>(
    frame: &mut Frame<'_>,
    selection: &super::Pick,
    inner: Rect,
    y: &mut u16,
    last: &mut &'agent str,
    agent: &'agent crate::engine::roster::Agent,
) {
    if agent.harness != *last {
        let all = selection
            .rows
            .iter()
            .filter(|row| row.0.harness == agent.harness)
            .all(|row| row.1);

        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(harness_label(&agent.harness), theme::accent_bold()),
                Span::styled(
                    if all {
                        " · all on · h: all off"
                    } else {
                        " · h: all on"
                    },
                    theme::muted(),
                ),
            ])),
            Rect {
                y: *y,
                height: 1,
                ..inner
            },
        );
        *y += 1;
        *last = &agent.harness;
    }
}

fn draw_server_form(
    frame: &mut Frame<'_>,
    place: impl Fn(u16) -> Rect,
    boxed: impl Fn(&mut Frame<'_>, Rect, &str) -> Rect,
    edit_form: &mut Box<super::EditForm>,
) {
    let form_area = place(16);

    let title = if edit_form.old.is_some() {
        "edit MCP server"
    } else {
        "add MCP server"
    };

    let inner = boxed(frame, form_area, title);
    let label_width = 10u16;
    let rows = [
        ("name", 1u16),
        ("type", 1),
        (if edit_form.kind == 0 { "command" } else { "URL" }, 1),
        (if edit_form.kind == 0 { "env" } else { "headers" }, 5),
        ("auto-allow", 1),
    ];
    let y = inner.y;

    draw_server_fields(frame, edit_form, inner, label_width, rows, y);

    let hint = "secrets: 1 KEY=value per line → the OS keychain on save; KEY=${VAR}: an env var of codehead";

    frame.render_widget(
        Paragraph::new(Line::styled(hint, theme::muted())).wrap(Wrap { trim: false }),
        Rect {
            y: inner.bottom().saturating_sub(2),
            height: 2,
            ..inner
        },
    );
}

fn draw_server_fields(
    frame: &mut Frame<'_>,
    edit_form: &mut Box<super::EditForm>,
    inner: Rect,
    label_width: u16,
    rows: [(&str, u16); 5],
    mut y: u16,
) {
    for (i, (label, height)) in rows.iter().enumerate() {
        let focused = edit_form.field == i;
        let label_area = Rect {
            y,
            height: 1,
            width: label_width,
            ..inner
        };

        frame.render_widget(
            Paragraph::new(Line::styled(
                *label,
                if focused { theme::accent_bold() } else { theme::muted() },
            )),
            label_area,
        );
        let input_area = Rect {
            x: inner.x + label_width,
            y,
            width: inner.width.saturating_sub(label_width),
            height: *height,
        };

        match i {
            0 => {
                frame.render_widget(Paragraph::new("").style(theme::panel()), input_area);
                edit_form.name.render(frame, input_area, focused);
            }
            1 => {
                draw_server_kind(frame, edit_form, focused, input_area);
            }
            2 => {
                frame.render_widget(Paragraph::new("").style(theme::panel()), input_area);
                edit_form.target.render(frame, input_area, focused);
            }
            3 => {
                frame.render_widget(Paragraph::new("").style(theme::panel()), input_area);
                edit_form.secrets.render(frame, input_area, focused);
            }
            _ => draw_auto_allow(frame, input_area, focused, edit_form.auto_allow),
        }

        y += height + 1;
    }
}

fn draw_server_kind(frame: &mut Frame<'_>, edit_form: &mut Box<super::EditForm>, focused: bool, input_area: Rect) {
    let spans: Vec<Span> = KINDS
        .iter()
        .enumerate()
        .map(|(index, kind)| Span::styled(format!(" {kind} "), kind_style(index == edit_form.kind, focused)))
        .collect();

    frame.render_widget(Paragraph::new(Line::from(spans)), input_area);
}

fn modal_box(frame: &mut Frame, area: Rect, modal_area: Rect, title: &str) -> Rect {
    // a modal: the screen behind it fades
    theme::dim(frame.buffer_mut(), area);
    frame.render_widget(Clear, modal_area);
    let block = Block::new()
        .borders(Borders::ALL)
        .border_type(BorderType::Plain)
        .border_style(theme::accent())
        .title(Span::styled(format!(" {title} "), theme::accent_bold()))
        .style(Style::default().bg(theme::pal().on_accent).fg(theme::pal().text));
    let inner = block.inner(modal_area);

    frame.render_widget(block, modal_area);

    Rect {
        x: inner.x + 1,
        width: inner.width.saturating_sub(2),
        ..inner
    }
}

fn draw_auto_allow(frame: &mut Frame, input_area: Rect, focused: bool, enabled: bool) {
    let style = if focused { theme::pill() } else { theme::text() };

    frame.render_widget(
        Paragraph::new(Line::styled(
            format!(" {} its tools run without an ask ", if enabled { "☑" } else { "☐" }),
            style,
        )),
        input_area,
    );
}

fn kind_style(selected: bool, focused: bool) -> Style {
    match (selected, focused) {
        (true, true) => theme::pill(),
        (true, false) => theme::accent_bold(),
        _ => theme::muted(),
    }
}
