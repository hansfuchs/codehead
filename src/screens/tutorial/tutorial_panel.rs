use super::{
    Block, Borders, Coach, DOCS, Line, MIN_H, MIN_W, Modifier, Paragraph, Quest, Rect, Span, Style, Task, Wrap, theme,
    wrap,
};

/// The quest panel: the page of the current quest only. A row of dots (1 per quest), its title,
/// its concept (what, when), its tasks (✓ done, › next with its keys, ○ later), its docs link.
/// The basics done, 0 advanced task yet: the stop offer on top.
pub(super) fn panel_lines(coach: &Coach, width: u16) -> Vec<Line<'static>> {
    let width = width as usize;
    let cur = coach.current();
    let at = coach.quest().unwrap_or(coach.quests.len().saturating_sub(1));
    let ok = Style::default().fg(theme::pal().success());
    let mut lines = Vec::new();

    // the dots: ✓ done quests, ◆ this one, ○ later; a gap between the basics and the advanced
    let mut dots = Vec::new();

    for (i, quest) in coach.quests.iter().enumerate() {
        if i > 0 && quest.advanced && !coach.quests[i - 1].advanced {
            dots.push(Span::styled(" │", theme::muted()));
        }

        let all = quest.tasks.iter().all(|task| coach.is_done(*task));

        dots.push(match (all, i == at) {
            (true, _) => Span::styled(" ✓", ok),
            (false, true) => Span::styled(" ◆", theme::accent_bold()),
            _ => Span::styled(" ○", theme::muted()),
        });
    }

    lines.push(Line::from(dots));
    lines.push(Line::from(""));

    if coach.at_stop() {
        lines.extend(wrap(
            &(Span::styled("✓ ", ok)),
            "basics done: you know enough to work.",
            theme::bold(),
            width,
        ));
        lines.extend(wrap(
            &(Span::raw("  ")),
            "esc esc: stop here, explore the rest on your own.",
            theme::text(),
            width,
        ));
        lines.extend(wrap(
            &(Span::raw("  ")),
            "or go on: the advanced quests.",
            theme::muted(),
            width,
        ));
        lines.push(Line::from(""));
    }

    let Some(quest) = coach.quests.get(at) else {
        return lines;
    };

    let kind = if quest.advanced { "advanced" } else { "basics" };

    lines.push(Line::from(vec![
        Span::styled(format!("{}/{} ", at + 1, coach.quests.len()), theme::accent_bold()),
        Span::styled(quest.title, theme::bold()),
        Span::styled(format!("  {kind}"), theme::muted()),
    ]));

    append_quest_details(coach, width, cur, ok, lines, quest)
}

fn append_quest_details(
    coach: &Coach,
    width: usize,
    cur: Option<Task>,
    ok: Style,
    mut lines: Vec<Line<'static>>,
    quest: &Quest,
) -> Vec<Line<'static>> {
    lines.push(Line::from(""));

    for paragraph in quest.about {
        lines.extend(wrap(
            &(Span::styled("│ ", theme::line())),
            paragraph,
            theme::text(),
            width,
        ));
    }

    lines.push(Line::from(""));

    for task in &quest.tasks {
        let (what, keys) = coach.text(*task);

        if what.is_empty() {
            continue;
        }

        if coach.is_done(*task) {
            lines.extend(wrap(
                &(Span::styled("  ✓ ", ok)),
                &what,
                theme::muted().add_modifier(Modifier::CROSSED_OUT),
                width,
            ));
        } else if Some(*task) == cur {
            lines.extend(wrap(
                &(Span::styled("  › ", theme::accent())),
                &what,
                theme::text(),
                width,
            ));
            lines.push(Line::from(vec![
                Span::raw("    "),
                Span::styled(format!(" {keys} "), theme::pill()),
            ]));
        } else {
            lines.extend(wrap(
                &(Span::styled("  ○ ", theme::muted())),
                &what,
                theme::muted(),
                width,
            ));
        }
    }

    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("more: ", theme::muted()),
        Span::styled(format!("{DOCS}{}", quest.doc), theme::accent()),
    ]));

    lines
}

/// The rows around the view: header, the border, the quest panel (or the task on the top row),
/// the footer.
pub(super) fn chrome(frame: &mut ratatui::Frame, coach: &Coach, dry: bool, view: Rect, side: Option<Rect>) {
    let geometry = draw_header(frame, dry, view, side);

    draw_quest_panel(frame, coach, side, geometry.border);
    draw_footer(frame, coach.finished(), geometry);
}

#[derive(Clone, Copy)]
struct ChromeLayout {
    full: Rect,
    border: Rect,
    x: u16,
    width: u16,
}
fn draw_header(frame: &mut ratatui::Frame, dry: bool, view: Rect, side: Option<Rect>) -> ChromeLayout {
    let full = frame.area();
    let x = view.x.saturating_sub(1);
    let right = side.map_or(view.right() + 1, ratatui::prelude::Rect::right);
    let width = (right - x).min(full.width.saturating_sub(x));
    let mut head = vec![
        Span::styled("codehead", theme::bold()),
        Span::styled("  try it", theme::accent_bold()),
    ];

    if dry {
        head.push(Span::styled("  dry run", theme::muted()));
    }

    // shrunk after the start: a hint, the tutorial goes on
    if full.width < MIN_W || full.height < MIN_H {
        head.push(Span::styled(
            format!(
                "  · small window {} × {}: best from {MIN_W} × {MIN_H}",
                full.width, full.height
            ),
            theme::fg(theme::pal().caution()),
        ));
    }

    frame.render_widget(
        Paragraph::new(Line::from(head)),
        Rect {
            x,
            y: 1,
            width,
            height: 1,
        },
    );
    let border = Rect {
        x,
        y: view.y - 1,
        width: view.width + 2,
        height: view.height + 2,
    };

    frame.render_widget(
        Block::default().borders(Borders::ALL).border_style(theme::line()),
        border,
    );

    ChromeLayout { full, border, x, width }
}

fn draw_quest_panel(frame: &mut ratatui::Frame, coach: &Coach, side: Option<Rect>, border: Rect) {
    let x = border.x;

    if let Some(side_area) = side {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(theme::accent())
            .title(Span::styled(" quests ", theme::accent_bold()));
        let inner = block.inner(side_area);

        frame.render_widget(block, side_area);

        // the whole inner area: a shorter list leaves 0 old text
        let width = inner.width.saturating_sub(1);

        frame.render_widget(
            Paragraph::new(panel_lines(coach, width)).wrap(Wrap { trim: false }),
            Rect {
                x: inner.x + 1,
                width,
                ..inner
            },
        );
    } else {
        let line = coach
            .current()
            .map(|task| coach.text(task))
            .map(|(what, keys)| format!("› {what}  [{keys}]"))
            .unwrap_or_default();

        frame.render_widget(
            Paragraph::new(Span::styled(
                format!("{line:<width$}", width = border.width as usize),
                theme::accent_bold(),
            )),
            Rect {
                x,
                y: 2,
                width: border.width,
                height: 1,
            },
        );
    }
}

fn draw_footer(frame: &mut ratatui::Frame, finished: bool, geometry: ChromeLayout) {
    let ChromeLayout { full, x, width, .. } = geometry;
    let leave = if finished { "next" } else { "back" };
    let keys = [
        ("esc esc", leave),
        ("ctrl+enter", "send"),
        ("ctrl+p", "menu"),
        ("F1-F4", "views"),
        ("ctrl+c ctrl+c", "quit"),
    ];
    let mut spans = Vec::new();

    for (i, (key, what)) in keys.iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw("  "));
        }

        spans.push(Span::styled(*key, theme::bold()));
        spans.push(Span::styled(format!(" {what}"), theme::muted()));
    }

    frame.render_widget(
        Paragraph::new(Line::from(spans)),
        Rect {
            x,
            y: full.height.saturating_sub(2),
            width,
            height: 1,
        },
    );
}
