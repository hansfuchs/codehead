use super::Rect;
#[cfg(test)]
use std::path::PathBuf;

pub(super) struct IdeState {
    /// The IDE's repo picker (ctrl+i with several repos): cursor, 1st row in view; its drawn rows;
    /// the repos line below the prompt (its anchor).
    pub(super) ide_pick: Option<(usize, usize)>,
    pub(super) ide_rows: Vec<(Rect, usize)>,
    /// The setup tutorial: an IDE open was asked (0 IDE opens there, a toast says so).
    pub(super) tutor_ide: bool,
    #[cfg(test)]
    pub(super) ide_opened: Vec<PathBuf>,
}

impl IdeState {
    pub(super) fn new() -> Self {
        Self {
            ide_pick: None,
            ide_rows: Vec::new(),
            tutor_ide: false,
            #[cfg(test)]
            ide_opened: Vec::new(),
        }
    }
}
