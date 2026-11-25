//! Mouse selection of drawn text in 1 area of the screen (the start screen's preview: a thread's
//! chat, a note, an agent): a drag selects from cell to cell, 2 clicks a word, 3 clicks the row.
//! The release copies the text of the drawn cells (rows trimmed at the end, joined by newlines).

use std::time::{Duration, Instant};

use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Position, Rect};

use crate::ui::theme;

#[derive(Default)]
pub(crate) struct ScreenSel {
    /// Start, end (screen cells, in the area). Equal: 0 selection.
    sel: Option<(Position, Position)>,
    drag: bool,
    clicks: Option<(Instant, Position, u8)>,
}

impl ScreenSel {
    pub(crate) fn clear(&mut self) {
        self.sel = None;
        self.drag = false;
    }

    pub(crate) fn is_dragging(&self) -> bool {
        self.drag
    }

    /// A mouse event over AREA (BUF: the last drawn frame). Some(text): copy it (the release of a
    /// drag, 2 or 3 clicks). The caller sends only events in AREA, or any while dragging.
    pub(crate) fn mouse(&mut self, event: MouseEvent, area: Rect, buf: &Buffer) -> Option<String> {
        mouse_event(event, area, buf, self)
    }

    /// Start, end in reading order.
    fn range(&self) -> Option<(Position, Position)> {
        let (agent, bytes) = self.sel?;

        Some(if (agent.y, agent.x) <= (bytes.y, bytes.x) {
            (agent, bytes)
        } else {
            (bytes, agent)
        })
    }

    /// The cells of row Y (in AREA) the selection covers: x from, x to (inclusive).
    fn cols(&self, area: Rect, y: u16) -> Option<(u16, u16)> {
        let (agent, bytes) = self.range()?;

        if y < agent.y || y > bytes.y {
            return None;
        }

        let x0 = if y == agent.y { agent.x } else { area.x };

        let x1 = if y == bytes.y {
            bytes.x
        } else {
            area.right().saturating_sub(1)
        };

        Some((x0, x1))
    }

    /// The selected text of BUF: per row its cells, trimmed at the end; blank rows at the ends off.
    pub(crate) fn text(&self, area: Rect, buf: &Buffer) -> Option<String> {
        selection_text(self, area, buf)
    }

    /// The selection in BUF: accent cells (as the chat's selection).
    pub(crate) fn paint(&self, area: Rect, buf: &mut Buffer) {
        paint_selection(self, area, buf);
    }
}

fn row_chars(buf: &Buffer, area: Rect, y: u16) -> Vec<char> {
    (area.x..area.right())
        .map(|x| {
            buf.cell((x, y))
                .and_then(|character| character.symbol().chars().next())
                .unwrap_or(' ')
        })
        .collect()
}

#[cfg(test)]
#[path = "screen_select_tests.rs"]
mod tests;

fn selection_text(widget_state: &ScreenSel, area: Rect, buf: &Buffer) -> Option<String> {
    let (agent, bytes) = widget_state.range()?;
    let mut out = Vec::new();

    for y in agent.y..=bytes.y {
        let Some((x0, x1)) = widget_state.cols(area, y) else {
            continue;
        };

        let row: String = (x0..=x1)
            .filter_map(|x| buf.cell((x, y)).map(|character| character.symbol().to_string()))
            .collect();

        out.push(row.trim_end().to_string());
    }

    let thread = out.join("\n").trim_matches('\n').to_string();

    (!thread.trim().is_empty()).then_some(thread)
}

fn paint_selection(widget_state: &ScreenSel, area: Rect, buf: &mut Buffer) {
    let Some((agent, bytes)) = widget_state.range() else {
        return;
    };

    if agent == bytes {
        return;
    }

    let st = theme::pill();

    for y in agent.y.max(area.y)..=bytes.y.min(area.bottom().saturating_sub(1)) {
        paint_character(widget_state, area, buf, st, y);
    }
}

fn paint_character(widget_state: &ScreenSel, area: Rect, buf: &mut Buffer, st: ratatui::prelude::Style, y: u16) {
    let Some((x0, x1)) = widget_state.cols(area, y) else {
        return;
    };

    for x in x0..=x1.min(area.right().saturating_sub(1)) {
        if let Some(character) = buf.cell_mut((x, y)) {
            character.set_style(st);
        }
    }
}

fn mouse_press(at: Position, area: Rect, buf: &Buffer, selection: &mut ScreenSel) -> Option<String> {
    let count_value = match selection.clicks {
        Some((thread, path, count)) if path == at && thread.elapsed() < Duration::from_millis(400) => count % 3 + 1,
        _ => 1,
    };

    selection.clicks = Some((Instant::now(), at, count_value));

    match count_value {
        2 => {
            let row = row_chars(buf, area, at.y);
            let i = usize::from(at.x - area.x);

            if row.get(i).is_none_or(|character| character.is_whitespace()) {
                selection.sel = None;
                return None;
            }

            let agent = (0..i)
                .rev()
                .take_while(|&j| !row[j].is_whitespace())
                .last()
                .unwrap_or(i);
            let bytes = (i..row.len())
                .take_while(|&j| !row[j].is_whitespace())
                .last()
                .unwrap_or(i);

            selection.sel = Some((
                Position {
                    x: area.x + agent as u16,
                    y: at.y,
                },
                Position {
                    x: area.x + bytes as u16,
                    y: at.y,
                },
            ));
            selection.drag = false;
            selection.text(area, buf)
        }
        3 => {
            selection.sel = Some((
                Position { x: area.x, y: at.y },
                Position {
                    x: area.right().saturating_sub(1),
                    y: at.y,
                },
            ));
            selection.drag = false;
            selection.text(area, buf)
        }
        _ => {
            selection.sel = Some((at, at));
            selection.drag = true;
            None
        }
    }
}

fn mouse_event(event: MouseEvent, area: Rect, buf: &Buffer, selection: &mut ScreenSel) -> Option<String> {
    let at = Position {
        x: event.column.clamp(area.x, area.right().saturating_sub(1)),
        y: event.row.clamp(area.y, area.bottom().saturating_sub(1)),
    };

    match event.kind {
        MouseEventKind::Down(MouseButton::Left) => mouse_press(at, area, buf, selection),
        MouseEventKind::Drag(MouseButton::Left) if selection.drag => {
            if let Some((from, _)) = selection.sel {
                selection.sel = Some((from, at));
            }

            None
        }
        MouseEventKind::Up(MouseButton::Left) if selection.drag => {
            selection.drag = false;

            match selection.sel {
                Some((agent_value, bytes_value)) if agent_value != bytes_value => selection.text(area, buf),
                _ => {
                    selection.sel = None;
                    None
                }
            }
        }
        _ => None,
    }
}
