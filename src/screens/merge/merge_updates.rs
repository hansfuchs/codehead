use super::{Instant, Merge, Pr, Res, associate};

impl Merge {
    pub(super) fn read_assoc(&mut self) {
        let Some(text) = self.sources.clone() else { return };

        if !self.budget() {
            return;
        }

        self.assoc_busy = true;
        self.assoc_at = Some(Instant::now());
        self.spawn(move || Res::Prs(associate(&text)));
    }

    /// The message to update the branch of P (behind: a merge; conflicts: a smart resolve).
    pub(super) fn update_prompt(&self, path: &Pr) -> String {
        let dir = self
            .dirs
            .get(&path.pr.repo)
            .map(|directory| format!(" The checkout: `{}`.", directory.display()))
            .unwrap_or_default();

        if path.conflicts() {
            format!(
                "The branch `{}` of pull request {} has conflicts with `{}`.{dir} Fetch, merge `origin/{}` into the branch (0 rebase, 0 force push) and resolve every conflict: keep the intent of both sides; read the commits on both sides where it is unclear. Build and run the tests, commit the merge, push. Then list each conflict and how you resolved it.",
                path.head, path.url, path.base, path.base
            )
        } else {
            format!(
                "Update the branch `{}` of pull request {} with `{}`.{dir} Fetch, merge `origin/{}` into it (0 rebase, 0 force push), push. 0 conflicts expected: if there are any, stop and tell me.",
                path.head, path.url, path.base, path.base
            )
        }
    }
}
