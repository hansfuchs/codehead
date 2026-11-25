use super::{Instant, PathBuf};

pub(super) struct MergeState {
    /// The merge tab: the thread's pull requests.
    pub(super) merge: crate::screens::merge::Merge,
    pub(super) merge_sources_at: Option<Instant>,
    /// The merge tab's sources read (git remotes, the log's PR links): off the UI thread.
    pub(super) merge_reads:
        crate::infra::background::Reads<(), (Vec<(String, PathBuf)>, crate::screens::merge::Sources)>,
}

impl MergeState {
    pub(super) fn new(id: &str) -> Self {
        Self {
            merge: crate::screens::merge::Merge::for_thread(id),
            merge_sources_at: None,
            merge_reads: crate::infra::background::Reads::default(),
        }
    }
}
