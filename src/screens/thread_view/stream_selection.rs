use super::stream::StreamFrame;
use super::{Frame, SelectionState, View, content_cols, theme};
impl SelectionState {
    pub(super) fn draw(&self, frame: &mut Frame, view: &View, window: &StreamFrame) {
        let Some((start, end)) = self.sel else {
            return;
        };

        let (start, end) = if start <= end { (start, end) } else { (end, start) };

        for y in window.shown.y..window.shown.bottom() {
            Self::draw_row(frame, view, window, (start, end), y);
        }
    }
    fn draw_row(frame: &mut Frame, view: &View, window: &StreamFrame, range: ((usize, u16), (usize, u16)), y: u16) {
        let ((r0, c0), (r1, c1)) = range;
        let area = window.area;

        let Some(row) = view.row_at(y) else {
            return;
        };

        if row < r0 || row > r1 {
            return;
        }

        let text: String = view.stream.drawn.rows[row]
            .line
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect();

        let Some((text_start, text_end)) = content_cols(&text) else {
            return;
        };

        if text_end <= text_start {
            return;
        }

        let (text_start, text_end) = (text_start as u16, text_end as u16 - 1);
        let (first, last) = (
            if row == r0 { c0.max(text_start) } else { text_start },
            if row == r1 { c1.min(text_end) } else { text_end },
        );

        if last < first {
            return;
        }

        let buffer = frame.buffer_mut();

        for x in first..=last.min(area.width.saturating_sub(1)) {
            buffer[(area.x + x, y)].set_style(theme::pill());
        }
    }
}
