use super::{Instant, KEY_AT, KeyEvent, Outcome, TextArea};

impl TextArea {
    pub(crate) fn key(&mut self, key_value: KeyEvent) -> Outcome {
        self.key_at(
            key_value,
            KEY_AT.with(std::cell::Cell::get).unwrap_or_else(Instant::now),
        )
    }
}
