use super::HashMap;

pub(super) struct FilterState {
    /// The filter: its text, it has the keys, its hits (file index, name marks; by `build`).
    pub(super) filter: String,
    /// The filter's input (cursor, selection); FILTER: its text.
    pub(super) filter_ta: crate::ui::textarea::TextArea,
    pub(super) filtering: bool,
    pub(super) hits: HashMap<usize, Vec<usize>>,
}

impl FilterState {
    pub(super) fn new() -> Self {
        Self {
            filter: String::new(),
            filter_ta: crate::ui::textarea::TextArea::new(true, ""),
            filtering: false,
            hits: HashMap::new(),
        }
    }
}
