//! Agent cards, as every screen shows them: grouped by harness (groups and agents A→Z), a heading
//! row per group (`claude · 3`), the cards in as many columns as fit. A card: sprite left, text
//! right; with a border (agents tab, task picker, F2 menu) or without (the `@` dropdown).

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph};
use unicode_width::UnicodeWidthStr;

use crate::agents::{self, Agent};
use crate::theme;

/// Rows of a card with a border.
pub const CARD_H: u16 = crate::sprites::H as u16 + 2;

/// Card positions of a grid: per card (agent index, x, virtual row of its top), the group headings
/// (virtual row, text), the virtual height.
pub struct Grid {
    pub card_width: u16,
    pub heads: Vec<(usize, String)>,
    pub places: Vec<(usize, u16, usize)>,
    pub height: usize,
}

/// Indexes of AGENTS by harness (groups and agents A→Z), only the agents KEEP lets through.
pub fn groups(agents: &[Agent], keep: impl Fn(&Agent) -> bool) -> Vec<(String, Vec<usize>)> {
    let mut groups: std::collections::BTreeMap<String, Vec<usize>> = Default::default();
    for (i, a) in agents.iter().enumerate() {
        if keep(a) {
            groups.entry(a.harness.clone()).or_default().push(i);
        }
    }
    groups
        .into_iter()
        .map(|(h, mut g)| {
            g.sort_by_key(|&i| agents[i].name.to_lowercase());
            (h, g)
        })
        .collect()
}

/// Q matches agent A: fuzzy over name, harness, model. Empty Q: every agent.
pub fn matches(a: &Agent, q: &str) -> bool {
    let q = q.trim().to_lowercase();
    q.is_empty() || crate::app::fuzzy(&q, &format!("{} {} {}", a.name, a.harness, agents::model_of(a))).is_some()
}

/// Card width for text rows of TEXT_W columns: sprite, gaps, BORDER (2 columns + 1 space each side).
pub fn card_width(text_w: usize, border: bool) -> u16 {
    (text_w + crate::sprites::W + if border { 6 } else { 3 }) as u16
}

/// Layout of GROUPS in WIDTH: per group a heading row, its card rows (CARD_H each), 1 blank row.
pub fn grid(groups: &[(String, Vec<usize>)], card_w: u16, card_h: u16, width: u16) -> Grid {
    grid_gap(groups, card_w, card_h, width, 1)
}

/// [`grid`] with 0 blank rows between the groups: bordered cards (task screen, agents tab, F2
/// menu), the border parts them already.
pub fn tight_grid(groups: &[(String, Vec<usize>)], card_w: u16, card_h: u16, width: u16) -> Grid {
    grid_gap(groups, card_w, card_h, width, 0)
}

/// Layout of GROUPS in WIDTH: per group a heading row, its card rows (CARD_H each), GAP blank rows.
fn grid_gap(groups: &[(String, Vec<usize>)], card_w: u16, card_h: u16, width: u16, gap: usize) -> Grid {
    let card_w = card_w.min(width.max(1));
    let per_row = ((width + 1) / (card_w + 1)).max(1) as usize;
    let mut heads = Vec::new();
    let mut places = Vec::new();
    let mut y = 0usize;
    for (harness, g) in groups {
        heads.push((y, format!("{harness} · {}", g.len())));
        y += 1;
        for (slot, &i) in g.iter().enumerate() {
            let x = (slot % per_row) as u16 * (card_w + 1);
            places.push((i, x, y + slot / per_row * card_h as usize));
        }
        y += g.len().div_ceil(per_row) * card_h as usize + gap;
    }
    Grid { card_width: card_w, heads, places, height: y.saturating_sub(gap) }
}

/// 1st virtual row to show so that the card at TOP (CARD_H rows) is in a VIEW of rows, its group
/// heading too when it sits right above it. OFFSET: the last value.
pub fn scroll(g: &Grid, top: usize, card_h: u16, offset: usize, view: usize) -> usize {
    let top = if g.heads.iter().any(|&(h, _)| h + 1 == top) { top - 1 } else { top };
    let mut offset = offset;
    if top < offset {
        offset = top;
    } else if top + card_h as usize + 1 > offset + view {
        offset = (top + card_h as usize + 1).saturating_sub(view);
    }
    offset.min(g.height.saturating_sub(view))
}

/// Grid G in AREA from virtual row OFFSET: the headings (bold), each card in full view through
/// CARD (frame, its area, agent index). Returns the drawn cards (clicks).
pub fn draw_grid(f: &mut Frame, area: Rect, g: &Grid, card_h: u16, offset: usize, mut card: impl FnMut(&mut Frame, Rect, usize)) -> Vec<(Rect, usize)> {
    let view = area.height as usize;
    let shown = |vy: usize, h: usize| vy >= offset && vy + h <= offset + view;
    for (vy, text) in &g.heads {
        if shown(*vy, 1) {
            let r = Rect { y: area.y + (vy - offset) as u16, height: 1, ..area };
            f.render_widget(Paragraph::new(Line::styled(text.clone(), theme::bold())), r);
        }
    }
    let mut out = Vec::new();
    for &(i, x, vy) in &g.places {
        if !shown(vy, card_h as usize) || x >= area.width {
            continue;
        }
        let r = Rect { x: area.x + x, y: area.y + (vy - offset) as u16, width: g.card_width.min(area.width - x), height: card_h };
        card(f, r, i);
        out.push((r, i));
    }
    out
}

/// `harness · model`.
pub fn tech(a: &Agent) -> String {
    let model = agents::model_of(a);
    if model.is_empty() { a.harness.clone() } else { format!("{} · {model}", a.harness) }
}

/// Effort row of A: Claude Code's global effortLevel (CLAUDE) for claude agents; other harnesses:
/// their own.
pub fn effort(a: &Agent, claude: &str) -> String {
    if a.harness == "claude" { format!("effort {claude}") } else { "effort: harness".into() }
}

/// Look of 1 card.
pub struct Look<'a> {
    /// Cursor on it: accent border, name + sprite in the agent color.
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
pub fn draw(f: &mut Frame, r: Rect, a: &Agent, rows: &[String], look: Look) {
    let color = agents::color(a.color.as_deref());
    let lit = look.cur || look.lit;
    let base = look.bg.map_or(Style::default(), |c| Style::default().bg(c));
    let inner = if let Some(border) = look.border {
        let mut block = Block::new().borders(Borders::ALL).border_type(BorderType::Plain).border_style(border).style(base);
        if let Some(t) = look.title {
            block = block.title(t);
        }
        let inner = block.inner(r);
        f.render_widget(block, r);
        Rect { x: inner.x + 1, width: inner.width.saturating_sub(1), ..inner }
    } else {
        f.render_widget(Paragraph::new("").style(base), r);
        r
    };
    let name_st = if lit { Style::default().fg(color).add_modifier(Modifier::BOLD) } else { theme::bold() };
    let room = (inner.width as usize).saturating_sub(crate::sprites::W + 2);
    let side: Vec<Vec<Span<'static>>> = rows
        .iter()
        .enumerate()
        .map(|(i, t)| vec![Span::styled(format!("  {}", crate::nav::fit(t, room)), if i == 0 { name_st } else { theme::muted() })])
        .collect();
    let sprite = agents::sprite(a);
    let lines: Vec<Line> = crate::sprites::lines(&sprite, if lit { color } else { theme::pal().muted }, side);
    f.render_widget(Paragraph::new(lines).style(base), inner);
}

/// Card border for the cursor / picked state (agent COLOR).
pub fn border(cur: bool, picked: bool, color: Color) -> Style {
    match (cur, picked) {
        (true, _) => theme::accent_bold(),
        (false, true) => Style::default().fg(color),
        (false, false) => theme::line(),
    }
}

/// Widest text row of ROWS, clamped to 10..=24 columns.
pub fn text_width<'a>(rows: impl Iterator<Item = &'a String>) -> usize {
    rows.map(|t| t.width()).max().unwrap_or(0).clamp(10, 24)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn agent(name: &str, harness: &str) -> Agent {
        Agent { name: name.into(), harness: harness.into(), model: None, color: None, mode: String::new(), prompt: None, last_seen: None }
    }

    #[test]
    fn grouped_by_harness_a_to_z() {
        let a = [agent("nova", "codex"), agent("Amon", "claude"), agent("lilith", "claude")];
        let g = groups(&a, |_| true);
        assert_eq!(g, vec![("claude".into(), vec![1, 2]), ("codex".into(), vec![0])]);
        let g = groups(&a, |x| matches(x, "cod"));
        assert_eq!(g, vec![("codex".into(), vec![0])]);
        // 2 columns of 20 in 41: claude in 1 row, codex below
        let l = grid(&groups(&a, |_| true), 20, CARD_H, 41);
        assert_eq!(l.heads, vec![(0, "claude · 2".into()), (8, "codex · 1".into())]);
        assert_eq!(l.places, vec![(1, 0, 1), (2, 21, 1), (0, 0, 9)]);
        assert_eq!(l.height, 15);
        // tight: the next heading right below the cards
        let l = tight_grid(&groups(&a, |_| true), 20, CARD_H, 41);
        assert_eq!(l.heads, vec![(0, "claude · 2".into()), (7, "codex · 1".into())]);
        assert_eq!(l.places, vec![(1, 0, 1), (2, 21, 1), (0, 0, 8)]);
        assert_eq!(l.height, 14);
    }
}
