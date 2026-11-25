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

use std::ops::ControlFlow;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::engine::flows::Resolved;
use crate::ui::theme;

#[derive(Clone, Debug, Default)]
pub(crate) struct Row {
    pub mark: (String, Style),
    pub title: String,
    pub title_style: Style,
    /// Shown right of the title, before the arcs.
    pub tags: Vec<(String, Style)>,
}

/// An arc FROM (its label) TO another node.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Arc {
    pub from: usize,
    pub to: usize,
    pub label: String,
    pub loops: bool,
}

/// The arcs of NODES: loops (back), gate outcomes.
pub(crate) fn arcs(nodes: &[Resolved], rounds: impl Fn(&str) -> u32) -> Vec<Arc> {
    let mut arcs = Vec::new();

    for (i, node) in nodes.iter().enumerate() {
        push_node_arcs(&rounds, &mut arcs, i, node);
    }

    arcs
}

fn push_node_arcs(rounds: &impl Fn(&str) -> u32, arcs: &mut Vec<Arc>, i: usize, node: &Resolved) {
    if let Some((to, until, max)) = &node.loops {
        let done = rounds(&node.id);

        let round = if done > 0 {
            format!(" · round {}/{max}", done + 1)
        } else {
            format!(" · max {max}")
        };

        arcs.push(Arc {
            from: i,
            to: *to,
            label: format!("↺ {}{round}", if until.is_empty() { "loop" } else { until }),
            loops: true,
        });
    }

    for (key, to) in &node.edges {
        arcs.push(Arc {
            from: i,
            to: *to,
            label: key.clone(),
            loops: false,
        });
    }
}

/// Columns the arcs take right of the titles.
pub(crate) fn arcs_width(arcs: &[Arc]) -> u16 {
    if arcs.is_empty() { 0 } else { 2 + 3 * arcs.len() as u16 }
}

/// Draw ROWS from OFFSET into AREA (1 row each), the arcs, row CURSOR highlighted (FOCUS: the
/// accent pill, else the panel bg). Returns the rects of the drawn rows: (rect, row index).
pub(crate) fn draw(
    buf: &mut Buffer,
    area: Rect,
    rows: &[Row],
    arcs: &[Arc],
    cursor: Option<usize>,
    focus: bool,
    offset: usize,
) -> Vec<(Rect, usize)> {
    let hits = Vec::new();

    if area.width < 12 || area.height == 0 {
        return hits;
    }

    // arcs: shortest span innermost (column 0 = nearest the titles)
    let mut order: Vec<usize> = (0..arcs.len()).collect();

    order.sort_by_key(|&arc| (arcs[arc].from.abs_diff(arcs[arc].to), arcs[arc].from));
    let arcs_w = arcs_width(arcs).min(area.width / 2);

    // the titles' column: as wide as the widest row (the arcs right next to it), the labels
    // keep 12+ columns
    let natural = rows
        .iter()
        .map(|row| {
            3 + row.mark.0.width()
                + 1
                + row.title.width()
                + row.tags.iter().map(|(tag, _)| tag.width() + 1).sum::<usize>()
                + 1
        })
        .max()
        .unwrap_or(10) as u16;

    let room = if arcs.is_empty() {
        area.width
    } else {
        area.width.saturating_sub(arcs_w + 13)
    };

    let text_w = natural.min(room).max(10).min(area.width);
    let shown = usize::from(area.height);

    draw_rows_and_arcs(
        buf,
        RailViewport {
            area,
            cursor,
            focus,
            offset,
            arcs_w,
            text_w,
        },
        rows,
        arcs,
        hits,
        &order,
        shown,
    )
}

fn draw_rows_and_arcs(
    buf: &mut Buffer,
    rail_viewport: RailViewport,
    rows: &[Row],
    arcs: &[Arc],
    mut hits: Vec<(Rect, usize)>,
    order: &[usize],
    shown: usize,
) -> Vec<(Rect, usize)> {
    let RailViewport {
        area,
        cursor,
        focus,
        offset,
        arcs_w,
        text_w,
    } = rail_viewport;

    for (index, row) in rows.iter().enumerate().skip(offset).take(shown) {
        draw_row(
            buf,
            RailRowLayout {
                area,
                cursor,
                focus,
                offset,
                hits: &mut hits,
                text_w,
            },
            index,
            row,
        );
    }

    // arcs
    let ax = area.x + text_w + 1;

    for (col, &arc_index) in order.iter().enumerate() {
        if let ControlFlow::Break(()) = draw_arc(
            buf,
            ArcLayout {
                area,
                offset,
                arcs_w,
                shown,
                ax,
                col,
            },
            arcs,
            arc_index,
        ) {
            break;
        }
    }

    hits
}

fn draw_arc(buf: &mut Buffer, context: ArcLayout, arcs: &[Arc], arc_index: usize) -> ControlFlow<()> {
    let ArcLayout {
        area,
        offset,
        arcs_w,
        shown,
        ax,
        col,
    } = context;

    let arc = &arcs[arc_index];
    let cx = ax + 2 + 3 * col as u16;

    if cx >= area.right() {
        return ControlFlow::Break(());
    }

    let style = if arc.loops {
        theme::fg(theme::pal().info())
    } else {
        theme::fg(theme::pal().attention())
    };

    let (top, bottom) = (arc.from.min(arc.to), arc.from.max(arc.to));
    let row_y = |i: usize| -> Option<u16> { (i >= offset && i < offset + shown).then(|| area.y + (i - offset) as u16) };

    let set = |buf: &mut Buffer, x: u16, y: u16, symbol: &str| {
        if x < area.right() {
            buf[(x, y)].set_symbol(symbol).set_style(style);
        }
    };

    if arc.from == arc.to {
        if let Some(y) = row_y(arc.from) {
            set(buf, ax, y, "↺");
        }
    } else {
        draw_arc_span(
            buf,
            ArcTargetLayout {
                ax,
                arc,
                cx,
                top,
                bottom,
            },
            row_y,
            set,
        );
    }

    // the label right of every arc column, on the source row
    if let Some(y) = row_y(arc.from) {
        draw_arc_label(buf, area, arc, ax, arcs_w, y, style);
    }

    ControlFlow::Continue(())
}

fn draw_arc_span(
    buf: &mut Buffer,
    context: ArcTargetLayout<'_>,
    row_y: impl Fn(usize) -> Option<u16>,
    set: impl Fn(&mut Buffer, u16, u16, &str),
) {
    let ArcTargetLayout {
        ax,
        arc,
        cx,
        top,
        bottom,
    } = context;

    for i in top..=bottom {
        draw_arc_row(
            buf,
            ArcRowLayout {
                ax,
                arc,
                cx,
                top,
                bottom,
                i,
            },
            &row_y,
            &set,
        );
    }
}

fn draw_arc_row(
    buf: &mut Buffer,
    context: ArcRowLayout<'_>,
    row_y: &impl Fn(usize) -> Option<u16>,
    set: &impl Fn(&mut Buffer, u16, u16, &str),
) {
    let ArcRowLayout {
        ax,
        arc,
        cx,
        top,
        bottom,
        i,
    } = context;

    let Some(y) = row_y(i) else {
        return;
    };

    if i == top || i == bottom {
        // the end rows: a horizontal run to the column, the corner, the arrow at the target
        let target = i == arc.to;

        set(buf, ax, y, if target { "◀" } else { "─" });

        for x in ax + 1..cx {
            let cur = buf[(x, y)].symbol().to_string();

            set(buf, x, y, crossing_symbol(&cur));
        }

        set(buf, cx, y, if i == top { "┐" } else { "┘" });
    } else {
        set(buf, cx, y, "│");
    }
}

fn draw_row(buf: &mut Buffer, context: RailRowLayout<'_>, index: usize, row: &Row) {
    let RailRowLayout {
        area,
        cursor,
        focus,
        offset,
        hits,
        text_w,
    } = context;

    let y = area.y + (index - offset) as u16;
    let row_area = Rect { y, height: 1, ..area };
    let on = cursor == Some(index);

    let base = rail_row_style(on, focus);

    clear_selected_row(buf, row_area, on, base);

    let mut x = area.x;

    let put = |buf: &mut Buffer, x: &mut u16, text: &str, style: Style, end: u16| {
        put_text(
            RailTextLayout {
                focus,
                y,
                on,
                base,
                x,
                style,
                end,
            },
            buf,
            text,
        );
    };

    let end = area.x + text_w;

    put(buf, &mut x, &format!("{:>2} ", index + 1), theme::muted(), end);
    put(buf, &mut x, &format!("{} ", row.mark.0), row.mark.1, end);
    let tags_w: u16 = row.tags.iter().map(|(tag, _)| tag.width() as u16 + 1).sum();
    let title_end = end.saturating_sub(tags_w).max(x + 4);
    let title = crate::ui::thread_card::fit(&row.title, usize::from(title_end.saturating_sub(x)));

    put(
        buf,
        &mut x,
        &title,
        if row.title_style == Style::default() {
            theme::text()
        } else {
            row.title_style
        },
        title_end,
    );
    x = x.max(title_end);

    for (tag, style) in &row.tags {
        put(buf, &mut x, " ", Style::default(), end);
        put(buf, &mut x, tag, *style, end);
    }

    hits.push((row_area, index));
}

fn put_text(context: RailTextLayout<'_>, buf: &mut Buffer, text: &str) {
    let RailTextLayout {
        focus,
        y,
        on,
        base,
        x,
        style,
        end,
    } = context;

    for ch in text.chars() {
        if let ControlFlow::Break(()) = put_char(buf, RailCellStyle { focus, on, base, style }, y, x, end, ch) {
            break;
        }
    }
}

fn put_char(
    buf: &mut Buffer,
    rail_cell_style: RailCellStyle,
    y: u16,
    x: &mut u16,
    end: u16,
    ch: char,
) -> ControlFlow<()> {
    let RailCellStyle { focus, on, base, style } = rail_cell_style;

    let char_w = ch.width().unwrap_or(0) as u16;

    if *x + char_w > end {
        return ControlFlow::Break(());
    }

    let style = if on {
        base.patch(Style {
            fg: if focus { None } else { style.fg },
            ..Style::default()
        })
    } else {
        style
    };

    buf[(*x, y)].set_symbol(&ch.to_string()).set_style(style);
    *x += char_w.max(1);

    ControlFlow::Continue(())
}

#[cfg(test)]
#[path = "flows_flow_rail_tests.rs"]
mod tests;

#[derive(Clone, Copy)]
struct ArcLayout {
    area: Rect,
    offset: usize,
    arcs_w: u16,
    shown: usize,
    ax: u16,
    col: usize,
}

#[derive(Clone, Copy)]
pub(super) struct ArcTargetLayout<'arc> {
    pub(super) ax: u16,
    pub(super) arc: &'arc Arc,
    pub(super) cx: u16,
    pub(super) top: usize,
    pub(super) bottom: usize,
}

#[derive(Clone, Copy)]
struct ArcRowLayout<'arc> {
    ax: u16,
    arc: &'arc Arc,
    cx: u16,
    top: usize,
    bottom: usize,
    i: usize,
}

pub(super) struct RailRowLayout<'hits> {
    pub(super) area: Rect,
    pub(super) cursor: Option<usize>,
    pub(super) focus: bool,
    pub(super) offset: usize,
    pub(super) hits: &'hits mut Vec<(Rect, usize)>,
    pub(super) text_w: u16,
}

struct RailTextLayout<'x> {
    focus: bool,
    y: u16,
    on: bool,
    base: Style,
    x: &'x mut u16,
    style: Style,
    end: u16,
}

fn draw_arc_label(buf: &mut Buffer, area: Rect, arc: &Arc, ax: u16, arcs_w: u16, y: u16, style: Style) {
    let lx = ax + arcs_w;
    let mut x = lx;

    for ch in crate::ui::thread_card::fit(&arc.label, usize::from(area.right().saturating_sub(lx))).chars() {
        if x >= area.right() {
            break;
        }

        buf[(x, y)].set_symbol(&ch.to_string()).set_style(style);
        x += ch.width().unwrap_or(1).max(1) as u16;
    }
}

fn crossing_symbol(symbol: &str) -> &'static str {
    if symbol == "│" { "┼" } else { "─" }
}

fn rail_row_style(on: bool, focus: bool) -> Style {
    if on {
        if focus {
            theme::pill().add_modifier(Modifier::BOLD)
        } else {
            theme::panel().add_modifier(Modifier::BOLD)
        }
    } else {
        Style::default()
    }
}

fn clear_selected_row(buf: &mut Buffer, row_area: Rect, on: bool, base: Style) {
    if on {
        for x in row_area.x..row_area.right() {
            buf[(x, row_area.y)].set_symbol(" ").set_style(base);
        }
    }
}

#[derive(Clone, Copy)]
struct RailViewport {
    area: Rect,
    cursor: Option<usize>,
    focus: bool,
    offset: usize,
    arcs_w: u16,
    text_w: u16,
}

#[derive(Clone, Copy)]
struct RailCellStyle {
    focus: bool,
    on: bool,
    base: Style,
    style: Style,
}
