//! The frame every dropdown shares (`@` agents, `>` commands, drafts): a label, max. `MAX_ROWS` rows,
//! a scroll window that follows the cursor, ▲ / ▼ on the right border while rows wait above / below,
//! and a place with room: above or below its anchor, shifted left at the right edge; a submenu right
//! or left of its parent.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::widgets::{Block, BorderType, Borders, Clear};
use unicode_width::UnicodeWidthStr;

use crate::ui::theme;

/// Live preview of a suggestion dropdown: ↑↓ put the current row into the text at once (enter /
/// tab not needed). The text typed before the 1st move stays the dropdown's query while the
/// text holds the preview; a key that changes the text ends it (the text stays).
#[derive(Clone, Debug, Default)]
pub(crate) struct Preview {
    at: Option<crate::ui::textarea::Pos>,
    typed: String,
    shown: String,
}

impl Preview {
    /// TA holds the preview: its start and the text typed before it. None: 0 preview.
    pub(crate) fn typed(&self, ta: &crate::ui::textarea::TextArea) -> Option<(crate::ui::textarea::Pos, String)> {
        let at = self.at?;

        (ta.since(at)? == self.shown).then(|| (at, self.typed.clone()))
    }

    /// TEXT into TA from AT to the cursor (in place of TYPED, the query, or the last preview).
    pub(crate) fn show(
        &mut self,
        ta: &mut crate::ui::textarea::TextArea,
        at: crate::ui::textarea::Pos,
        typed: &str,
        text: &str,
    ) {
        if self.typed(ta).is_none() {
            self.typed = typed.to_string();
        }

        ta.replace_to_cursor(at, text);
        self.at = Some(at);
        self.shown = text.to_string();
    }

    pub(crate) fn clear(&mut self) {
        self.at = None;
    }

    /// A key that goes on with the query (a filter char): the typed text back in place of the
    /// preview (the key then types after it). True: a preview was in the text.
    pub(crate) fn revert(&mut self, ta: &mut crate::ui::textarea::TextArea) -> bool {
        let Some((at, typed)) = self.typed(ta) else {
            return false;
        };

        ta.replace_to_cursor(at, &typed);
        self.at = None;
        true
    }
}

/// Most rows a dropdown shows at a time.
pub(crate) const MAX_ROWS: usize = 10;

/// First row in view, so that CURSOR (of LEN rows) is 1 of the SHOWN rows. OFFSET: the last value.
pub(crate) fn fit(offset: usize, cursor: usize, len: usize, shown: usize) -> usize {
    if shown == 0 || len <= shown {
        return 0;
    }

    let off = offset.min(len - shown);

    if cursor < off {
        cursor
    } else if cursor >= off + shown {
        cursor + 1 - shown
    } else {
        off
    }
}

/// Area of a dropdown of ROWS rows (max. `MAX_ROWS` in view) and width W at ANCHOR, inside BOUNDS.
/// ABOVE: the side tried first; the other side when the 1st has less room than the dropdown needs
/// (and the other has more). At the right edge: shifted left. None: 0 room for 1 row.
pub(crate) fn place(anchor: Rect, width: u16, rows: usize, bounds: Rect, above: bool) -> Option<Rect> {
    place_max(anchor, width, rows, MAX_ROWS, bounds, above)
}

/// [`place`] with max. MAX rows in view.
pub(crate) fn place_max(
    anchor: Rect,
    max_width: u16,
    rows: usize,
    max: usize,
    bounds: Rect,
    above: bool,
) -> Option<Rect> {
    let want = rows.clamp(1, max) as u16 + 2;
    let up = anchor.y.saturating_sub(bounds.y);
    let down = bounds.bottom().saturating_sub(anchor.bottom());
    let (first, second) = if above { (up, down) } else { (down, up) };
    let use_first = first >= want || first >= second;
    let room = if use_first { first } else { second };
    let height = want.min(room);

    if height < 3 {
        return None;
    }

    let y = if use_first == above {
        anchor.y - height
    } else {
        anchor.bottom()
    };

    let width = max_width.min(bounds.width);
    let x = anchor.x.min(bounds.right().saturating_sub(width)).max(bounds.x);

    Some(Rect { x, y, width, height })
}

/// Area of a submenu of ROWS rows and width W for the row at `ROW_Y` of PARENT: right of the parent,
/// else left of it (its 1st row lines up with `ROW_Y`, moved up when the bottom has 0 room). 0 room on
/// both sides: above STACK (the area of the open dropdowns), else below it, cut to the room there;
/// only with 0 room at all: over the parent.
pub(crate) fn place_beside(
    parent: Rect,
    row_y: u16,
    width: u16,
    rows: usize,
    bounds: Rect,
    stack: Rect,
) -> Option<Rect> {
    place_beside_max(parent, row_y, width, rows, MAX_ROWS, bounds, stack)
}

/// [`place_beside`] with max. MAX rows in view (a preview that shows a whole flow).
pub(crate) fn place_beside_max(
    parent: Rect,
    row_y: u16,
    max_width: u16,
    rows: usize,
    max: usize,
    bounds: Rect,
    stack: Rect,
) -> Option<Rect> {
    let height = (rows.clamp(1, max) as u16 + 2).min(bounds.height);

    if height < 3 {
        return None;
    }

    let width = max_width.min(bounds.width);

    let beside = beside_column(parent, stack, bounds, width);

    if let Some(x) = beside {
        let y = row_y.saturating_sub(1).min(bounds.bottom() - height).max(bounds.y);

        return Some(Rect { x, y, width, height });
    }

    let x = parent.x.min(bounds.right() - width).max(bounds.x);
    let up = stack.y.saturating_sub(bounds.y);
    let down = bounds.bottom().saturating_sub(stack.bottom());

    if up >= height || (up >= 3 && up >= down) {
        let up_height = height.min(up);

        return Some(Rect {
            x,
            y: stack.y - up_height,
            width,
            height: up_height,
        });
    }

    if down >= 3 {
        return Some(Rect {
            x,
            y: stack.bottom(),
            width,
            height: height.min(down),
        });
    }

    let y = row_y.saturating_sub(1).min(bounds.bottom() - height).max(bounds.y);

    Some(Rect {
        x: bounds.right() - width,
        y,
        width,
        height,
    })
}

/// Frame of a dropdown in R: cleared, plain border, LABEL on top, NOTE bottom right. UP / DOWN:
/// rows wait above / below → ▲ / ▼ on the right border. ACTIVE: the dropdown with the keys (accent
/// border; a parent of an open submenu: gray). Returns the area of the rows.
pub(crate) fn frame(frame: &mut Frame, area: Rect, label: &str, note: &str, state: FrameState) -> Rect {
    let FrameState { up, down, active } = state;

    frame.render_widget(Clear, area);
    let border = if active { theme::accent() } else { theme::line() };
    let mut block = Block::new()
        .borders(Borders::ALL)
        .border_type(BorderType::Plain)
        .border_style(border)
        .title(Span::styled(
            format!(" {label} "),
            if active { theme::accent_bold() } else { theme::muted() },
        ))
        .style(Style::default().bg(theme::pal().on_accent).fg(theme::pal().text));

    if !note.is_empty() {
        block = block.title_bottom(Line::styled(format!(" {note} "), theme::muted()).right_aligned());
    }

    let inner = block.inner(area);

    frame.render_widget(block, area);

    if inner.height > 0 && area.width > 0 {
        frame_buf(frame, area, up, down, inner);
    }

    inner
}

fn frame_buf(frame: &mut Frame<'_>, area: Rect, up: bool, down: bool, inner: Rect) {
    let x = area.right() - 1;
    let st = theme::accent_bold().bg(theme::pal().on_accent);
    let buf = frame.buffer_mut();

    if up && down && inner.height == 1 {
        buf[(x, inner.y)].set_symbol("↕").set_style(st);
    } else {
        if up {
            buf[(x, inner.y)].set_symbol("▲").set_style(st);
        }

        if down {
            buf[(x, inner.bottom() - 1)].set_symbol("▼").set_style(st);
        }
    }
}

/// A row of [`list`]: its text, a detail at the right (muted), NOW: the current choice (`●`),
/// MARKS: chars of the text a filter matched (accent).
#[derive(Clone, Default)]
pub(crate) struct Row {
    pub text: String,
    pub detail: String,
    pub now: bool,
    pub marks: Vec<usize>,
}

/// A list dropdown in R (from [`place`]): its frame with LABEL / NOTE, ROWS from OFFSET so that
/// CUR is in view (a detail gets max. half a row; a long one keeps its end), EMPTY on 0 rows.
/// Returns the new offset and the drawn rows (area, index in ROWS) for clicks.
#[derive(Clone, Copy)]
pub(crate) struct List<'content> {
    pub label: &'content str,
    pub note: &'content str,
    pub rows: &'content [Row],
    pub cursor: usize,
    pub offset: usize,
    pub empty: &'content str,
}
pub(crate) fn list(frame: &mut Frame, area: Rect, content: List<'_>) -> (usize, Vec<(Rect, usize)>) {
    list_in(frame, area, content, true)
}

/// [`list`]; ACTIVE: the dropdown with the keys (a parent of an open submenu: false, gray).
pub(crate) fn list_in(frame: &mut Frame, area: Rect, content: List<'_>, active: bool) -> (usize, Vec<(Rect, usize)>) {
    let List {
        label,
        note,
        rows,
        cursor: cur,
        offset,
        empty,
    } = content;
    let count = rows.len();
    let shown = (area.height as usize).saturating_sub(2);
    let off = fit(offset, cur, count, shown);
    let inner = self::frame(
        frame,
        area,
        label,
        note,
        FrameState {
            up: off > 0,
            down: off + shown < count,
            active,
        },
    );

    if count == 0 {
        frame.render_widget(
            Paragraph::new(Line::styled(format!(" {empty}"), theme::muted())),
            Rect {
                height: inner.height.min(1),
                ..inner
            },
        );
    }

    let out = Vec::new();

    draw_list_rows(frame, rows, cur, shown, off, inner, out)
}

fn draw_list_rows(
    frame: &mut Frame<'_>,
    rows: &[Row],
    cur: usize,
    shown: usize,
    off: usize,
    inner: Rect,
    mut out: Vec<(Rect, usize)>,
) -> (usize, Vec<(Rect, usize)>) {
    for (j, it) in rows.iter().enumerate().skip(off).take(shown) {
        draw_list_row(frame, cur, off, inner, &mut out, j, it);
    }

    (off, out)
}

fn draw_list_row(
    frame: &mut Frame<'_>,
    cur: usize,
    off: usize,
    inner: Rect,
    out: &mut Vec<(Rect, usize)>,
    j: usize,
    it: &Row,
) {
    let row = Rect {
        y: inner.y + (j - off) as u16,
        height: 1,
        ..inner
    };
    let hl = j == cur;

    let st = if hl {
        theme::pill().add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme::pal().text)
    };

    let dim = if hl { theme::pill() } else { theme::muted() };
    let mark = if it.now { "● " } else { "  " };

    // the detail: max. half the row, a long path keeps its end (`worktree …/end`)
    let half = (row.width as usize / 2).max(20);

    let detail: String = if it.detail.width() > half {
        let (head, path) = it
            .detail
            .split_once(' ')
            .filter(|(word, _)| *word == "worktree")
            .map_or(("", it.detail.as_str()), |(word, path)| (word, path));

        let keep = half.saturating_sub(head.width() + 2);
        let tail: Vec<char> = path.chars().rev().take(keep).collect();
        let tail: String = tail.into_iter().rev().collect();

        if head.is_empty() {
            format!("…{tail}")
        } else {
            format!("{head} …{tail}")
        }
    } else {
        it.detail.clone()
    };

    draw_list_text(frame, out, j, it, &(detail), RowStyles { row, hl, st, dim, mark });
}

fn draw_list_text(
    frame: &mut Frame<'_>,
    out: &mut Vec<(Rect, usize)>,
    j: usize,
    it: &Row,
    detail: &String,
    settings: RowStyles<'_>,
) {
    let RowStyles { row, hl, st, dim, mark } = settings;

    let room = (row.width as usize).saturating_sub(detail.width() + 6);

    let text: String = if it.text.width() > room {
        it.text.chars().take(room.saturating_sub(1)).chain(['…']).collect()
    } else {
        it.text.clone()
    };

    let pad = room.saturating_sub(text.width());

    let hit = if hl {
        st.add_modifier(Modifier::UNDERLINED)
    } else {
        st.fg(theme::pal().accent)
    };

    let mut spans = vec![Span::styled(format!(" {mark}"), st)];

    spans.extend(crate::ui::fuzzy::spans(&text, &it.marks, st, hit));
    spans.extend([
        Span::styled(" ".repeat(pad + 1), st),
        Span::styled(format!("{detail} "), dim),
    ]);
    let line = Line::from(spans);

    frame.render_widget(Paragraph::new(line).style(st), row);
    out.push((row, j));
}

#[cfg(test)]
#[path = "dropdown_tests.rs"]
mod tests;

#[derive(Clone, Copy)]
pub(crate) struct FrameState {
    pub up: bool,
    pub down: bool,
    pub active: bool,
}

fn beside_column(parent: Rect, stack: Rect, bounds: Rect, width: u16) -> Option<u16> {
    if parent.right() + width <= bounds.right() {
        Some(parent.right())
    } else if stack.x >= bounds.x + width {
        // left of every open dropdown (left of the parent only: over the parent's parent)
        Some(stack.x - width)
    } else {
        None
    }
}

#[derive(Clone, Copy)]
struct RowStyles<'context> {
    row: Rect,
    hl: bool,
    st: Style,
    dim: Style,
    mark: &'context str,
}
