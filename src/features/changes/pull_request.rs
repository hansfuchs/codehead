use super::{Instant, Pr, Receiver};

pub(super) struct PullRequest {
    /// The branch's pull request (read every 30 s while the view is open), the read on its way.
    pub(super) pr: Option<Pr>,
    pub(super) pr_read: Option<Receiver<Option<Pr>>>,
    pub(super) pr_at: Option<Instant>,
}

impl PullRequest {
    pub(super) fn new() -> Self {
        Self {
            pr: None,
            pr_read: None,
            pr_at: None,
        }
    }
}
