use super::stream::StreamFrame;
use super::{Frame, Line, Paragraph, Position, Rect, Span, Style, View, WAVE, chat, theme};
impl View {
    pub(super) fn draw_stream_waves(&self, frame: &mut Frame, window: &StreamFrame) {
        draw_waves(self, frame, window);
    }
    pub(super) fn draw_stream_hover(&self, frame: &mut Frame, window: &StreamFrame) {
        let Some(at) = self.selection.hover.filter(|point| window.shown.contains(*point)) else {
            return;
        };

        let Some(row_index) = self.row_at(at.y) else {
            return;
        };

        let area = window.area;
        let buf = frame.buffer_mut();

        let col = at.x - area.x;
        let row = &self.stream.drawn.rows[row_index];
        let hit = self.stream.drawn.hits.contains_key(&row_index);
        let span = row
            .links
            .iter()
            .chain(row.copies.iter())
            .find(|(start, end, _)| col >= *start && col < *end)
            .map(|(start, end, _)| (*start, *end));

        if let Some((start, end)) = span {
            theme::hover(
                buf,
                Rect {
                    x: area.x + start,
                    y: at.y,
                    width: end - start,
                    height: 1,
                },
            );
        } else if hit {
            hover_message(self, buf, area, at);
        }
    }

    pub(super) fn draw_stream_fold(&self, frame: &mut Frame, area: Rect) {
        let buf = frame.buffer_mut();

        if self.stream.back == 0 || area.height <= 1 {
            return;
        }

        let y = area.bottom() - 1;

        for x in area.x..area.right() {
            buf[(x, y)].reset();
            buf[(x, y)].set_symbol("┄").set_style(theme::line());
        }
    }
    pub(super) fn draw_stream_empty(&self, frame: &mut Frame, window: &StreamFrame) {
        if window.total != 0 {
            return;
        }

        let area = window.area;

        let note = if self.agents.members.is_empty() {
            "0 agents on this thread · @name brings one in"
        } else {
            "waiting for the first event…"
        };

        frame.render_widget(
            Paragraph::new(Line::styled(format!("  {note}"), theme::muted())),
            Rect {
                y: area.bottom().saturating_sub(1),
                height: 1,
                ..area
            },
        );
    }
}

fn hover_message(view: &View, buf: &mut ratatui::buffer::Buffer, area: Rect, at: Position) {
    // bubbles: the hover stays inside the box's borders
    let (x0, x1) = if view.stream.look == chat::Look::Bubbles {
        (area.x + 2, area.right().saturating_sub(2))
    } else {
        (area.x, area.right())
    };

    theme::hover(
        buf,
        Rect {
            x: x0,
            y: at.y,
            width: x1.saturating_sub(x0),
            height: 1,
        },
    );
}

fn draw_waves(view: &View, frame: &mut Frame, window: &StreamFrame) {
    let tick = view.tick();
    let StreamFrame {
        area,
        shown,
        start,
        end,
        cut,
        ..
    } = *window;

    for (row, col, color) in &view.stream.drawn.waves {
        if *row < start + cut || *row >= end {
            continue;
        }

        let y = shown.y + (row - start - cut) as u16;

        if y < shown.bottom() && area.x + col + 3 <= area.right() {
            frame.render_widget(
                Paragraph::new(Span::styled(WAVE[tick % WAVE.len()], Style::default().fg(*color))),
                Rect {
                    x: area.x + col,
                    y,
                    width: 3,
                    height: 1,
                },
            );
        }
    }
}
