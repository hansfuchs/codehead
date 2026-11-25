use super::{Block, BorderType, Borders, Frame, Line, McpTab, Modifier, Paragraph, Rect, Span, Style, theme};

impl McpTab {
    pub(crate) fn draw(&mut self, frame: &mut Frame, area: Rect, active: bool) {
        draw_mcp_tab(self, frame, area, active);
    }
}

fn draw_group_heading(frame: &mut Frame<'_>, heads: &[&str], key: usize, row: Rect) {
    let title = heads[key];
    let rule = (row.width as usize).saturating_sub(title.chars().count() + 1);

    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(title.to_string(), theme::accent_bold()),
            Span::styled(format!(" {}", "─".repeat(rule)), theme::line()),
        ])),
        row,
    );
}

fn draw_server_row(
    frame: &mut Frame<'_>,
    active: bool,
    items: &[super::Item],
    label_width: usize,
    row: Rect,
    j: usize,
    mcp_tab: &mut McpTab,
) {
    let item = &items[j];
    let area_2 = Rect {
        x: row.x + 2,
        width: row.width.saturating_sub(2),
        ..row
    };

    super::row(
        frame,
        area_2,
        item,
        &item.detail,
        j == mcp_tab.cursor && active && !mcp_tab.filtering,
        label_width,
    );
    mcp_tab.hits.push((area_2, j));
}

fn scroll_to_selected_row(at: usize, mcp_tab: &mut McpTab) {
    if at < mcp_tab.offset {
        mcp_tab.offset = at.saturating_sub(1);
    }
}

struct ServerGroups {
    lines: Vec<Option<usize>>,
    heads: Vec<&'static str>,
}

fn draw_mcp_tab(mcp_tab: &mut McpTab, frame: &mut Frame, area: Rect, active: bool) {
    mcp_tab.hits.clear();
    mcp_tab.modal_hits.clear();
    mcp_tab.filter.hide();

    if area.height < 6 || area.width < 20 {
        return;
    }

    draw_filter(mcp_tab, frame, area, active);
    let (items, actions) = mcp_tab.items();

    if mcp_tab.cursor >= items.len() {
        mcp_tab.cursor = items.len().saturating_sub(1);
    }

    draw_summary(mcp_tab, frame, area);
    let body = Rect {
        y: area.y + 3,
        height: area.height.saturating_sub(4),
        ..area
    };
    let label_width = (body.width / 2).min(44) as usize;
    let y = draw_action_box(mcp_tab, frame, body, active, &items, actions, label_width);
    let list = Rect {
        y,
        height: body.bottom().saturating_sub(y),
        ..body
    };
    let groups = server_groups(&items, actions);

    if items.len() == actions {
        draw_empty(mcp_tab, frame, list);
    }

    draw_group_list(mcp_tab, frame, list, active, &items, label_width, &groups);
    draw_footer(mcp_tab, frame, area);
}

fn draw_filter(mcp_tab: &mut McpTab, frame: &mut Frame, area: Rect, active: bool) {
    // the filter row
    let filter_area = Rect { height: 1, ..area };

    frame.render_widget(Paragraph::new("").style(theme::panel()), filter_area);
    mcp_tab.filter.render(
        frame,
        Rect {
            x: filter_area.x + 1,
            width: filter_area.width.saturating_sub(2),
            ..filter_area
        },
        active && mcp_tab.filtering && mcp_tab.modal.is_none(),
    );
}

fn draw_summary(mcp_tab: &McpTab, frame: &mut Frame, area: Rect) {
    // the state line: updating, counts, the CLI errors
    let mut head = vec![];

    if mcp_tab.updating() {
        head.push(Span::styled(
            format!(
                "updating{} ",
                if mcp_tab.jobs.refreshing {
                    ": claude mcp list checks every server (up to 1 min.)"
                } else {
                    ""
                }
            ),
            theme::text().fg(theme::pal().progress()),
        ));
    }

    head.push(Span::styled(
        format!(
            "{} codehead · {} harness servers",
            mcp_tab.servers.len(),
            mcp_tab.natives.len()
        ),
        theme::muted(),
    ));

    for (harness, error_2) in &mcp_tab.jobs.cli_errors {
        head.push(Span::styled(
            format!(" · {harness}: {error_2}"),
            theme::text().fg(theme::pal().error()),
        ));
    }

    frame.render_widget(
        Paragraph::new(Line::from(head)),
        Rect {
            y: area.y + 1,
            height: 1,
            ..area
        },
    );
}

fn draw_action_box(
    mcp_tab: &mut McpTab,
    frame: &mut Frame,
    body: Rect,
    active: bool,
    items: &[super::Item],
    actions: usize,
    label_width: usize,
) -> u16 {
    if actions == 0 {
        return body.y;
    }

    {
        let shown = actions.min(6);
        let height_2 = shown as u16 + 2;
        let area_value = Rect {
            y: body.y,
            height: height_2.min(body.height),
            ..body
        };
        let block = Block::new()
            .borders(Borders::ALL)
            .border_type(BorderType::Plain)
            .border_style(Style::default().fg(theme::pal().attention()))
            .title(Span::styled(
                format!(" needs action · {actions} "),
                theme::text().fg(theme::pal().attention()).add_modifier(Modifier::BOLD),
            ));
        let inner = block.inner(area_value);

        frame.render_widget(block, area_value);

        let offset = if mcp_tab.cursor < actions {
            mcp_tab.cursor.saturating_sub(shown - 1)
        } else {
            0
        };

        for (j, item) in items.iter().enumerate().take(actions).skip(offset).take(shown) {
            let row = Rect {
                y: inner.y + (j - offset) as u16,
                height: 1,
                ..inner
            };

            super::row(
                frame,
                row,
                item,
                &item.action,
                j == mcp_tab.cursor && active && !mcp_tab.filtering,
                label_width,
            );
            mcp_tab.hits.push((row, j));
        }

        body.y + height_2 + 1
    }
}

fn server_groups(items: &[super::Item], actions: usize) -> ServerGroups {
    let mut lines: Vec<Option<usize>> = Vec::new();
    let mut heads: Vec<&'static str> = Vec::new();
    let mut last = "";

    for (j, item) in items.iter().enumerate().skip(actions) {
        if item.group != last {
            separate_server_groups(&mut lines, &mut heads);

            lines.push(None);
            heads.push(item.group);
            last = item.group;
        }

        lines.push(Some(j));
        heads.push("");
    }

    ServerGroups { lines, heads }
}

fn draw_empty(mcp_tab: &McpTab, frame: &mut Frame, list: Rect) {
    let empty = if mcp_tab.filter.text().trim().is_empty() {
        "0 servers · a: add one · r: refresh the harness lists"
    } else {
        "0 servers match the filter"
    };

    frame.render_widget(
        Paragraph::new(Line::styled(empty, theme::muted())),
        Rect { height: 1, ..list },
    );
}

fn draw_group_list(
    mcp_tab: &mut McpTab,
    frame: &mut Frame,
    list: Rect,
    active: bool,
    items: &[super::Item],
    label_width: usize,
    groups: &ServerGroups,
) {
    let ServerGroups { lines, heads } = groups;
    let view = list.height as usize;

    if let Some(at) = lines.iter().position(|line_value| *line_value == Some(mcp_tab.cursor)) {
        scroll_to_selected_row(at, mcp_tab);

        if at >= mcp_tab.offset + view {
            mcp_tab.offset = at + 1 - view;
        }
    }

    mcp_tab.offset = mcp_tab.offset.min(lines.len().saturating_sub(view));

    for (key, line) in lines.iter().enumerate().skip(mcp_tab.offset).take(view) {
        let row = Rect {
            y: list.y + (key - mcp_tab.offset) as u16,
            height: 1,
            ..list
        };

        match line {
            None if !heads[key].is_empty() => {
                draw_group_heading(frame, heads, key, row);
            }
            None => {}
            Some(j) => {
                draw_server_row(frame, active, items, label_width, row, *j, mcp_tab);
            }
        }
    }
}

fn draw_footer(mcp_tab: &mut McpTab, frame: &mut Frame, area: Rect) {
    frame.render_widget(
        Paragraph::new(Line::styled(
            mcp_tab.status.clone(),
            if mcp_tab.error {
                theme::text().fg(theme::pal().error())
            } else {
                theme::muted()
            },
        )),
        Rect {
            y: area.bottom() - 1,
            height: 1,
            ..area
        },
    );

    if mcp_tab.modal.is_some() {
        super::draw_helpers::draw_modal(frame, area, mcp_tab);
    }
}

fn separate_server_groups(lines: &mut Vec<Option<usize>>, heads: &mut Vec<&str>) {
    if !lines.is_empty() {
        lines.push(None);
        heads.push("");
    }
}
