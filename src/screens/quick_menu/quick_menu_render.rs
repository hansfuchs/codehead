use super::{
    Block, BorderType, Borders, Clear, Frame, Level, Line, Menu, Modifier, Paragraph, Rect, Span, TABS, Tab, theme,
    thread_card,
};

impl Menu {
    /// The menu in AREA (the whole screen), as a command palette: centered on the screen's
    /// width, its top on the row below the header row (row 0).
    pub(crate) fn render(&mut self, frame: &mut Frame, area: Rect) {
        draw_menu(self, frame, area);
    }
}

fn draw_menu(menu: &mut Menu, frame: &mut Frame, area: Rect) {
    menu.poll();
    menu.poll_detail();

    // wide screens: up to 100 columns (45% of the width), 76 at least
    let width = area.width.saturating_sub(4).min((area.width * 45 / 100).clamp(76, 100));
    let top = area.y + 1.min(area.height.saturating_sub(8));
    let height = area.bottom().saturating_sub(top + 1).min(32).max(8.min(area.height));
    let bounds = Rect {
        x: area.x + (area.width - width) / 2,
        y: top,
        width,
        height,
    };

    menu.layout.frame = bounds;

    // a modal: the screen behind it fades
    theme::dim(frame.buffer_mut(), area);
    frame.render_widget(Clear, bounds);
    let block = Block::new()
        .borders(Borders::ALL)
        .border_type(BorderType::Plain)
        .border_style(theme::accent())
        .title(Span::styled(" menu ", theme::accent_bold()))
        .style(theme::text());
    let inner = block.inner(bounds);

    frame.render_widget(block, bounds);
    let inner = Rect {
        x: inner.x + 1,
        width: inner.width.saturating_sub(2),
        ..inner
    };

    if inner.height < 4 {
        return;
    }

    draw_tabs(menu, frame, inner);

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

    menu.layout.rows.clear();

    // the threads tab: the list's hints (the tab bar: the bar's)
    let list_hints: Vec<(&str, &str)> = match (menu.tab, menu.level) {
        (Tab::Threads, Level::Bar) => vec![("←→", "tabs"), ("↓", "repos"), ("type", "filter"), ("esc", "close")],
        (Tab::Threads, _) => {
            let mut hints = menu.list.hints();

            if !menu.list.modal() {
                hints.push(("esc", "close"));
            }

            hints
        }
        _ => Vec::new(),
    };

    draw_menu_content(menu, frame, body, foot, &list_hints);
}

fn draw_menu_content(menu: &mut Menu, frame: &mut Frame<'_>, body: Rect, foot: Rect, list_hints: &[(&str, &str)]) {
    let hints: &[(&str, &str)] = match menu.tab {
        Tab::Quick => {
            menu.draw_quick(frame, body);
            &[("↑↓", "move"), ("enter", "run"), ("←→", "tabs"), ("esc", "close")]
        }
        Tab::Threads => {
            let tick = (menu.started.elapsed().as_millis() / thread_card::WAVE_MS) as usize;

            menu.list.draw(frame, body, tick);
            list_hints
        }
        Tab::Merge => {
            menu.prs
                .draw_levels(frame, body, menu.level == Level::Group, menu.level == Level::Body);

            match menu.level {
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
            menu.draw_search(frame, body);
            &[
                ("type", "search"),
                ("↑↓", "move"),
                ("enter", "jump to it"),
                ("←→", "tabs"),
                ("esc", "close"),
            ]
        }
        Tab::Tokens => {
            menu.draw_tokens(frame, body);
            &[("↑↓", "scroll"), ("r", "reload"), ("←→", "tabs"), ("esc", "close")]
        }
    };

    let mut spans = Vec::new();

    for (key, label) in hints {
        spans.push(Span::styled(format!("{key} "), theme::accent()));
        spans.push(Span::styled(format!("{label}   "), theme::muted()));
    }

    frame.render_widget(Paragraph::new(Line::from(spans)), foot);
}

fn draw_tabs(menu: &mut Menu, frame: &mut Frame<'_>, inner: Rect) {
    // tab bar
    let mut x = inner.x;

    menu.layout.tab_rects.clear();

    for (tab, label) in TABS {
        let text = format!(" {label} ");
        let width = text.chars().count() as u16;

        let style = match (tab == menu.tab, menu.level) {
            (true, Level::Bar) => theme::pill().add_modifier(Modifier::BOLD),
            (true, _) => theme::panel().add_modifier(Modifier::BOLD),
            _ => theme::muted(),
        };

        let rect = Rect {
            x,
            y: inner.y,
            width: width.min(inner.right().saturating_sub(x)),
            height: 1,
        };

        frame.render_widget(Paragraph::new(Span::styled(text, style)), rect);
        menu.layout.tab_rects.push((rect, tab));
        x += width + 1;
    }
}
