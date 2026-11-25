//! Agent cards, as every screen shows them: grouped by harness (groups and agents A→Z), a heading
//! row per group (`claude · 3`), the cards in as many columns as fit. A card: sprite left, text
//! right; with a border (agents tab, task picker, F2 menu) or without (the `@` dropdown).

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph};
use unicode_width::UnicodeWidthStr;

use crate::domain::agents::{self, Agent};
use crate::ui::theme;

/// Rows of a card with a border.
pub(crate) const CARD_H: u16 = crate::ui::sprites::HEIGHT as u16 + 2;

/// Card positions of a grid: per card (agent index, x, virtual row of its top), the group headings
/// (virtual row, text), the virtual height.
pub(crate) struct Grid {
    pub card_width: u16,
    pub heads: Vec<(usize, String)>,
    pub places: Vec<(usize, u16, usize)>,
    pub height: usize,
}

/// Indexes of AGENTS by harness (groups and agents A→Z), only the agents KEEP lets through.
pub(crate) fn groups(agents: &[Agent], keep: impl Fn(&Agent) -> bool) -> Vec<(String, Vec<usize>)> {
    let mut groups: std::collections::BTreeMap<String, Vec<usize>> = std::collections::BTreeMap::default();

    for (i, agent) in agents.iter().enumerate() {
        if keep(agent) {
            groups.entry(agent.harness.clone()).or_default().push(i);
        }
    }

    groups
        .into_iter()
        .map(|(harness, mut group)| {
            group.sort_by_key(|&i| agents[i].name.to_lowercase());
            (harness, group)
        })
        .collect()
}

/// Q matches agent A: fuzzy over name, harness, model. Empty Q: every agent.
pub(crate) fn matches(agent: &Agent, query: &str) -> bool {
    let query = query.trim().to_lowercase();

    query.is_empty()
        || crate::domain::fuzzy::fuzzy(
            &query,
            &format!("{} {} {}", agent.name, agent.harness, agents::model_of(agent)),
        )
        .is_some()
}

/// Card width for text rows of `TEXT_W` columns: sprite, gaps, BORDER (2 columns + 1 space each side).
pub(crate) fn card_width(text_w: usize, border: bool) -> u16 {
    (text_w + crate::ui::sprites::WIDTH + if border { 6 } else { 3 }) as u16
}

/// Layout of GROUPS in WIDTH: per group a heading row, its card rows (`CARD_H` each), 1 blank row.
pub(crate) fn grid(groups: &[(String, Vec<usize>)], card_w: u16, card_h: u16, width: u16) -> Grid {
    grid_gap(groups, card_w, card_h, width, 1)
}

/// [`grid`] with 0 blank rows between the groups: bordered cards (task screen, agents tab, F2
/// menu), the border parts them already.
pub(crate) fn tight_grid(groups: &[(String, Vec<usize>)], card_w: u16, card_h: u16, width: u16) -> Grid {
    grid_gap(groups, card_w, card_h, width, 0)
}

/// Layout of GROUPS in WIDTH: per group a heading row, its card rows (`CARD_H` each), GAP blank rows.
fn grid_gap(groups: &[(String, Vec<usize>)], card_w: u16, card_h: u16, width: u16, gap: usize) -> Grid {
    let card_w = card_w.min(width.max(1));
    let per_row = ((width + 1) / (card_w + 1)).max(1) as usize;
    let mut heads = Vec::new();
    let mut places = Vec::new();
    let mut y = 0usize;

    for (harness, group) in groups {
        heads.push((y, format!("{harness} · {}", group.len())));
        y += 1;

        for (slot, &i) in group.iter().enumerate() {
            let x = (slot % per_row) as u16 * (card_w + 1);

            places.push((i, x, y + slot / per_row * card_h as usize));
        }

        y += group.len().div_ceil(per_row) * card_h as usize + gap;
    }

    Grid {
        card_width: card_w,
        heads,
        places,
        height: y.saturating_sub(gap),
    }
}

/// 1st virtual row to show so that the card at TOP (`CARD_H` rows) is in a VIEW of rows, its group
/// heading too when it sits right above it. OFFSET: the last value.
pub(crate) fn scroll(layout: &Grid, top: usize, card_h: u16, offset: usize, view: usize) -> usize {
    let bottom = top + card_h as usize;

    let top = if layout.heads.iter().any(|&(row, _)| row + 1 == top) {
        top - 1
    } else {
        top
    };

    let mut offset = offset;

    if top < offset {
        offset = top;
    } else if bottom > offset + view {
        offset = bottom.saturating_sub(view);
    }

    let offset = offset.min(layout.height.saturating_sub(view));

    // never start inside a card or a heading: `draw_grid` skips a cut card, its rows stay blank
    starts(layout)
        .into_iter()
        .find(|&start| start >= offset && start <= top)
        .unwrap_or(offset)
}

/// The virtual rows where a heading or a row of cards starts, ascending.
fn starts(layout: &Grid) -> Vec<usize> {
    let mut rows: Vec<usize> = layout
        .heads
        .iter()
        .map(|&(y, _)| y)
        .chain(layout.places.iter().map(|&(_, _, y)| y))
        .collect();

    rows.sort_unstable();
    rows.dedup();
    rows
}

/// Rows of a VIEW that `draw_grid` fills from OFFSET: the bottom of the last whole heading or card.
pub(crate) fn used(layout: &Grid, card_h: u16, offset: usize, view: usize) -> usize {
    let fits = |vy: usize, height: usize| vy >= offset && vy + height <= offset + view;

    // a heading counts with its 1st card row only: 0 dangling headings at the bottom
    let heads = layout
        .heads
        .iter()
        .filter(|&&(y, _)| fits(y, 1 + card_h as usize))
        .map(|&(y, _)| y + 1);
    let cards = layout
        .places
        .iter()
        .filter(|&&(_, _, y)| fits(y, card_h as usize))
        .map(|&(_, _, y)| y + card_h as usize);

    heads.chain(cards).max().map_or(0, |bottom| bottom - offset)
}

/// Grid G in AREA from virtual row OFFSET: the headings (bold), each card in full view through
/// CARD (frame, its area, agent index). Returns the drawn cards (clicks).
pub(crate) fn draw_grid(
    frame: &mut Frame,
    area: Rect,
    layout: &Grid,
    card_h: u16,
    offset: usize,
    mut card: impl FnMut(&mut Frame, Rect, usize),
) -> Vec<(Rect, usize)> {
    let view = area.height as usize;
    let shown = |vy: usize, height: usize| vy >= offset && vy + height <= offset + view;

    for (vy, heading) in &layout.heads {
        if shown(*vy, 1) {
            let head_area = Rect {
                y: area.y + (vy - offset) as u16,
                height: 1,
                ..area
            };

            frame.render_widget(Paragraph::new(Line::styled(heading.clone(), theme::bold())), head_area);
        }
    }

    let mut out = Vec::new();

    for &(i, x, vy) in &layout.places {
        if !shown(vy, card_h as usize) || x >= area.width {
            continue;
        }

        let card_area = Rect {
            x: area.x + x,
            y: area.y + (vy - offset) as u16,
            width: layout.card_width.min(area.width - x),
            height: card_h,
        };

        card(frame, card_area, i);
        out.push((card_area, i));
    }

    out
}

/// `harness · model`.
pub(crate) fn tech(agent: &Agent) -> String {
    let model = agents::model_of(agent);

    if model.is_empty() {
        agent.harness.clone()
    } else {
        format!("{} · {model}", agent.harness)
    }
}

/// Effort row of A: Claude Code's global effortLevel (CLAUDE) for claude agents; other harnesses:
/// their own.
pub(crate) fn effort(agent: &Agent, claude: &str) -> String {
    if agent.harness == "claude" {
        format!("effort {claude}")
    } else {
        "effort: harness".into()
    }
}

/// Look of 1 card.
pub(crate) struct Look<'a> {
    /// Cursor on it: accent border (+ accent bar on its far left column), name + sprite in the
    /// agent color.
    pub cur: bool,
    /// Name + sprite in the agent color without the cursor (picked).
    pub lit: bool,
    /// Border (None: 0 border). Default: accent (cursor), else gray.
    pub border: Option<Style>,
    pub title: Option<Span<'a>>,
    /// Background of the whole card (dropdown: its bg, highlighted row: hover).
    pub bg: Option<Color>,
}

/// Card of agent A in R: sprite left, ROWS right (the 1st: the name, bold).
pub(crate) fn draw(frame: &mut Frame, area: Rect, agent: &Agent, rows: &[String], look: Look) {
    let color = agents::color(agent.color.as_deref());
    let lit = look.cur || look.lit;
    let base = look.bg.map_or(Style::default(), |bg| Style::default().bg(bg));

    let inner = if let Some(border) = look.border {
        let mut block = Block::new()
            .borders(Borders::ALL)
            .border_type(BorderType::Plain)
            .border_style(border)
            .style(base);

        if let Some(title) = look.title {
            block = block.title(title);
        }

        let inner = block.inner(area);

        frame.render_widget(block, area);
        Rect {
            x: inner.x + 1,
            width: inner.width.saturating_sub(1),
            ..inner
        }
    } else {
        frame.render_widget(Paragraph::new("").style(base), area);
        area
    };

    let name_st = if lit {
        Style::default().fg(color).add_modifier(Modifier::BOLD)
    } else {
        theme::bold()
    };

    let room = (inner.width as usize).saturating_sub(crate::ui::sprites::WIDTH + 2);

    let side: Vec<Vec<Span<'static>>> = rows
        .iter()
        .enumerate()
        .map(|(i, row)| {
            vec![Span::styled(
                format!("  {}", crate::ui::thread_card::fit(row, room)),
                if i == 0 { name_st } else { theme::muted() },
            )]
        })
        .collect();

    let sprite = agents::sprite(agent).desynced(&agent.name);
    let lines: Vec<Line> = crate::ui::sprites::lines(&sprite, if lit { color } else { theme::pal().subtext0 }, side);

    frame.render_widget(Paragraph::new(lines).style(base), inner);

    if look.cur && look.border.is_some() {
        theme::accent_edge(frame.buffer_mut(), area);
    }
}

/// Card border for the cursor / picked state (agent COLOR).
pub(crate) fn border(cur: bool, picked: bool, color: Color) -> Style {
    match (cur, picked) {
        (true, _) => theme::accent_bold(),
        (false, true) => Style::default().fg(color),
        (false, false) => theme::line(),
    }
}

/// Widest text row of ROWS, clamped to 10..=24 columns.
pub(crate) fn text_width<'a>(rows: impl Iterator<Item = &'a String>) -> usize {
    rows.map(|row| row.width()).max().unwrap_or(0).clamp(10, 24)
}

#[cfg(test)]
#[path = "cards_tests.rs"]
mod tests;
