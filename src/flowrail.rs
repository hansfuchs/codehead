//! The rail of a flow: 1 row per node (number, mark, title, tags), loops and gate outcomes as arcs
//! right of the titles. Each arc has its own column (shorter arcs inside) so nested loops stay
//! readable; the target end has `◀`, the label sits right of the arcs on the source row. The flow
//! builder and the thread's flow box draw with it.
//!
//! ```text
//!  3  plan              ◀──┐
//!  4  owner-gate     ⏸  ───┘ changes
//!  6  peer-review       ◀──┐
//!  7  address-review    ───┘ ↺ until 0 findings · max 3
//! ```

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::engine::flows::Resolved;
use crate::theme;

#[derive(Clone, Debug, Default)]
pub struct Row {
    pub mark: (String, Style),
    pub title: String,
    pub title_style: Style,
    /// Shown right of the title, before the arcs.
    pub tags: Vec<(String, Style)>,
}

/// An arc FROM (its label) TO another node.
#[derive(Clone, Debug, PartialEq)]
pub struct Arc {
    pub from: usize,
    pub to: usize,
    pub label: String,
    pub loops: bool,
}

/// The arcs of NODES: loops (back), gate outcomes.
pub fn arcs(nodes: &[Resolved], rounds: impl Fn(&str) -> u32) -> Vec<Arc> {
    let mut v = Vec::new();
    for (i, n) in nodes.iter().enumerate() {
        if let Some((to, until, max)) = &n.loops {
            let r = rounds(&n.id);
            let round = if r > 0 { format!(" · round {}/{max}", r + 1) } else { format!(" · max {max}") };
            v.push(Arc { from: i, to: *to, label: format!("↺ {}{round}", if until.is_empty() { "loop" } else { until }), loops: true });
        }
        for (k, to) in &n.edges {
            v.push(Arc { from: i, to: *to, label: k.clone(), loops: false });
        }
    }
    v
}

/// Columns the arcs take right of the titles.
pub fn arcs_width(arcs: &[Arc]) -> u16 {
    if arcs.is_empty() { 0 } else { 2 + 3 * arcs.len() as u16 }
}

/// Draw ROWS from OFFSET into AREA (1 row each), the arcs, row CURSOR highlighted (FOCUS: the
/// accent pill, else the panel bg). Returns the rects of the drawn rows: (rect, row index).
pub fn draw(buf: &mut Buffer, area: Rect, rows: &[Row], arcs: &[Arc], cursor: Option<usize>, focus: bool, offset: usize) -> Vec<(Rect, usize)> {
    let mut hits = Vec::new();
    if area.width < 12 || area.height == 0 {
        return hits;
    }
    // arcs: shortest span innermost (column 0 = nearest the titles)
    let mut order: Vec<usize> = (0..arcs.len()).collect();
    order.sort_by_key(|&k| (arcs[k].from.abs_diff(arcs[k].to), arcs[k].from));
    let aw = arcs_width(arcs).min(area.width / 2);
    // the titles' column: as wide as the widest row (the arcs right next to it), the labels
    // keep 12+ columns
    let natural = rows.iter().map(|r| 3 + r.mark.0.width() + 1 + r.title.width() + r.tags.iter().map(|(t, _)| t.width() + 1).sum::<usize>() + 1).max().unwrap_or(10) as u16;
    let room = if arcs.is_empty() { area.width } else { area.width.saturating_sub(aw + 13) };
    let text_w = natural.min(room).max(10).min(area.width);
    let shown = usize::from(area.height);
    for (k, row) in rows.iter().enumerate().skip(offset).take(shown) {
        let y = area.y + (k - offset) as u16;
        let r = Rect { y, height: 1, ..area };
        let on = cursor == Some(k);
        let base = if on {
            if focus { theme::pill().add_modifier(Modifier::BOLD) } else { theme::panel().add_modifier(Modifier::BOLD) }
        } else {
            Style::default()
        };
        if on {
            for x in r.x..r.right() {
                buf[(x, y)].set_symbol(" ").set_style(base);
            }
        }
        let mut x = area.x;
        let put = |buf: &mut Buffer, x: &mut u16, s: &str, st: Style, end: u16| {
            for ch in s.chars() {
                let w = ch.width().unwrap_or(0) as u16;
                if *x + w > end {
                    break;
                }
                let st = if on { base.patch(Style { fg: if focus { None } else { st.fg }, ..Style::default() }) } else { st };
                buf[(*x, y)].set_symbol(&ch.to_string()).set_style(st);
                *x += w.max(1);
            }
        };
        let end = area.x + text_w;
        put(buf, &mut x, &format!("{:>2} ", k + 1), theme::muted(), end);
        put(buf, &mut x, &format!("{} ", row.mark.0), row.mark.1, end);
        let tags_w: u16 = row.tags.iter().map(|(t, _)| t.width() as u16 + 1).sum();
        let title_end = end.saturating_sub(tags_w).max(x + 4);
        let title = crate::nav::fit(&row.title, usize::from(title_end.saturating_sub(x)));
        put(buf, &mut x, &title, if row.title_style == Style::default() { theme::text() } else { row.title_style }, title_end);
        x = x.max(title_end);
        for (t, st) in &row.tags {
            put(buf, &mut x, " ", Style::default(), end);
            put(buf, &mut x, t, *st, end);
        }
        hits.push((r, k));
    }
    // arcs
    let ax = area.x + text_w + 1;
    for (col, &k) in order.iter().enumerate() {
        let a = &arcs[k];
        let cx = ax + 2 + 3 * col as u16;
        if cx >= area.right() {
            break;
        }
        let st = if a.loops { theme::accent() } else { Style::default().fg(theme::pal().warn) };
        let (top, bottom) = (a.from.min(a.to), a.from.max(a.to));
        let row_y = |i: usize| -> Option<u16> { (i >= offset && i < offset + shown).then(|| area.y + (i - offset) as u16) };
        let set = |buf: &mut Buffer, x: u16, y: u16, s: &str| {
            if x < area.right() {
                buf[(x, y)].set_symbol(s).set_style(st);
            }
        };
        if a.from == a.to {
            if let Some(y) = row_y(a.from) {
                set(buf, ax, y, "↺");
            }
        } else {
            for i in top..=bottom {
                let Some(y) = row_y(i) else { continue };
                if i == top || i == bottom {
                    // the end rows: a horizontal run to the column, the corner, the arrow at the target
                    let target = i == a.to;
                    set(buf, ax, y, if target { "◀" } else { "─" });
                    for x in ax + 1..cx {
                        let cur = buf[(x, y)].symbol().to_string();
                        if cur == "│" {
                            set(buf, x, y, "┼");
                        } else {
                            set(buf, x, y, "─");
                        }
                    }
                    set(buf, cx, y, if i == top { "┐" } else { "┘" });
                } else {
                    set(buf, cx, y, "│");
                }
            }
        }
        // the label right of every arc column, on the source row
        if let Some(y) = row_y(a.from) {
            let lx = ax + aw;
            let mut x = lx;
            for ch in crate::nav::fit(&a.label, usize::from(area.right().saturating_sub(lx))).chars() {
                if x >= area.right() {
                    break;
                }
                buf[(x, y)].set_symbol(&ch.to_string()).set_style(st);
                x += ch.width().unwrap_or(1).max(1) as u16;
            }
        }
    }
    hits
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(buf: &Buffer, area: Rect) -> Vec<String> {
        (area.y..area.bottom()).map(|y| (area.x..area.right()).map(|x| buf[(x, y)].symbol().to_string()).collect::<String>().trim_end().to_string()).collect()
    }

    #[test]
    fn nested_arcs_take_their_own_columns() {
        let rows: Vec<Row> = ["a", "b", "c", "d"].iter().map(|t| Row { mark: ("○".into(), Style::default()), title: t.to_string(), ..Default::default() }).collect();
        let arcs = vec![Arc { from: 3, to: 0, label: "outer".into(), loops: true }, Arc { from: 2, to: 1, label: "inner".into(), loops: false }];
        let area = Rect::new(0, 0, 40, 4);
        let mut buf = Buffer::empty(area);
        draw(&mut buf, area, &rows, &arcs, Some(1), true, 0);
        let t = text(&buf, area);
        assert!(t[0].contains("◀────┐"), "{t:?}");
        assert!(t[1].contains("◀─┐  │"), "{t:?}");
        assert!(t[2].contains("──┘  │") && t[2].ends_with("inner"), "{t:?}");
        assert!(t[3].contains("─────┘") && t[3].ends_with("outer"), "{t:?}");
    }
}
