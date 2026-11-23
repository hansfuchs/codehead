//! Mouse selection of drawn text in 1 area of the screen (the start screen's preview: a thread's
//! chat, a note, an agent): a drag selects from cell to cell, 2 clicks a word, 3 clicks the row.
//! The release copies the text of the drawn cells (rows trimmed at the end, joined by newlines).

use std::time::{Duration, Instant};

use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Position, Rect};

use crate::theme;

#[derive(Default)]
pub struct ScreenSel {
    /// Start, end (screen cells, in the area). Equal: 0 selection.
    sel: Option<(Position, Position)>,
    drag: bool,
    clicks: Option<(Instant, Position, u8)>,
}

impl ScreenSel {
    pub fn clear(&mut self) {
        self.sel = None;
        self.drag = false;
    }

    pub fn is_dragging(&self) -> bool {
        self.drag
    }

    /// A mouse event over AREA (BUF: the last drawn frame). Some(text): copy it (the release of a
    /// drag, 2 or 3 clicks). The caller sends only events in AREA, or any while dragging.
    pub fn mouse(&mut self, m: MouseEvent, area: Rect, buf: &Buffer) -> Option<String> {
        let at = Position { x: m.column.clamp(area.x, area.right().saturating_sub(1)), y: m.row.clamp(area.y, area.bottom().saturating_sub(1)) };
        match m.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                let n = match self.clicks {
                    Some((t, p, n)) if p == at && t.elapsed() < Duration::from_millis(400) => n % 3 + 1,
                    _ => 1,
                };
                self.clicks = Some((Instant::now(), at, n));
                match n {
                    2 => {
                        let row = row_chars(buf, area, at.y);
                        let i = usize::from(at.x - area.x);
                        if row.get(i).is_none_or(|c| c.is_whitespace()) {
                            self.sel = None;
                            return None;
                        }
                        let a = (0..i).rev().take_while(|&j| !row[j].is_whitespace()).last().unwrap_or(i);
                        let b = (i..row.len()).take_while(|&j| !row[j].is_whitespace()).last().unwrap_or(i);
                        self.sel = Some((Position { x: area.x + a as u16, y: at.y }, Position { x: area.x + b as u16, y: at.y }));
                        self.drag = false;
                        self.text(area, buf)
                    }
                    3 => {
                        self.sel = Some((Position { x: area.x, y: at.y }, Position { x: area.right().saturating_sub(1), y: at.y }));
                        self.drag = false;
                        self.text(area, buf)
                    }
                    _ => {
                        self.sel = Some((at, at));
                        self.drag = true;
                        None
                    }
                }
            }
            MouseEventKind::Drag(MouseButton::Left) if self.drag => {
                if let Some((from, _)) = self.sel {
                    self.sel = Some((from, at));
                }
                None
            }
            MouseEventKind::Up(MouseButton::Left) if self.drag => {
                self.drag = false;
                match self.sel {
                    Some((a, b)) if a != b => self.text(area, buf),
                    _ => {
                        self.sel = None;
                        None
                    }
                }
            }
            _ => None,
        }
    }

    /// Start, end in reading order.
    fn range(&self) -> Option<(Position, Position)> {
        let (a, b) = self.sel?;
        Some(if (a.y, a.x) <= (b.y, b.x) { (a, b) } else { (b, a) })
    }

    /// The cells of row Y (in AREA) the selection covers: x from, x to (inclusive).
    fn cols(&self, area: Rect, y: u16) -> Option<(u16, u16)> {
        let (a, b) = self.range()?;
        if y < a.y || y > b.y {
            return None;
        }
        let x0 = if y == a.y { a.x } else { area.x };
        let x1 = if y == b.y { b.x } else { area.right().saturating_sub(1) };
        Some((x0, x1))
    }

    /// The selected text of BUF: per row its cells, trimmed at the end; blank rows at the ends off.
    pub fn text(&self, area: Rect, buf: &Buffer) -> Option<String> {
        let (a, b) = self.range()?;
        let mut out = Vec::new();
        for y in a.y..=b.y {
            let Some((x0, x1)) = self.cols(area, y) else { continue };
            let row: String = (x0..=x1).filter_map(|x| buf.cell((x, y)).map(|c| c.symbol().to_string())).collect();
            out.push(row.trim_end().to_string());
        }
        let t = out.join("\n").trim_matches('\n').to_string();
        (!t.trim().is_empty()).then_some(t)
    }

    /// The selection in BUF: accent cells (as the chat's selection).
    pub fn paint(&self, area: Rect, buf: &mut Buffer) {
        let Some((a, b)) = self.range() else { return };
        if a == b {
            return;
        }
        let st = theme::pill();
        for y in a.y.max(area.y)..=b.y.min(area.bottom().saturating_sub(1)) {
            let Some((x0, x1)) = self.cols(area, y) else { continue };
            for x in x0..=x1.min(area.right().saturating_sub(1)) {
                if let Some(c) = buf.cell_mut((x, y)) {
                    c.set_style(st);
                }
            }
        }
    }
}

fn row_chars(buf: &Buffer, area: Rect, y: u16) -> Vec<char> {
    (area.x..area.right()).map(|x| buf.cell((x, y)).and_then(|c| c.symbol().chars().next()).unwrap_or(' ')).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::crossterm::event::KeyModifiers;

    fn ev(kind: MouseEventKind, x: u16, y: u16) -> MouseEvent {
        MouseEvent { kind, column: x, row: y, modifiers: KeyModifiers::NONE }
    }

    #[test]
    fn a_drag_copies_the_cells_and_clicks_a_word_or_the_row() {
        let area = Rect::new(0, 0, 20, 3);
        let mut buf = Buffer::empty(area);
        buf.set_string(0, 0, "hello world", ratatui::style::Style::default());
        buf.set_string(0, 1, "second row here", ratatui::style::Style::default());
        let mut s = ScreenSel::default();
        assert_eq!(s.mouse(ev(MouseEventKind::Down(MouseButton::Left), 6, 0), area, &buf), None);
        s.mouse(ev(MouseEventKind::Drag(MouseButton::Left), 5, 1), area, &buf);
        assert_eq!(s.mouse(ev(MouseEventKind::Up(MouseButton::Left), 5, 1), area, &buf).as_deref(), Some("world\nsecond"));
        // 2 clicks: the word
        s.mouse(ev(MouseEventKind::Down(MouseButton::Left), 9, 1), area, &buf);
        s.mouse(ev(MouseEventKind::Up(MouseButton::Left), 9, 1), area, &buf);
        assert_eq!(s.mouse(ev(MouseEventKind::Down(MouseButton::Left), 9, 1), area, &buf).as_deref(), Some("row"));
        // 3 clicks: the row
        assert_eq!(s.mouse(ev(MouseEventKind::Down(MouseButton::Left), 9, 1), area, &buf).as_deref(), Some("second row here"));
        // the paint: accent cells of the row
        s.paint(area, &mut buf);
        assert_eq!(buf[(0, 1)].bg, theme::pal().accent);
        assert_ne!(buf[(0, 0)].bg, theme::pal().accent);
    }
}
