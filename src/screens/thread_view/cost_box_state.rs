use super::{Instant, Rect};
use crate::features::cost::{AllThreads, Snap};

/// The tokens box: its numbers, compact or expanded (ctrl+o / ⌘o / a click), the expanded box's
/// scroll and every thread's cost (read when it opens).
pub(super) struct CostBox {
    pub(super) snap: Option<Snap>,
    pub(super) read_at: Instant,
    pub(super) expanded: bool,
    pub(super) scroll: u16,
    pub(super) all: Option<AllThreads>,
    /// The every-thread read on its way (1 at a time).
    pub(super) all_reading: bool,
    /// A new every-thread read is due (the box expanded): the last list stays until it comes.
    pub(super) all_due: bool,
    /// The box as drawn: a click in it toggles.
    pub(super) rect: Rect,
    /// Expanded: the compact box's place and the top it grows to, for the draw after the prompt.
    pub(super) anchor: Option<(Rect, u16)>,
}

impl CostBox {
    pub(super) fn new() -> Self {
        Self {
            snap: None,
            read_at: Instant::now()
                .checked_sub(std::time::Duration::from_secs(60))
                .unwrap_or_else(Instant::now),
            expanded: false,
            scroll: 0,
            all: None,
            all_reading: false,
            all_due: false,
            rect: Rect::default(),
            anchor: None,
        }
    }

    /// Compact ↔ expanded; expanded: from its top, every thread read again (the box opens on
    /// the last list: its height right at once).
    pub(super) fn toggle(&mut self) {
        self.expanded = !self.expanded;
        self.scroll = 0;
        self.all_due |= self.expanded;
    }

    /// Every thread's cost wants a read: 0 list yet (read ahead while compact), or due.
    pub(super) fn wants_all(&self) -> bool {
        (self.all.is_none() || self.all_due) && !self.all_reading
    }
}
