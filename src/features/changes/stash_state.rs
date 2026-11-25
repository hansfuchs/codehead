use super::stash;

pub(super) struct StashState {
    /// The stash (read again when its file changes: an agent named an entry).
    pub(super) stash: Vec<stash::Entry>,
    pub(super) stash_at: Option<std::time::SystemTime>,
}

impl StashState {
    pub(super) fn new(id: &str) -> Self {
        Self {
            stash: stash::read(id),
            stash_at: stash::stamp(id),
        }
    }
}
