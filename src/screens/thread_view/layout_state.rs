use super::{Pane, Rect};

pub(super) struct LayoutState {
    pub(super) bar_rects: Vec<(Rect, Pane)>,
    /// The `⌂ home` pill left of the view bar (a click: the start screen).
    pub(super) home_rect: Rect,
    /// The changes view with the chat right of it (wide terminals; alt+\ toggles); the view's area.
    pub(super) split: bool,
    /// The view follows the width (changes + chat when wide, chat when narrow) until the owner
    /// picks a view.
    pub(super) auto_pane: bool,
    /// The last draw had the changes + chat side by side.
    pub(super) wide_split: bool,
    pub(super) view_rect: Rect,
    pub(super) input_rect: Rect,
    pub(super) hint_rect: Rect,
    pub(super) stream_rect: Rect,
}

impl LayoutState {
    pub(super) fn new() -> Self {
        Self {
            bar_rects: Vec::new(),
            home_rect: Rect::default(),
            split: true,
            auto_pane: true,
            wide_split: false,
            view_rect: Rect::default(),
            input_rect: Rect::default(),
            hint_rect: Rect::default(),
            stream_rect: Rect::default(),
        }
    }
}
