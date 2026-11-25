use super::{Instant, PathBuf};

pub(super) struct CheckoutState {
    /// The thread's checkouts (its work dir 1st), the one the changes view shows.
    pub(super) checkouts: Vec<PathBuf>,
    pub(super) checkout: usize,
    /// The owner picked the checkout (a tab, ctrl+w): it stays; else the view follows the
    /// thread's newest worktree.
    pub(super) checkout_picked: bool,
    /// The repo tabs on top of the changes view (shift+←→, ctrl+w, a click): 1 per checkout.
    pub(super) checkout_tabs: crate::ui::tab_group::TabGroup,
    /// The repo tabs have the keys (↑ from the diff's top, a click): ←→ pick a repo.
    pub(super) repo_focus: bool,
    pub(super) checkout_labels: Vec<String>,
    /// Under the prompt: the repos, branches, worktrees the thread works in (read again every 3 s).
    pub(super) spots: Vec<crate::screens::thread_view::workspot::Spot>,
    pub(super) spots_at: Option<Instant>,
}

impl CheckoutState {
    pub(super) fn new() -> Self {
        Self {
            checkouts: Vec::new(),
            checkout: 0,
            checkout_picked: false,
            checkout_tabs: crate::ui::tab_group::TabGroup::default(),
            repo_focus: false,
            checkout_labels: Vec::new(),
            spots: Vec::new(),
            spots_at: None,
        }
    }
}
