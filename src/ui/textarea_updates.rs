use super::{Op, TextArea};

impl TextArea {
    /// New value as 1 undo step (chat history ↑↓): ctrl+z returns to the text before.
    pub(crate) fn load_text(&mut self, text: &str) {
        if text == self.text() {
            return;
        }

        self.snapshot(Op::Other);
        self.replace_all(text);
        self.last_op = None;
    }
}
