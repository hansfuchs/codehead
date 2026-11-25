use super::{
    BRANCH, COMMANDS, Command, FLOW, Frame, HANDOFF, INFO_ROWS, KINDS, Kind, Level, Line, MAX_W, MAX_W_SKILLS, MENTION,
    Modifier, Paragraph, RELOAD, Rect, SKILLS, SUB, Span, Style, TextArea, UnicodeWidthStr, dropdown, head, tail,
    theme,
};

impl Command {
    /// The dropdowns for the `>` of TA (TA rendered this frame), inside BOUNDS and never over AVOID
    /// (the prompt box): above it (0 room there: below it). The 1st at the column of the `>`, each
    /// next level beside its parent; 0 room beside: above the dropdowns.
    pub(crate) fn render(&mut self, frame: &mut Frame, ta: &TextArea, bounds: Rect, avoid: Rect) {
        render_command(self, frame, ta, bounds, avoid);
    }

    fn menu(&mut self, level: Level, level_query: &str, items: bool) -> Vec<usize> {
        if items {
            Vec::new()
        } else {
            super::menu(level, level_query, self)
        }
    }
}

fn item_row(idx: usize, command: &mut Command) -> (String, String, bool) {
    let item = &command.items[command.matched.1[idx]];

    (item.label.clone(), item.detail.clone(), false)
}

fn render_command(command: &mut Command, frame: &mut Frame, ta: &TextArea, bounds: Rect, avoid: Rect) {
    command.rows.clear();
    command.areas.clear();

    let Some((at, query)) = command.sync(ta) else {
        return;
    };

    let count = command.settle(&query);
    let depth = super::depth(command);

    if depth == 0 && count == 0 {
        return;
    }

    let Some(screen) = ta.screen_pos(at) else { return };

    let mut layout = command_layout(bounds, avoid, screen.x);

    for level_depth in 0..=depth {
        let menu = menu_view(command, level_depth, depth, &query);

        if !draw_menu(command, frame, &menu, &mut layout) {
            return;
        }
    }
}

struct CommandLayout {
    room: Rect,
    anchor: Rect,
    up: bool,
    parent: Option<(Rect, u16)>,
    stack: Option<Rect>,
}

struct MenuView<'query> {
    level: Level,
    active: bool,
    query: &'query str,
    depth: usize,
    items: bool,
    menu: Vec<usize>,
    len: usize,
    cursor: usize,
    label: String,
    width: usize,
}

fn command_layout(bounds: Rect, avoid: Rect, x: u16) -> CommandLayout {
    // the room above the prompt box; less than 5 rows there and more below: below it
    let above = Rect {
        height: avoid.y.saturating_sub(bounds.y).min(bounds.height),
        ..bounds
    };
    let below_y = avoid.bottom().clamp(bounds.y, bounds.bottom());
    let below = Rect {
        y: below_y,
        height: bounds.bottom() - below_y,
        ..bounds
    };

    // above: more room there, or room for a full list (12 rows)
    let up = above.height >= 12 || above.height >= below.height;
    let room = if up { above } else { below };
    let anchor = Rect {
        x,
        y: if up { avoid.y } else { avoid.bottom().saturating_sub(1) },
        width: 1,
        height: 1,
    };

    CommandLayout {
        room,
        anchor,
        up,
        parent: None,
        stack: None,
    }
}

fn menu_view<'query>(command: &mut Command, depth: usize, open_depth: usize, query: &'query str) -> MenuView<'query> {
    let level = super::level(depth, command);
    let active = depth == open_depth;
    let level_query = if active { query } else { "" };
    let items = matches!(level, Level::Items(_));
    let menu: Vec<usize> = command.menu(level, level_query, items);

    let len = if items { command.matched.1.len() } else { menu.len() };

    let cursor = if !items && !active {
        menu.iter().position(|&i| i == command.cursor[depth]).unwrap_or(0)
    } else {
        command.cursor[depth]
    };

    let (label, width) = menu_label_width(command, level, &menu);

    MenuView {
        level,
        active,
        query: level_query,
        depth,
        items,
        menu,
        len,
        cursor,
        label,
        width,
    }
}

fn menu_area(menu: &MenuView<'_>, layout: &CommandLayout) -> Option<Rect> {
    let rows = menu.len.max(1);

    match (layout.parent, layout.stack) {
        (Some((area, y)), Some(stack)) => dropdown::place_beside(area, y, menu.width as u16, rows, layout.room, stack),
        _ => dropdown::place(layout.anchor, menu.width as u16, rows, layout.room, layout.up),
    }
}

fn draw_menu(command: &mut Command, frame: &mut Frame, menu_view: &MenuView<'_>, layout: &mut CommandLayout) -> bool {
    let MenuView {
        level,
        active,
        query: level_query,
        depth,
        len,
        cursor,
        label,
        ..
    } = menu_view;
    let (level, active, depth, len, cursor) = (*level, *active, *depth, *len, *cursor);

    let Some(area) = menu_area(menu_view, layout) else {
        return false;
    };

    let shown = (area.height as usize).saturating_sub(2);

    command.offset[depth] = dropdown::fit(command.offset[depth], cursor, len, shown);
    let off = command.offset[depth];

    let note = match level {
        Level::Items(_) if command.loading => "scan…".to_string(),
        Level::Items(_) if !level_query.is_empty() => format!("{len} of {}", command.items.len()),
        Level::Items(_) => format!("{len}"),
        _ => String::new(),
    };

    let inner = dropdown::frame(
        frame,
        area,
        label,
        &note,
        dropdown::FrameState {
            up: off > 0,
            down: off + shown < len,
            active,
        },
    );

    command.areas.push(area);
    layout.stack = Some(layout.stack.map_or(area, |stack| stack.union(area)));

    draw_empty_menu(command, frame, inner, len, level_query);
    let cur_y = draw_menu_rows(command, frame, inner, shown, off, menu_view);

    layout.parent = Some((area, cur_y));
    draw_item_info(command, frame, area, menu_view, layout);

    true
}

fn draw_empty_menu(command: &Command, frame: &mut Frame, inner: Rect, len: usize, level_query: &str) {
    if len == 0 {
        let text = if command.loading {
            " scan of the transcripts…"
        } else if level_query.is_empty() {
            " 0 entries"
        } else {
            " 0 matches"
        };

        frame.render_widget(
            Paragraph::new(Line::styled(text, theme::muted())),
            Rect {
                height: inner.height.min(1),
                ..inner
            },
        );
    }
}

fn draw_menu_rows(
    command: &mut Command,
    frame: &mut Frame,
    inner: Rect,
    shown: usize,
    off: usize,
    menu: &MenuView<'_>,
) -> u16 {
    let len = menu.len;
    let cursor = menu.cursor;
    let depth = menu.depth;
    let mut cur_y = inner.y;

    for i in 0..shown.min(len.saturating_sub(off)) {
        let idx = off + i;
        let row = Rect {
            y: inner.y + i as u16,
            height: 1,
            ..inner
        };
        let highlighted = idx == cursor;

        mark_current_row(&mut cur_y, row, highlighted);
        draw_menu_row(command, frame, row, idx, highlighted, menu);
        command.rows.push((row, depth, idx));
    }

    cur_y
}

fn draw_menu_row(
    command: &mut Command,
    frame: &mut Frame,
    row: Rect,
    idx: usize,
    highlighted: bool,
    view: &MenuView<'_>,
) {
    let level = view.level;
    let active = view.active;
    let items = view.items;
    let menu = &view.menu;

    let (text, detail, submenu) = match level {
        Level::Commands => (
            COMMANDS[menu[idx]].0.to_string(),
            COMMANDS[menu[idx]].1.to_string(),
            !matches!(menu[idx], BRANCH | HANDOFF | RELOAD | FLOW | SUB),
        ),
        Level::Kinds => (KINDS[menu[idx]].1.to_string(), KINDS[menu[idx]].2.to_string(), true),
        Level::Items(_) => item_row(idx, command),
    };

    let style = match (highlighted, active) {
        (true, true) => theme::pill().add_modifier(Modifier::BOLD),
        (true, false) => theme::pill_dim().add_modifier(Modifier::BOLD),
        _ => Style::default().fg(theme::pal().text),
    };

    let dim = if highlighted && active {
        style.remove_modifier(Modifier::BOLD)
    } else {
        style.fg(theme::pal().subtext0).remove_modifier(Modifier::BOLD)
    };

    // menus: a `›` on the right (a submenu opens)
    let mark = if submenu {
        " ›"
    } else if items {
        ""
    } else {
        "  "
    };

    let spans = row_spans(
        &text,
        &detail,
        mark,
        row.width,
        style,
        dim,
        matches!(level, Level::Items(Kind::Skills)),
    );

    frame.render_widget(Paragraph::new(Line::from(spans)).style(style), row);
}

fn row_spans(
    text: &str,
    detail: &str,
    mark: &str,
    width: u16,
    style: Style,
    dim: Style,
    skills: bool,
) -> Vec<Span<'static>> {
    let room_w = (width as usize).saturating_sub(2 + mark.width());

    // the label first (a path keeps its end); the detail gets the rest, cut at its end
    let text = tail(text, room_w);
    let left = room_w - text.width();

    let detail = if detail.is_empty() || left < 6 {
        String::new()
    } else {
        head(detail, left - 2)
    };

    let pad = left.saturating_sub(if detail.is_empty() { 0 } else { detail.width() + 2 });
    let mut spans = vec![Span::styled(format!(" {text}"), style)];

    // skills: the description right after the name; the others: at the right edge
    if skills {
        if !detail.is_empty() {
            spans.push(Span::styled(format!("  {detail}"), dim));
        }

        spans.push(Span::styled(" ".repeat(pad), style));
    } else {
        spans.push(Span::styled(" ".repeat(pad), style));

        if !detail.is_empty() {
            spans.push(Span::styled(format!("  {detail}"), dim));
        }
    }

    spans.push(Span::styled(format!("{mark} "), dim));

    spans
}

fn draw_item_info(command: &mut Command, frame: &mut Frame, area: Rect, view: &MenuView<'_>, layout: &CommandLayout) {
    let active = view.active;
    let items = view.items;
    let cursor = view.cursor;
    let room = layout.room;
    let stack = layout.stack;

    // the long text of the current row (a skill's description): a box above the dropdown
    if active
        && items
        && let Some(item) = command.matched.1.get(cursor).and_then(|&i| command.items.get(i))
        && !item.info.is_empty()
        && area.width > 8
    {
        let chars: Vec<char> = item.info.chars().collect();
        let lines: Vec<String> = crate::ui::textarea::wrap(&chars, area.width as usize - 4)
            .into_iter()
            .take(INFO_ROWS)
            .map(|(start, end)| chars[start..end].iter().collect::<String>().trim_end().to_string())
            .collect();
        let height = lines.len() as u16 + 2;
        let spot = info_position(area, room, stack, height);

        if let Some(y) = spot {
            let info_area = Rect { y, height, ..area };
            let inner = dropdown::frame(
                frame,
                info_area,
                &item.label,
                "",
                dropdown::FrameState {
                    up: false,
                    down: false,
                    active: false,
                },
            );

            frame.render_widget(
                Paragraph::new(
                    lines
                        .into_iter()
                        .map(|line| Line::styled(format!(" {line}"), theme::text()))
                        .collect::<Vec<_>>(),
                ),
                inner,
            );
            command.areas.push(info_area);
        }
    }
}

fn mark_current_row(cur_y: &mut u16, row: Rect, highlighted: bool) {
    if highlighted {
        *cur_y = row.y;
    }
}

fn menu_label_width(command: &Command, level: Level, menu: &[usize]) -> (String, usize) {
    match level {
        Level::Commands => (
            "commands".to_string(),
            menu.iter()
                .map(|&i| COMMANDS[i].0.width() + COMMANDS[i].1.width())
                .max()
                .unwrap_or(0)
                + 8,
        ),
        Level::Kinds => (
            COMMANDS[MENTION].0.to_string(),
            KINDS.iter().map(|row| row.1.width() + row.2.width()).max().unwrap_or(0) + 8,
        ),
        Level::Items(Kind::Skills) => (
            COMMANDS[SKILLS].0.to_string(),
            (command.items_w + 4).clamp(40, MAX_W_SKILLS),
        ),
        Level::Items(kind) => (
            format!(
                "{} › {}",
                COMMANDS[MENTION].0,
                KINDS.iter().find(|row| row.0 == kind).map_or("items", |row| row.1)
            ),
            (command.items_w + 4).clamp(28, MAX_W),
        ),
    }
}

fn info_position(area: Rect, room: Rect, stack: Option<Rect>, height: u16) -> Option<u16> {
    let top = stack.map_or(area.y, |stack| stack.y);

    if top >= room.y + height {
        Some(top - height)
    } else if area.bottom() + height <= room.bottom() && stack.is_some_and(|stack| stack.bottom() <= area.bottom()) {
        Some(area.bottom())
    } else {
        None
    }
}
