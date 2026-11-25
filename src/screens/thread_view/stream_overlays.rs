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

        let st = window.area;
        let buf = frame.buffer_mut();

        let col = at.x - st.x;
        let row = &self.stream.drawn.rows[row_index];
        let hit = self.stream.drawn.hits.contains_key(&row_index);
        let span = row
            .links
            .iter()
            .chain(row.copies.iter())
            .find(|(agent, bytes, _)| col >= *agent && col < *bytes)
            .map(|(agent, bytes, _)| (*agent, *bytes));

        if let Some((agent, bytes)) = span {
            theme::hover(
                buf,
                Rect {
                    x: st.x + agent,
                    y: at.y,
                    width: bytes - agent,
                    height: 1,
                },
            );
        } else if hit {
            hover_message(self, buf, st, at);
        }
    }

    pub(super) fn draw_stream_fold(&self, frame: &mut Frame, st: Rect) {
        let buf = frame.buffer_mut();

        if self.stream.back == 0 || st.height <= 1 {
            return;
        }

        let y = st.bottom() - 1;

        for x in st.x..st.right() {
            buf[(x, y)].reset();
            buf[(x, y)].set_symbol("┄").set_style(theme::line());
        }
    }
    pub(super) fn draw_stream_empty(&self, frame: &mut Frame, window: &StreamFrame) {
        if window.total != 0 {
            return;
        }

        let st = window.area;

        let note = if self.agents.members.is_empty() {
            "0 agents on this thread · @name brings one in"
        } else {
            "waiting for the first event…"
        };

        frame.render_widget(
            Paragraph::new(Line::styled(format!("  {note}"), theme::muted())),
            Rect {
                y: st.bottom().saturating_sub(1),
                height: 1,
                ..st
            },
        );
    }
}

fn hover_message(view: &View, buf: &mut ratatui::buffer::Buffer, st: Rect, at: Position) {
    // bubbles: the hover stays inside the box's borders
    let (x0, x1) = if view.stream.look == chat::Look::Bubbles {
        (st.x + 2, st.right().saturating_sub(2))
    } else {
        (st.x, st.right())
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
        area: st,
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

        if y < shown.bottom() && st.x + col + 3 <= st.right() {
            frame.render_widget(
                Paragraph::new(Span::styled(WAVE[tick % WAVE.len()], Style::default().fg(*color))),
                Rect {
                    x: st.x + col,
                    y,
                    width: 3,
                    height: 1,
                },
            );
        }
    }
}
