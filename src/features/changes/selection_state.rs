use super::{HashMap, Instant, LKey};

pub(super) struct SelectionState {
    /// The selected diff lines per path (space, a mouse drag): a comment goes on them.
    pub(super) sel: HashMap<String, std::collections::BTreeSet<LKey>>,
    /// A mouse drag: its 1st row, the selection before it, adds (left button) or takes out
    /// (right button).
    pub(super) drag: Option<(usize, HashMap<String, std::collections::BTreeSet<LKey>>, bool)>,
    /// The last right click (a 2nd one soon after: the selection clears); the last left click's
    /// row (shift+click: the rows from it).
    pub(super) right_at: Option<Instant>,
    pub(super) anchor: Option<usize>,
}

impl SelectionState {
    pub(super) fn new() -> Self {
        Self {
            sel: HashMap::new(),
            drag: None,
            right_at: None,
            anchor: None,
        }
    }
}
