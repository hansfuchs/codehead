use super::{Instant, Position};

pub(super) struct SelectionState {
    /// Drag selection in the stream: (row, col) of its start and end, in drawn rows.
    pub(super) sel: Option<((usize, u16), (usize, u16))>,
    pub(super) dragging: bool,
    /// Last click in the stream: time, (row, col), count (2: word, 3: row).
    pub(super) clicks: Option<(Instant, (usize, u16), u8)>,
    /// Mouse position: the clickable row under it gets the hover bg.
    pub(super) hover: Option<Position>,
    /// The last drawn frame: ctrl+click reads the URL under the mouse from it.
    pub(super) last_frame: ratatui::buffer::Buffer,
    /// Where the drag started (drawn row, col).
    pub(super) drag_from: Option<(usize, u16)>,
}

impl SelectionState {
    pub(super) fn new() -> Self {
        Self {
            sel: None,
            dragging: false,
            clicks: None,
            hover: None,
            last_frame: ratatui::buffer::Buffer::default(),
            drag_from: None,
        }
    }
}
