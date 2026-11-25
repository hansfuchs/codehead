use super::{Modal, Rect};

pub(super) struct ModalState {
    /// A modal over the view: a comment, a question; its buttons (yes / save: true).
    pub(super) modal: Option<Modal>,
    pub(super) modal_btns: Vec<(Rect, bool)>,
    /// The comment modal's selected lines on screen: the wheel scrolls them.
    pub(super) modal_quote: Rect,
    /// An alt press with 0 key after it yet: its release toggles the focus.
    /// The last esc in the comment modal: a 2nd within 2 s drops an edited comment.
    pub(super) esc_at: Option<std::time::Instant>,
}

impl ModalState {
    pub(super) fn new() -> Self {
        Self {
            modal: None,
            modal_btns: Vec::new(),
            modal_quote: Rect::default(),
            esc_at: None,
        }
    }
}
