use super::{
    Buffer, Frame, Instant, Limit, Line, MORPH, MORPH_TIME, Morph, Next, Paragraph, Rect, Show, Snap, Span, Style,
    Widget, compact_lines, expanded_lines, now, theme,
};
use ratatui::style::Color;

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
    let rows: Vec<_> = snapshot
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
        .map(|agent| (agent.ctx_tokens + msg) as f64 * agent.usage.usd_per_token)
        .sum();

    Some((tok, usd))
}

/// The next message's share of LIMIT, percent (0: 0 rate or 0 $ yet).
pub(super) fn share_of(limit: &Limit, cost_next: Option<(u64, f64)>) -> f64 {
    match (cost_next, limit.usd_per_pct) {
        (Some((_, usd)), Some(rate)) if rate > 0.0 => usd / rate,
        _ => 0.0,
    }
}

/// A plan window as a dithered bar of WIDTH cells: used on a solid gray (caution / error near the
/// limit), the next message's share `▒` in info, the rest `░`.
pub(super) fn window_bar(pct: f64, share: f64, width: usize) -> Vec<Span<'static>> {
    let color = if pct >= 90.0 {
        theme::pal().error()
    } else if pct >= 70.0 {
        theme::pal().caution()
    } else {
        theme::pal().overlay0
    };

    dither_bar(pct, share, width, color)
}

/// A bar of WIDTH cells: PCT % solid USED (background), the SHARE % `▒` in info, the rest `░`.
pub(super) fn dither_bar(pct: f64, share: f64, width: usize, used_color: Color) -> Vec<Span<'static>> {
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
        Span::styled(" ".repeat(used), Style::default().bg(used_color)),
        Span::styled("▒".repeat(share), theme::fg(theme::pal().info())),
        Span::styled("░".repeat(rest), theme::line()),
    ]
}

/// Rows of the compact box of a thread with HARNESSES and AGENTS: 1 per harness, 1 per agent.
pub(crate) fn box_rows(harnesses: usize, agents: usize) -> u16 {
    (harnesses.max(1) + agents.max(1)) as u16
}

/// Rows the expanded box wants at WIDTH (inside its frame), the compact rows incl.
pub(crate) fn expanded_rows(snapshot: &Snap, show: Show, width: u16) -> u16 {
    let lines = expanded_lines(snapshot, show, now(), width as usize).len()
        + compact_lines(snapshot, width as usize, now(), None).len()
        + 1;

    lines as u16
}

/// The box in AREA: compact (the plan windows and contexts), or expanded (the details above
/// them). Only the cells that change move: bars dither over, text decodes in. The last box stays
/// over a thread switch (the next thread's box morphs out of it).
pub(crate) fn draw_in(frame: &mut Frame, area: Rect, snapshot: &Snap, show: Show) {
    let at0 = Rect::new(0, 0, area.width, area.height);
    let mut fresh = Buffer::empty(at0);

    draw_at(&mut fresh, at0, snapshot, now(), show);
    let mut morph = MORPH.lock().unwrap_or_else(std::sync::PoisonError::into_inner);

    match morph.as_mut() {
        // a new size (expanded, collapsed, a resize): the new box at once, 0 mix of 2 sizes
        Some(current) if current.to.area != fresh.area => {
            *current = settled(fresh);
        }
        None => *morph = Some(settled(fresh)),
        Some(current) if current.to != fresh => {
            current.from = current.shown();
            current.to = fresh;
            current.start = Instant::now();
        }
        Some(_) => {}
    }

    blit(frame, area, &morph.as_ref().expect("set above").shown());
}

fn settled(fresh: Buffer) -> Morph {
    Morph {
        from: fresh.clone(),
        to: fresh,
        start: Instant::now().checked_sub(MORPH_TIME).unwrap_or_else(Instant::now),
    }
}

/// The last box, in AREA (a new thread: its 1st snapshot not in yet). 0 box yet, or a box of
/// another size: nothing.
pub(crate) fn draw_last(frame: &mut Frame, area: Rect) {
    let morph = MORPH.lock().unwrap_or_else(std::sync::PoisonError::into_inner);

    if let Some(current) = morph.as_ref().filter(|current| current.to.area.width == area.width) {
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

/// The box into BUF at AREA: compact rows at its bottom; expanded: the details above them
/// (scrolled; the compact rows stay).
pub(super) fn draw_at(buf: &mut Buffer, area: Rect, snapshot: &Snap, now: u64, show: Show) {
    Paragraph::new("").style(theme::text()).render(area, buf);
    let width = area.width as usize;

    if !snapshot.found {
        Paragraph::new(Line::styled("thread not found (yet)", theme::muted())).render(area, buf);
        return;
    }

    let cost_next = super::render::next_cost(snapshot, show.next);
    let compact = compact_lines(snapshot, width, now, cost_next);
    let bottom = (compact.len() as u16).min(area.height);
    let low = Rect {
        y: area.bottom() - bottom,
        height: bottom,
        ..area
    };

    Paragraph::new(compact).render(low, buf);

    let Some(expanded) = show.expanded else {
        return;
    };

    // the details, 1 blank row over the compact rows
    let high = Rect {
        height: area.height.saturating_sub(bottom + 1),
        ..area
    };
    let lines = expanded_lines(snapshot, show, now, width);
    let max = (lines.len() as u16).saturating_sub(high.height);

    Paragraph::new(lines)
        .scroll((expanded.scroll.min(max), 0))
        .render(high, buf);
}
