use super::{Frame, Paragraph, Rect, View};
pub(super) struct StreamFrame {
    pub(super) area: Rect,
    pub(super) shown: Rect,
    pub(super) start: usize,
    pub(super) end: usize,
    pub(super) cut: usize,
    pub(super) total: usize,
}
impl View {
    pub(super) fn draw_stream(&mut self, frame: &mut Frame, area: Rect) {
        if self.stream.dirty || self.stream.width != area.width {
            self.refresh_stream(area);
        }

        self.mark_stream_seen();
        let (rows, window) = self.stream_rows(area);

        frame.render_widget(Paragraph::new(rows), window.shown);
        self.draw_stream_waves(frame, &window);
        self.selection.draw(frame, self, &window);
        self.draw_stream_hover(frame, &window);
        self.draw_stream_fold(frame, area);
        self.draw_stream_empty(frame, &window);
    }
}
