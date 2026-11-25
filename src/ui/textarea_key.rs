use super::{Instant, KEY_AT, KeyEvent, Outcome, TextArea};

impl TextArea {
    pub(crate) fn key(&mut self, key: KeyEvent) -> Outcome {
        self.key_at(key, KEY_AT.with(std::cell::Cell::get).unwrap_or_else(Instant::now))
    }
}
