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
        let st = window.area;

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

        let Some((ta, tb)) = content_cols(&text) else {
            return;
        };

        if tb <= ta {
            return;
        }

        let (ta, tb) = (ta as u16, tb as u16 - 1);
        let (first, last) = (
            if row == r0 { c0.max(ta) } else { ta },
            if row == r1 { c1.min(tb) } else { tb },
        );

        if last < first {
            return;
        }

        let buffer = frame.buffer_mut();

        for x in first..=last.min(st.width.saturating_sub(1)) {
            buffer[(st.x + x, y)].set_style(theme::pill());
        }
    }
}
