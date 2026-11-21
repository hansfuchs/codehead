//! The frame every dropdown shares (`@` agents, `>` commands, drafts): a label, max. MAX_ROWS rows,
//! a scroll window that follows the cursor, ▲ / ▼ on the right border while rows wait above / below,
//! and a place with room: above or below its anchor, shifted left at the right edge; a submenu right
//! or left of its parent.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear};

use crate::theme;

/// Live preview of a suggestion dropdown: ↑↓ put the current row into the text at once (enter /
/// tab not needed). The text typed before the 1st move stays the dropdown's query while the
/// text holds the preview; a key that changes the text ends it (the text stays).
#[derive(Clone, Debug, Default)]
pub struct Preview {
    at: Option<crate::textarea::Pos>,
    typed: String,
    shown: String,
}

impl Preview {
    /// TA holds the preview: its start and the text typed before it. None: 0 preview.
    pub fn typed(&self, ta: &crate::textarea::TextArea) -> Option<(crate::textarea::Pos, String)> {
        let at = self.at?;
        (ta.since(at)? == self.shown).then(|| (at, self.typed.clone()))
    }

    /// TEXT into TA from AT to the cursor (in place of TYPED, the query, or the last preview).
    pub fn show(&mut self, ta: &mut crate::textarea::TextArea, at: crate::textarea::Pos, typed: &str, text: &str) {
        if self.typed(ta).is_none() {
            self.typed = typed.to_string();
        }
        ta.replace_to_cursor(at, text);
        self.at = Some(at);
        self.shown = text.to_string();
    }

    pub fn clear(&mut self) {
        self.at = None;
    }

    /// A key that goes on with the query (a filter char): the typed text back in place of the
    /// preview (the key then types after it). True: a preview was in the text.
    pub fn revert(&mut self, ta: &mut crate::textarea::TextArea) -> bool {
        let Some((at, typed)) = self.typed(ta) else { return false };
        ta.replace_to_cursor(at, &typed);
        self.at = None;
        true
    }
}

/// Most rows a dropdown shows at a time.
pub const MAX_ROWS: usize = 10;

/// First row in view, so that CURSOR (of LEN rows) is 1 of the SHOWN rows. OFFSET: the last value.
pub fn fit(offset: usize, cursor: usize, len: usize, shown: usize) -> usize {
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

/// Area of a dropdown of ROWS rows (max. MAX_ROWS in view) and width W at ANCHOR, inside BOUNDS.
/// ABOVE: the side tried first; the other side when the 1st has less room than the dropdown needs
/// (and the other has more). At the right edge: shifted left. None: 0 room for 1 row.
pub fn place(anchor: Rect, w: u16, rows: usize, bounds: Rect, above: bool) -> Option<Rect> {
    place_max(anchor, w, rows, MAX_ROWS, bounds, above)
}

/// [`place`] with max. MAX rows in view.
pub fn place_max(anchor: Rect, w: u16, rows: usize, max: usize, bounds: Rect, above: bool) -> Option<Rect> {
    let want = rows.clamp(1, max) as u16 + 2;
    let up = anchor.y.saturating_sub(bounds.y);
    let down = bounds.bottom().saturating_sub(anchor.bottom());
    let (first, second) = if above { (up, down) } else { (down, up) };
    let use_first = first >= want || first >= second;
    let room = if use_first { first } else { second };
    let h = want.min(room);
    if h < 3 {
        return None;
    }
    let y = if use_first == above { anchor.y - h } else { anchor.bottom() };
    let w = w.min(bounds.width);
    let x = anchor.x.min(bounds.right().saturating_sub(w)).max(bounds.x);
    Some(Rect { x, y, width: w, height: h })
}

/// Area of a submenu of ROWS rows and width W for the row at ROW_Y of PARENT: right of the parent,
/// else left of it (its 1st row lines up with ROW_Y, moved up when the bottom has 0 room). 0 room on
/// both sides: above STACK (the area of the open dropdowns), else below it, cut to the room there;
/// only with 0 room at all: over the parent.
pub fn place_beside(parent: Rect, row_y: u16, w: u16, rows: usize, bounds: Rect, stack: Rect) -> Option<Rect> {
    let h = (rows.clamp(1, MAX_ROWS) as u16 + 2).min(bounds.height);
    if h < 3 {
        return None;
    }
    let w = w.min(bounds.width);
    let beside = if parent.right() + w <= bounds.right() {
        Some(parent.right())
    } else if stack.x >= bounds.x + w {
        // left of every open dropdown (left of the parent only: over the parent's parent)
        Some(stack.x - w)
    } else {
        None
    };
    if let Some(x) = beside {
        let y = row_y.saturating_sub(1).min(bounds.bottom() - h).max(bounds.y);
        return Some(Rect { x, y, width: w, height: h });
    }
    let x = parent.x.min(bounds.right() - w).max(bounds.x);
    let up = stack.y.saturating_sub(bounds.y);
    let down = bounds.bottom().saturating_sub(stack.bottom());
    if up >= h || (up >= 3 && up >= down) {
        let h = h.min(up);
        return Some(Rect { x, y: stack.y - h, width: w, height: h });
    }
    if down >= 3 {
        return Some(Rect { x, y: stack.bottom(), width: w, height: h.min(down) });
    }
    let y = row_y.saturating_sub(1).min(bounds.bottom() - h).max(bounds.y);
    Some(Rect { x: bounds.right() - w, y, width: w, height: h })
}

/// Frame of a dropdown in R: cleared, plain border, LABEL on top, NOTE bottom right. UP / DOWN:
/// rows wait above / below → ▲ / ▼ on the right border. ACTIVE: the dropdown with the keys (accent
/// border; a parent of an open submenu: gray). Returns the area of the rows.
pub fn frame(f: &mut Frame, r: Rect, label: &str, note: &str, up: bool, down: bool, active: bool) -> Rect {
    f.render_widget(Clear, r);
    let border = if active { theme::accent() } else { theme::line() };
    let mut block = Block::new()
        .borders(Borders::ALL)
        .border_type(BorderType::Plain)
        .border_style(border)
        .title(Span::styled(format!(" {label} "), if active { theme::accent_bold() } else { theme::muted() }))
        .style(Style::default().bg(theme::pal().on_accent).fg(theme::pal().text));
    if !note.is_empty() {
        block = block.title_bottom(Line::styled(format!(" {note} "), theme::muted()).right_aligned());
    }
    let inner = block.inner(r);
    f.render_widget(block, r);
    if inner.height > 0 && r.width > 0 {
        let x = r.right() - 1;
        let st = theme::accent_bold().bg(theme::pal().on_accent);
        let buf = f.buffer_mut();
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
    inner
}

/// A row of [`list`]: its text, a detail at the right (muted), NOW: the current choice (`●`),
/// MARKS: chars of the text a filter matched (accent).
#[derive(Clone, Default)]
pub struct Row {
    pub text: String,
    pub detail: String,
    pub now: bool,
    pub marks: Vec<usize>,
}

/// A list dropdown in R (from [`place`]): its frame with LABEL / NOTE, ROWS from OFFSET so that
/// CUR is in view (a detail gets max. half a row; a long one keeps its end), EMPTY on 0 rows.
/// Returns the new offset and the drawn rows (area, index in ROWS) for clicks.
#[allow(clippy::too_many_arguments)]
pub fn list(f: &mut Frame, r: Rect, label: &str, note: &str, rows: &[Row], cur: usize, offset: usize, empty: &str) -> (usize, Vec<(Rect, usize)>) {
    use ratatui::style::Modifier;
    use ratatui::widgets::Paragraph;
    use unicode_width::UnicodeWidthStr;
    let n = rows.len();
    let shown = (r.height as usize).saturating_sub(2);
    let off = fit(offset, cur, n, shown);
    let inner = frame(f, r, label, note, off > 0, off + shown < n, true);
    if n == 0 {
        f.render_widget(Paragraph::new(Line::styled(format!(" {empty}"), theme::muted())), Rect { height: inner.height.min(1), ..inner });
    }
    let mut out = Vec::new();
    for (j, it) in rows.iter().enumerate().skip(off).take(shown) {
        let row = Rect { y: inner.y + (j - off) as u16, height: 1, ..inner };
        let hl = j == cur;
        let st = if hl { theme::pill().add_modifier(Modifier::BOLD) } else { Style::default().fg(theme::pal().text) };
        let dim = if hl { theme::pill() } else { theme::muted() };
        let mark = if it.now { "● " } else { "  " };
        // the detail: max. half the row, a long path keeps its end (`worktree …/end`)
        let half = (row.width as usize / 2).max(20);
        let detail: String = if it.detail.width() > half {
            let (head, path) = it.detail.split_once(' ').filter(|(h, _)| *h == "worktree").map_or(("", it.detail.as_str()), |(h, p)| (h, p));
            let keep = half.saturating_sub(head.width() + 2);
            let tail: Vec<char> = path.chars().rev().take(keep).collect();
            let tail: String = tail.into_iter().rev().collect();
            if head.is_empty() { format!("…{tail}") } else { format!("{head} …{tail}") }
        } else {
            it.detail.clone()
        };
        let room = (row.width as usize).saturating_sub(detail.width() + 6);
        let text: String = if it.text.width() > room { it.text.chars().take(room.saturating_sub(1)).chain(['…']).collect() } else { it.text.clone() };
        let pad = room.saturating_sub(text.width());
        let hit = if hl { st.add_modifier(Modifier::UNDERLINED) } else { st.fg(theme::pal().accent) };
        let mut spans = vec![Span::styled(format!(" {mark}"), st)];
        spans.extend(crate::fuzzy::spans(&text, &it.marks, st, hit));
        spans.extend([Span::styled(" ".repeat(pad + 1), st), Span::styled(format!("{detail} "), dim)]);
        let line = Line::from(spans);
        f.render_widget(Paragraph::new(line).style(st), row);
        out.push((row, j));
    }
    (off, out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_follows_the_cursor() {
        assert_eq!(fit(0, 0, 5, 10), 0);
        assert_eq!(fit(0, 12, 30, 10), 3);
        assert_eq!(fit(3, 12, 30, 10), 3);
        assert_eq!(fit(3, 2, 30, 10), 2);
        assert_eq!(fit(25, 29, 30, 10), 20);
        assert_eq!(fit(0, 29, 30, 10), 20);
    }

    #[test]
    fn placed_where_room_is() {
        let b = Rect::new(0, 0, 80, 30);
        let a = |x, y| Rect::new(x, y, 1, 1);
        // room above: above, bottom on the row over the anchor
        assert_eq!(place(a(5, 20), 20, 4, b, true), Some(Rect::new(5, 14, 20, 6)));
        // 0 room above: below
        assert_eq!(place(a(5, 1), 20, 4, b, true), Some(Rect::new(5, 2, 20, 6)));
        // 30 rows: 10 in view
        assert_eq!(place(a(5, 20), 20, 30, b, true).map(|r| r.height), Some(12));
        // less room than rows on both sides: the bigger side, cut
        assert_eq!(place(a(5, 8), 20, 30, Rect::new(0, 0, 80, 14), true), Some(Rect::new(5, 0, 20, 8)));
        // right edge: shifted left
        assert_eq!(place(a(75, 20), 20, 2, b, true).map(|r| r.x), Some(60));
        // below first
        assert_eq!(place(Rect::new(0, 2, 20, 1), 30, 3, b, false), Some(Rect::new(0, 3, 30, 5)));
        assert_eq!(place(a(0, 1), 10, 3, Rect::new(0, 0, 80, 3), true), None);
    }

    #[test]
    fn submenu_right_then_left_then_above() {
        let b = Rect::new(0, 0, 80, 30);
        let parent = Rect::new(10, 10, 20, 6);
        assert_eq!(place_beside(parent, 12, 30, 4, b, parent), Some(Rect::new(30, 11, 30, 6)));
        // 0 room right: left
        let parent = Rect::new(50, 10, 20, 6);
        assert_eq!(place_beside(parent, 12, 30, 4, b, parent), Some(Rect::new(20, 11, 30, 6)));
        // 0 room on both sides: above the open dropdowns, at the parent's column (shifted to fit)
        let parent = Rect::new(40, 10, 30, 6);
        let stack = Rect::new(5, 8, 65, 8);
        assert_eq!(place_beside(parent, 12, 50, 4, b, stack), Some(Rect::new(30, 2, 50, 6)));
        // less room above than rows: cut to the room
        assert_eq!(place_beside(parent, 12, 50, 10, Rect::new(0, 3, 80, 13), stack), Some(Rect::new(30, 3, 50, 5)));
        // 0 room above: below them
        assert_eq!(place_beside(parent, 12, 50, 4, Rect::new(0, 7, 80, 23), stack), Some(Rect::new(30, 16, 50, 6)));
        // bottom edge: moved up
        let p = Rect::new(0, 20, 10, 8);
        assert_eq!(place_beside(p, 27, 20, 10, b, p).map(|r| (r.y, r.height)), Some((18, 12)));
    }
}
