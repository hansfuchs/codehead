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
    let mut value = Vec::new();

    for (i, count) in nodes.iter().enumerate() {
        arcs_result(&rounds, &mut value, i, count);
    }

    value
}

fn arcs_result(rounds: &impl Fn(&str) -> u32, value: &mut Vec<Arc>, i: usize, count: &Resolved) {
    if let Some((to, until, max)) = &count.loops {
        let result = rounds(&count.id);

        let round = if result > 0 {
            format!(" · round {}/{max}", result + 1)
        } else {
            format!(" · max {max}")
        };

        value.push(Arc {
            from: i,
            to: *to,
            label: format!("↺ {}{round}", if until.is_empty() { "loop" } else { until }),
            loops: true,
        });
    }

    for (key, to) in &count.edges {
        value.push(Arc {
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

    order.sort_by_key(|&key| (arcs[key].from.abs_diff(arcs[key].to), arcs[key].from));
    let aw = arcs_width(arcs).min(area.width / 2);

    // the titles' column: as wide as the widest row (the arcs right next to it), the labels
    // keep 12+ columns
    let natural = rows
        .iter()
        .map(|result| {
            3 + result.mark.0.width()
                + 1
                + result.title.width()
                + result.tags.iter().map(|(thread, _)| thread.width() + 1).sum::<usize>()
                + 1
        })
        .max()
        .unwrap_or(10) as u16;

    let room = if arcs.is_empty() {
        area.width
    } else {
        area.width.saturating_sub(aw + 13)
    };

    let text_w = natural.min(room).max(10).min(area.width);
    let shown = usize::from(area.height);

    finish_draw(
        buf,
        RailViewport {
            area,
            cursor,
            focus,
            offset,
            aw,
            text_w,
        },
        rows,
        arcs,
        hits,
        &order,
        shown,
    )
}

fn finish_draw(
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
        aw,
        text_w,
    } = rail_viewport;

    for (key_value, row) in rows.iter().enumerate().skip(offset).take(shown) {
        draw_area_value(
            buf,
            RailRowLayout {
                area,
                cursor,
                focus,
                offset,
                hits: &mut hits,
                text_w,
            },
            key_value,
            row,
        );
    }

    // arcs
    let ax = area.x + text_w + 1;

    for (col, &key_2) in order.iter().enumerate() {
        if let ControlFlow::Break(()) = draw_agent(
            buf,
            ArcLayout {
                area,
                offset,
                aw,
                shown,
                ax,
                col,
            },
            arcs,
            key_2,
        ) {
            break;
        }
    }

    hits
}

fn draw_agent(buf: &mut Buffer, context: ArcLayout, arcs: &[Arc], key_2: usize) -> ControlFlow<()> {
    let ArcLayout {
        area,
        offset,
        aw,
        shown,
        ax,
        col,
    } = context;

    let agent = &arcs[key_2];
    let cx = ax + 2 + 3 * col as u16;

    if cx >= area.right() {
        return ControlFlow::Break(());
    }

    let st = if agent.loops {
        theme::fg(theme::pal().info())
    } else {
        theme::fg(theme::pal().attention())
    };

    let (top, bottom) = (agent.from.min(agent.to), agent.from.max(agent.to));
    let row_y = |i: usize| -> Option<u16> { (i >= offset && i < offset + shown).then(|| area.y + (i - offset) as u16) };

    let set = |buf: &mut Buffer, x: u16, y: u16, text_2: &str| {
        if x < area.right() {
            buf[(x, y)].set_symbol(text_2).set_style(st);
        }
    };

    if agent.from == agent.to {
        if let Some(y) = row_y(agent.from) {
            set(buf, ax, y, "↺");
        }
    } else {
        draw_agent_target(
            buf,
            ArcTargetLayout {
                ax,
                agent,
                cx,
                top,
                bottom,
            },
            row_y,
            set,
        );
    }

    // the label right of every arc column, on the source row
    if let Some(y) = row_y(agent.from) {
        draw_arc_label(buf, area, agent, ax, aw, y, st);
    }

    ControlFlow::Continue(())
}

fn draw_agent_target(
    buf: &mut Buffer,
    context: ArcTargetLayout<'_>,
    row_y: impl Fn(usize) -> Option<u16>,
    set: impl Fn(&mut Buffer, u16, u16, &str),
) {
    let ArcTargetLayout {
        ax,
        agent,
        cx,
        top,
        bottom,
    } = context;

    for i in top..=bottom {
        draw_agent_target_target(
            buf,
            ArcRowLayout {
                ax,
                agent,
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

fn draw_agent_target_target(
    buf: &mut Buffer,
    context: ArcRowLayout<'_>,
    row_y: &impl Fn(usize) -> Option<u16>,
    set: &impl Fn(&mut Buffer, u16, u16, &str),
) {
    let ArcRowLayout {
        ax,
        agent,
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
        let target = i == agent.to;

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

fn draw_area_value(buf: &mut Buffer, context: RailRowLayout<'_>, key_value: usize, row: &Row) {
    let RailRowLayout {
        area,
        cursor,
        focus,
        offset,
        hits,
        text_w,
    } = context;

    let y = area.y + (key_value - offset) as u16;
    let area_value = Rect { y, height: 1, ..area };
    let on = cursor == Some(key_value);

    let base = rail_row_style(on, focus);

    clear_selected_row(buf, area_value, on, base);

    let mut x = area.x;

    let put = |buf: &mut Buffer, x: &mut u16, text_value: &str, st: Style, end: u16| {
        draw_area_value_width_value(
            RailTextLayout {
                focus,
                y,
                on,
                base,
                x,
                st,
                end,
            },
            buf,
            text_value,
        );
    };

    let end = area.x + text_w;

    put(buf, &mut x, &format!("{:>2} ", key_value + 1), theme::muted(), end);
    put(buf, &mut x, &format!("{} ", row.mark.0), row.mark.1, end);
    let tags_w: u16 = row
        .tags
        .iter()
        .map(|(thread_value, _)| thread_value.width() as u16 + 1)
        .sum();
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

    for (thread_2, st) in &row.tags {
        put(buf, &mut x, " ", Style::default(), end);
        put(buf, &mut x, thread_2, *st, end);
    }

    hits.push((area_value, key_value));
}

fn draw_area_value_width_value(context: RailTextLayout<'_>, buf: &mut Buffer, text_value: &str) {
    let RailTextLayout {
        focus,
        y,
        on,
        base,
        x,
        st,
        end,
    } = context;

    for ch in text_value.chars() {
        if let ControlFlow::Break(()) =
            draw_area_value_width_value_width_value(buf, RailCellStyle { focus, on, base, st }, y, x, end, ch)
        {
            break;
        }
    }
}

fn draw_area_value_width_value_width_value(
    buf: &mut Buffer,
    rail_cell_style: RailCellStyle,
    y: u16,
    x: &mut u16,
    end: u16,
    ch: char,
) -> ControlFlow<()> {
    let RailCellStyle { focus, on, base, st } = rail_cell_style;

    let width_value = ch.width().unwrap_or(0) as u16;

    if *x + width_value > end {
        return ControlFlow::Break(());
    }

    let st = if on {
        base.patch(Style {
            fg: if focus { None } else { st.fg },
            ..Style::default()
        })
    } else {
        st
    };

    buf[(*x, y)].set_symbol(&ch.to_string()).set_style(st);
    *x += width_value.max(1);

    ControlFlow::Continue(())
}

#[cfg(test)]
#[path = "flows_flow_rail_tests.rs"]
mod tests;

#[derive(Clone, Copy)]
struct ArcLayout {
    area: Rect,
    offset: usize,
    aw: u16,
    shown: usize,
    ax: u16,
    col: usize,
}

#[derive(Clone, Copy)]
pub(super) struct ArcTargetLayout<'reference_0> {
    pub(super) ax: u16,
    pub(super) agent: &'reference_0 Arc,
    pub(super) cx: u16,
    pub(super) top: usize,
    pub(super) bottom: usize,
}

#[derive(Clone, Copy)]
struct ArcRowLayout<'reference_0> {
    ax: u16,
    agent: &'reference_0 Arc,
    cx: u16,
    top: usize,
    bottom: usize,
    i: usize,
}

pub(super) struct RailRowLayout<'reference_0> {
    pub(super) area: Rect,
    pub(super) cursor: Option<usize>,
    pub(super) focus: bool,
    pub(super) offset: usize,
    pub(super) hits: &'reference_0 mut Vec<(Rect, usize)>,
    pub(super) text_w: u16,
}

struct RailTextLayout<'reference_0> {
    focus: bool,
    y: u16,
    on: bool,
    base: Style,
    x: &'reference_0 mut u16,
    st: Style,
    end: u16,
}

fn draw_arc_label(buf: &mut Buffer, area: Rect, agent: &Arc, ax: u16, aw: u16, y: u16, st: Style) {
    let lx = ax + aw;
    let mut x = lx;

    for ch in crate::ui::thread_card::fit(&agent.label, usize::from(area.right().saturating_sub(lx))).chars() {
        if x >= area.right() {
            break;
        }

        buf[(x, y)].set_symbol(&ch.to_string()).set_style(st);
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

fn clear_selected_row(buf: &mut Buffer, area_value: Rect, on: bool, base: Style) {
    if on {
        for x in area_value.x..area_value.right() {
            buf[(x, area_value.y)].set_symbol(" ").set_style(base);
        }
    }
}

#[derive(Clone, Copy)]
struct RailViewport {
    area: Rect,
    cursor: Option<usize>,
    focus: bool,
    offset: usize,
    aw: u16,
    text_w: u16,
}

#[derive(Clone, Copy)]
struct RailCellStyle {
    focus: bool,
    on: bool,
    base: Style,
    st: Style,
}
