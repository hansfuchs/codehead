use super::{
    AgentRow, Buffer, Duration, Frame, Instant, Limit, Line, MORPH, MORPH_TIME, Morph, Next, Paragraph, Rect, Snap,
    Span, Style, Widget, agents, now, theme,
};

/// 250 → "250", 98049 → "98.0k", 12812803 → "12.8M".
pub(crate) fn human(value: u64) -> String {
    match value {
        0 => "·".into(),
        1..1_000 => value.to_string(),
        1_000..1_000_000 => format!("{:.1}k", value as f64 / 1e3),
        _ => format!("{:.1}M", value as f64 / 1e6),
    }
}

/// Short name of plan window LABEL: `five_hour` → `5h`, `seven_day` → `7d`, `seven_day_opus` → `7d opus`.
pub(super) fn window(label: &str) -> String {
    match label {
        "five_hour" => "5h".into(),
        "seven_day" | "week" => "7d".into(),
        other => match other.strip_prefix("seven_day_") {
            Some(rest) => format!("7d {}", rest.replace('_', " ")),
            None => other.replace('_', " "),
        },
    }
}

pub(super) fn until(reset_at: u64, now: u64) -> String {
    let remaining = reset_at.saturating_sub(now);

    match remaining {
        0..3600 => format!("{}m", remaining / 60),
        3600..86400 => format!("{}h", remaining / 3600),
        _ => format!("{}d", remaining / 86400),
    }
}

/// The next message's least cost: (tokens, $). Each recipient reads its context + the message
/// (1 model call; tools add more). 0 recipient with a known context: None.
pub(super) fn next_cost(snapshot: &Snap, next_message: &Next) -> Option<(u64, f64)> {
    let msg = (next_message.chars as u64).div_ceil(4);
    let rows: Vec<&AgentRow> = snapshot
        .agents
        .iter()
        .filter(|agent| next_message.to.contains(&agent.name) && agent.ctx_tokens > 0)
        .collect();

    if rows.is_empty() {
        return None;
    }

    let tok = rows.iter().map(|agent| agent.ctx_tokens + msg).sum();
    let usd = rows
        .iter()
        .map(|agent| (agent.ctx_tokens + msg) as f64 * agent.usd_per_token)
        .sum();

    Some((tok, usd))
}

/// Window L as a line bar of WIDTH cells: the used % `━` in text, caution / error near the limit,
/// the share of the next message `━` in info.
pub(super) fn bar(pct: f64, next: f64, width: usize) -> Vec<Span<'static>> {
    let color = if pct >= 90.0 {
        theme::pal().error()
    } else if pct >= 70.0 {
        theme::pal().caution()
    } else {
        theme::pal().text
    };

    line_bar(pct, next, width, theme::fg(color))
}

/// A line bar of WIDTH cells: PCT % `━` in USED, the next SHARE % `━` in info, the rest `─`.
pub(super) fn line_bar(pct: f64, share: f64, width: usize, used_style: Style) -> Vec<Span<'static>> {
    let cells = |percent: f64| ((percent.clamp(0.0, 100.0) / 100.0) * width as f64).round() as usize;
    let used = cells(pct).min(width);

    // a share > 0: 1 cell at least (visible)
    let share = if share > 0.0 {
        cells(share).max(1).min(width - used)
    } else {
        0
    };

    let rest = width - used - share;

    vec![
        Span::styled("━".repeat(used), used_style),
        Span::styled("━".repeat(share), theme::fg(theme::pal().info())),
        Span::styled("─".repeat(rest), theme::line()),
    ]
}

/// The cost box in AREA. NEXT: the next message (its cost line, its share in the window bars).
/// Only the cells that change move: bars and sparklines dither over, text decodes in. The last
/// box stays over a thread switch (the next thread's box morphs out of it).
pub(crate) fn draw_in(frame: &mut Frame, area: Rect, snapshot: &Snap, age: Duration, next_message: &Next) {
    let at0 = Rect::new(0, 0, area.width, area.height);
    let mut fresh = Buffer::empty(at0);

    draw_at(&mut fresh, at0, snapshot, age, now(), next_message);
    let mut morph = MORPH.lock().unwrap_or_else(std::sync::PoisonError::into_inner);

    match morph.as_mut() {
        None => {
            *morph = Some(Morph {
                from: fresh.clone(),
                to: fresh,
                start: Instant::now().checked_sub(MORPH_TIME).unwrap(),
            });
        }
        Some(current) if current.to != fresh => {
            current.from = current.shown();
            current.to = fresh;
            current.start = Instant::now();
        }
        Some(_) => {}
    }

    blit(frame, area, &morph.as_ref().expect("set above").shown());
}

/// The last cost box, in AREA (a new thread: its 1st snapshot not in yet). 0 box yet: nothing.
pub(crate) fn draw_last(frame: &mut Frame, area: Rect) {
    let morph = MORPH.lock().unwrap_or_else(std::sync::PoisonError::into_inner);

    if let Some(current) = morph.as_ref() {
        blit(frame, area, &current.shown());
    }
}

/// A cost box morph under way: frames at 30 per second.
pub(crate) fn morphing() -> bool {
    MORPH
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .as_ref()
        .is_some_and(|morph| morph.start.elapsed() < MORPH_TIME)
}

/// SHOWN (at 0, 0) into the frame at AREA.
pub(super) fn blit(frame: &mut Frame, area: Rect, shown: &Buffer) {
    let buf = frame.buffer_mut();

    for y in 0..area.height.min(shown.area.height) {
        blit_row(area, shown, buf, y);
    }
}

fn blit_row(area: Rect, shown: &Buffer, buf: &mut Buffer, y: u16) {
    for x in 0..area.width.min(shown.area.width) {
        if let Some(cell) = buf.cell_mut((area.x + x, area.y + y)) {
            *cell = shown[(x, y)].clone();
        }
    }
}

pub(super) fn draw(frame: &mut Frame, snapshot: &Snap, age: Duration, now: u64) {
    let area = frame.area();

    draw_at(frame.buffer_mut(), area, snapshot, age, now, &Next::default());
}

pub(super) fn draw_at(buf: &mut Buffer, area: Rect, snapshot: &Snap, age: Duration, now: u64, next_message: &Next) {
    Paragraph::new("").style(theme::text()).render(area, buf);
    let inner = Rect {
        x: area.x + 1,
        width: area.width.saturating_sub(2),
        ..area
    };
    let width = inner.width as usize;
    let mut lines = Vec::new();
    let cost = format!("${:.2} thread", snapshot.cost);

    let compact = area.height < 12;

    // agent sprites side by side (live first), thread cost right of the 1st row. no title: the numbers speak
    append_sprites(snapshot, width, &mut lines, &cost, compact);

    if !snapshot.found {
        if compact {
            lines.push(Line::styled(format!("{cost:>width$}"), theme::muted()));
        }

        lines.push(Line::styled("thread not found (yet)", theme::muted()));
        Paragraph::new(lines).render(inner, buf);
        return;
    }

    let name_w = append_cost_columns(snapshot, width, &mut lines, &cost, compact);

    let cost_next = next_cost(snapshot, next_message);

    append_plan_limits(snapshot, now, width, &mut lines, compact, name_w, cost_next);

    // the next message (thread view only): tokens, $, its share of each window
    append_next_cost(snapshot, next_message, &mut lines, name_w, cost_next);

    if !snapshot.status.is_empty() {
        lines.push(Line::styled(snapshot.status.clone(), theme::muted()));
    }

    Paragraph::new(lines).render(inner, buf);

    if area.height > 1 && !compact {
        let foot = Rect {
            y: area.bottom() - 1,
            height: 1,
            ..inner
        };

        Paragraph::new(Line::styled(
            format!("updated {}s ago · q close", age.as_secs()),
            theme::line(),
        ))
        .render(foot, buf);
    }
}

fn append_sprites(snapshot: &Snap, width: usize, lines: &mut Vec<Line<'_>>, cost: &str, compact: bool) {
    let fit = (width.saturating_sub(cost.len() + 1) + 2) / (crate::ui::sprites::WIDTH + 2);
    let mut by_live: Vec<&AgentRow> = snapshot.agents.iter().filter(|agent| agent.sprite.is_some()).collect();

    by_live.sort_by_key(|agent| !agent.live);

    // compact (< 12 rows, the thread view's header): 0 sprites, the cost on the column row, 0 footer

    let shown: Vec<&AgentRow> = if compact {
        Vec::new()
    } else {
        by_live.into_iter().take(fit).collect()
    };

    let rows = if compact {
        0
    } else if shown.is_empty() {
        1
    } else {
        crate::ui::sprites::HEIGHT
    };

    let frames: Vec<[String; crate::ui::sprites::HEIGHT]> = shown
        .iter()
        .map(|agent| {
            agent
                .sprite
                .as_ref()
                .expect("filtered above")
                .frame(crate::domain::avatar::Status::Idle)
        })
        .collect();

    for row in 0..rows {
        let mut spans = Vec::new();

        for (i, agent) in shown.iter().enumerate() {
            sprite_spans(&frames, row, &mut spans, i, agent);
        }

        if row == 0 {
            let used: usize = spans.iter().map(ratatui::prelude::Span::width).sum();

            spans.push(Span::raw(" ".repeat(width.saturating_sub(used + cost.len()))));
            spans.push(Span::styled(cost.to_owned(), theme::muted()));
        }

        lines.push(Line::from(spans));
    }
}

fn sprite_spans(frames: &[[String; 4]], row_index: usize, spans: &mut Vec<Span<'_>>, i: usize, agent: &AgentRow) {
    let dim = if agent.live {
        Style::default()
    } else {
        Style::default().add_modifier(ratatui::style::Modifier::DIM)
    };

    spans.push(Span::styled(
        format!("{}{}", if i > 0 { "  " } else { "" }, frames[i][row_index]),
        dim.fg(agents::color(agent.color.as_deref())),
    ));
}

fn append_cost_columns(snapshot: &Snap, width: usize, lines: &mut Vec<Line<'_>>, cost: &str, compact: bool) -> usize {
    let name_width = snapshot
        .agents
        .iter()
        .map(|agent| agent.name.chars().count())
        .max()
        .unwrap_or(3)
        .max(3)
        + 1;

    // name, tok 7, $ 9, 2, ctx line rest, 1, ctx % 4 (the last-hour sparkline: the ctrl+p tokens tab)
    let context_width = width.saturating_sub(name_width + 7 + 9 + 2 + 1 + 4);
    let head = format!(
        "{:name_width$}{:>7}{:>9}  {}",
        "",
        "tok",
        "$",
        if context_width > 0 { "context" } else { "" }
    );

    if compact {
        // the thread cost right of the column names
        let room = width.saturating_sub(head.trim_end().chars().count());
        let head = head.trim_end().to_string();

        let tail = if room > cost.len() {
            format!("{cost:>room$}")
        } else {
            String::new()
        };

        lines.push(Line::styled(format!("{head}{tail}"), theme::muted()));
    } else {
        lines.push(Line::styled(head, theme::muted()));
    }

    append_agent_costs(snapshot, lines, name_width, context_width);

    append_cost_totals(snapshot, lines, compact, name_width);

    name_width
}

fn append_cost_totals(snapshot: &Snap, lines: &mut Vec<Line<'_>>, compact: bool, name_width: usize) {
    if snapshot.agents.len() != 1 {
        lines.push(Line::from(vec![
            Span::styled(format!("{:name_width$}", "all"), theme::muted()),
            Span::styled(format!("{:>7}", human(snapshot.input + snapshot.output)), theme::text()),
            Span::styled(format!("{:>9}", format!("${:.2}", snapshot.cost)), theme::text()),
        ]));
    }

    // compact: the room goes to the window bars and the next message
    if !compact {
        lines.push(Line::styled(
            format!(
                "{} turns · cache {} read, {} write",
                snapshot.turns,
                human(snapshot.cache_read),
                human(snapshot.cache_write)
            ),
            theme::muted(),
        ));
    }
}

fn append_agent_costs(snapshot: &Snap, lines: &mut Vec<Line<'_>>, name_width: usize, context_width: usize) {
    for agent in &snapshot.agents {
        // live session: bold name
        let color = Style::default().fg(agents::color(agent.color.as_deref()));

        let name = if agent.live {
            color.add_modifier(ratatui::style::Modifier::BOLD)
        } else {
            color
        };

        let mut spans = vec![
            Span::styled(format!("{:name_width$}", agent.name), name),
            Span::styled(format!("{:>7}", human(agent.tokens)), theme::text()),
            Span::styled(format!("{:>9}", format!("${:.2}", agent.usd)), theme::text()),
            Span::raw("  "),
        ];

        // the context line: ━ the context tokens over the model's window, the agent's color
        // (dim: 0 live session, error: ≥ 90 %)
        let pct = agent.ctx.unwrap_or(0) as f64;

        let fill = if pct >= 90.0 {
            theme::fg(theme::pal().error())
        } else {
            color
        };

        let fill = if agent.live {
            fill
        } else {
            fill.add_modifier(ratatui::style::Modifier::DIM)
        };

        spans.extend(line_bar(pct, 0.0, context_width, fill));
        spans.push(Span::styled(
            format!(" {:>4}", agent.ctx.map_or("·".into(), |pct| format!("{pct}%"))),
            theme::muted(),
        ));
        lines.push(Line::from(spans));
    }
}

fn append_next_cost(
    snapshot: &Snap,
    next_message: &Next,
    lines: &mut Vec<Line<'_>>,
    name_width: usize,
    cost_next: Option<(u64, f64)>,
) {
    let mut spans = vec![Span::styled(
        format!("{:lw$} ", "next", lw = name_width.saturating_sub(1)),
        theme::muted(),
    )];

    match cost_next {
        Some((tok, usd)) => {
            spans.push(Span::styled(format!("≥{} tok", human(tok)), theme::text()));
            spans.push(Span::styled(format!(" ~${usd:.2}"), theme::muted()));

            for limit in &snapshot.limits {
                let share = limit
                    .usd_per_pct
                    .filter(|rate| *rate > 0.0)
                    .map_or("?".into(), |rate| format!("+{:.1}%", usd / rate));

                spans.push(Span::styled(format!(" · {} ", window(&limit.label)), theme::muted()));
                spans.push(Span::styled(share, theme::fg(theme::pal().info())));
            }
        }
        None => spans.push(Span::styled("· (0 context yet)", theme::muted())),
    }

    if !next_message.to.is_empty() {
        lines.push(Line::from(spans));
    }
}

#[path = "cost_render_limits.rs"]
mod limits;
use limits::append_plan_limits;
