use super::Rect;

pub(super) struct MiniChatState {
    /// The chat above the prompt in the changes view (0 split): open; its title row (a click
    /// folds / opens it).
    pub(super) mini_chat: bool,
    pub(super) mini_rect: Rect,
}

impl MiniChatState {
    pub(super) fn new() -> Self {
        Self {
            mini_chat: true,
            mini_rect: Rect::default(),
        }
    }
}
