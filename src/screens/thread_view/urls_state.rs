use super::{Instant, Receiver, Rect};

pub(super) struct UrlState {
    /// portless: the live URLs of this thread's worktree (read every 5 s), the read on its way, the
    /// link on row 0.
    pub(super) urls: Vec<String>,
    pub(super) urls_read: Option<Receiver<Vec<String>>>,
    pub(super) urls_at: Option<Instant>,
    pub(super) url_rect: Option<(Rect, String)>,
}

impl UrlState {
    pub(super) fn new() -> Self {
        Self {
            urls: Vec::new(),
            urls_read: None,
            urls_at: None,
            url_rect: None,
        }
    }
}
