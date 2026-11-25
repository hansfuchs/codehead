use super::{
    Block, BorderType, Borders, Clear, Frame, Level, Line, Menu, Modifier, Paragraph, Rect, Span, TABS, Tab, theme,
    thread_card,
};

impl Menu {
    /// The menu in AREA (the whole screen), as a command palette: centered on the screen's
    /// width, its top on the row below the header row (row 0).
    pub(crate) fn render(&mut self, frame_value: &mut Frame, area: Rect) {
        draw_menu(self, frame_value, area);
    }
}

fn draw_menu(widget_state: &mut Menu, frame_value: &mut Frame, area: Rect) {
    widget_state.poll();
    widget_state.poll_detail();

    // wide screens: up to 100 columns (45% of the width), 76 at least
    let width_value = area.width.saturating_sub(4).min((area.width * 45 / 100).clamp(76, 100));
    let top = area.y + 1.min(area.height.saturating_sub(8));
    let height_value = area.bottom().saturating_sub(top + 1).min(32).max(8.min(area.height));
    let area_value = Rect {
        x: area.x + (area.width - width_value) / 2,
        y: top,
        width: width_value,
        height: height_value,
    };

    widget_state.layout.frame = area_value;

    // a modal: the screen behind it fades
    theme::dim(frame_value.buffer_mut(), area);
    frame_value.render_widget(Clear, area_value);
    let block = Block::new()
        .borders(Borders::ALL)
        .border_type(BorderType::Plain)
        .border_style(theme::accent())
        .title(Span::styled(" menu ", theme::accent_bold()))
        .style(theme::text());
    let inner = block.inner(area_value);

    frame_value.render_widget(block, area_value);
    let inner = Rect {
        x: inner.x + 1,
        width: inner.width.saturating_sub(2),
        ..inner
    };

    if inner.height < 4 {
        return;
    }

    draw_tabs(widget_state, frame_value, inner);

    let body = Rect {
        y: inner.y + 2,
        height: inner.height.saturating_sub(3),
        ..inner
    };
    let foot = Rect {
        y: inner.bottom() - 1,
        height: 1,
        ..inner
    };

    widget_state.layout.rows.clear();

    // the threads tab: the list's hints (the tab bar: the bar's)
    let list_hints: Vec<(&str, &str)> = match (widget_state.tab, widget_state.level) {
        (Tab::Threads, Level::Bar) => vec![("←→", "tabs"), ("↓", "repos"), ("type", "filter"), ("esc", "close")],
        (Tab::Threads, _) => {
            let mut height_2 = widget_state.list.hints();

            if !widget_state.list.modal() {
                height_2.push(("esc", "close"));
            }

            height_2
        }
        _ => Vec::new(),
    };

    draw_menu_content(widget_state, frame_value, body, foot, &(list_hints));
}

fn draw_menu_content(
    widget_state: &mut Menu,
    frame_value: &mut Frame<'_>,
    body: Rect,
    foot: Rect,
    list_hints: &[(&str, &str)],
) {
    let hints: &[(&str, &str)] = match widget_state.tab {
        Tab::Quick => {
            widget_state.draw_quick(frame_value, body);
            &[("↑↓", "move"), ("enter", "run"), ("←→", "tabs"), ("esc", "close")]
        }
        Tab::Threads => {
            let tick = (widget_state.started.elapsed().as_millis() / thread_card::WAVE_MS) as usize;

            widget_state.list.draw(frame_value, body, tick);
            list_hints
        }
        Tab::Merge => {
            widget_state.prs.draw_levels(
                frame_value,
                body,
                widget_state.level == Level::Group,
                widget_state.level == Level::Body,
            );

            match widget_state.level {
                Level::Bar => &[("←→", "tabs"), ("↓", "states"), ("esc", "close")],
                Level::Group => &[("←→", "state"), ("↑", "tabs"), ("↓", "list"), ("esc", "close")],
                Level::Body => &[
                    ("←→", "state"),
                    ("↑↓", "move"),
                    ("enter", "open: its thread, else the merge tab"),
                    ("tab", "tabs"),
                    ("esc", "close"),
                ],
            }
        }
        Tab::Search => {
            widget_state.draw_search(frame_value, body);
            &[
                ("type", "search"),
                ("↑↓", "move"),
                ("enter", "jump to it"),
                ("←→", "tabs"),
                ("esc", "close"),
            ]
        }
        Tab::Tokens => {
            widget_state.draw_tokens(frame_value, body);
            &[("↑↓", "scroll"), ("r", "reload"), ("←→", "tabs"), ("esc", "close")]
        }
    };

    let mut spans = Vec::new();

    for (key, line) in hints {
        spans.push(Span::styled(format!("{key} "), theme::accent()));
        spans.push(Span::styled(format!("{line}   "), theme::muted()));
    }

    frame_value.render_widget(Paragraph::new(Line::from(spans)), foot);
}

fn draw_tabs(widget_state: &mut Menu, frame_value: &mut Frame<'_>, inner: Rect) {
    // tab bar
    let mut x = inner.x;

    widget_state.layout.tab_rects.clear();

    for (thread, label) in TABS {
        let text = format!(" {label} ");
        let wd = text.chars().count() as u16;

        let st = match (thread == widget_state.tab, widget_state.level) {
            (true, Level::Bar) => theme::pill().add_modifier(Modifier::BOLD),
            (true, _) => theme::panel().add_modifier(Modifier::BOLD),
            _ => theme::muted(),
        };

        let rr = Rect {
            x,
            y: inner.y,
            width: wd.min(inner.right().saturating_sub(x)),
            height: 1,
        };

        frame_value.render_widget(Paragraph::new(Span::styled(text, st)), rr);
        widget_state.layout.tab_rects.push((rr, thread));
        x += wd + 1;
    }
}
