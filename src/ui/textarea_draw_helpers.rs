#[cfg(test)]
use super::Rect;
use super::TextArea;

impl TextArea {
    #[cfg(test)]
    pub(super) fn render_for_test(&mut self, width: u16) {
        let backend = ratatui::backend::TestBackend::new(width, 5);
        let mut term = ratatui::Terminal::new(backend).unwrap();

        term.draw(|frame| self.render(frame, Rect::new(0, 0, width, 5), false))
            .unwrap();
    }
}
