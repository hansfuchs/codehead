use super::{HashSet, chat};

pub(super) struct StreamState {
    pub(super) drawn: chat::Drawn,
    pub(super) dirty: bool,
    pub(super) width: u16,
    /// Rows scrolled up from the bottom (0: live).
    pub(super) back: usize,
    pub(super) open: HashSet<usize>,
    pub(super) all_runs: bool,
    /// Thoughts: how they show (setting), the open folds, ctrl+t: all open.
    pub(super) thoughts: chat::Thoughts,
    pub(super) open_thoughts: HashSet<usize>,
    pub(super) all_thoughts: bool,
    /// Per agent: live text not logged yet (token chunks), shown at the end of the stream.
    pub(super) live: Vec<(String, String)>,
    /// How the stream draws messages (setting `messages`).
    pub(super) look: chat::Look,
    /// A search result to scroll to (its message time); the item it found, marked until a key.
    pub(super) jump: Option<u64>,
    pub(super) flash: Option<usize>,
}

impl StreamState {
    pub(super) fn new(id: &str) -> Self {
        Self {
            drawn: chat::Drawn::default(),
            dirty: true,
            width: 0,
            back: 0,
            open: HashSet::new(),
            all_runs: false,
            thoughts: crate::infra::config::thoughts(),
            open_thoughts: HashSet::new(),
            all_thoughts: false,
            live: Vec::new(),
            look: crate::infra::config::messages(),
            jump: crate::domain::search::take_jump(id),
            flash: None,
        }
    }
}
