use super::{
    Changes, HashSet, Path, PrSrc, View, classifier, comment_state, commits, filter_state, layout, modal_state,
    operation_state, pull_request, read_marks, reviewer, scm, selection_state, stash_state,
};

impl View {
    /// The changes of pull request PR (DIR: a checkout of its repo, else any dir).
    pub(crate) fn for_pr(pr: &crate::infra::gh::PrRef, dir: &Path) -> View {
        let mut view = View::new(&format!("pr~{}", pr.key().replace('/', "~")), dir);

        view.pr_src = Some(PrSrc {
            pr: pr.clone(),
            ..Default::default()
        });
        view
    }

    /// PR mode: the pull request's base, commits, threads (a new commit list: the picker keeps
    /// its pick while it is still there).
    pub(crate) fn set_pr(&mut self, path: &crate::infra::gh::Pr) {
        set_pr_view(self, path);
    }

    /// Tests: the cursor on file I's box.
    #[cfg(test)]
    pub(crate) fn jump_to_file(&mut self, i: usize) {
        self.jump_file(i);
    }

    /// PR mode: show COMMIT's diff (None: all), read now.
    pub(crate) fn pick_commit(&mut self, commit: Option<String>) {
        if let Some(src) = self.pr_src.as_mut()
            && src.commit != commit
        {
            src.commit = commit;
            self.loading = None;
            self.refresh(true);
        }
    }

    /// The checkout the view reads.
    pub(crate) fn dir(&self) -> &Path {
        &self.dir
    }

    pub(crate) fn new(id: &str, dir: &Path) -> View {
        View {
            operation_state: operation_state::OperationState::new(),
            pull_request: pull_request::PullRequest::new(),
            reviewer: reviewer::Reviewer::new(id),
            comment_state: comment_state::CommentState::new(id),
            stash_state: stash_state::StashState::new(id),
            modal_state: modal_state::ModalState::new(),
            selection_state: selection_state::SelectionState::new(),
            filter_state: filter_state::FilterState::new(),
            classifier: classifier::Classification::new(id),
            layout: layout::Layout::new(),
            id: id.to_string(),
            dir: dir.to_path_buf(),
            topics: crate::infra::bus::subscribe(),
            _shown: crate::infra::watch::show(dir),
            git: crate::infra::watch::git_dirs(dir).map(|(thread, character, output)| {
                (
                    crate::infra::watch::canon(&thread),
                    crate::infra::watch::canon(&character),
                    crate::infra::watch::canon(&output),
                )
            }),
            stale: false,
            changes: Changes::default(),
            marks: read_marks(id),
            toggled: HashSet::new(),
            side: None,
            at: 0,
            top: 0,
            loading: None,
            loaded: None,
            error: None,
            pending_cursor: None,
            scm: scm::Panel::new(),
            del: None,
            del_prev: None,
            pushing: Vec::new(),
            pushing_at: None,
            push_by: None,
            push_hold: false,
            #[cfg(test)]
            branch_override: None,
            pr_src: None,
            commit_src: None,
            peek: None,
            agents: Vec::new(),
            members: Vec::new(),
            all: false,
            marks_id: id.to_string(),
        }
    }

    /// The read-only view of commit C of DIR's branch (the review box of the panel's commit).
    pub(crate) fn for_commit(dir: &Path, character: &commits::Commit) -> View {
        let mut view = View::new(&format!("commit~{}", character.short), dir);

        view.marks.clear();
        view.comment_state.comments.clear();
        view.stash_state.stash.clear();
        view.reviewer.review = None;
        view.reviewer.review_model = None;
        view.classifier.classifier = None;
        view.classifier.rank = None;
        view.scm.shown = false;
        view.commit_src = Some(character.clone());

        view
    }

    /// The branch's changes against its merge-base of thread ID (the panel's "all changes" row):
    /// the thread's comments, its own reviewed marks, 0 git steps.
    pub(crate) fn for_branch(id: &str, dir: &Path) -> View {
        let mut view = View::new(id, dir);

        view.all = true;
        view.marks_id = format!("{id}~all");
        view.marks = read_marks(&view.marks_id);
        view.stash_state.stash.clear();
        view.reviewer.review = None;
        view.reviewer.review_model = None;
        view.classifier.classifier = None;
        view.classifier.rank = None;
        view.scm.shown = false;

        view
    }

    /// The commit the review shows now (enter on a commit in the panel).
    pub(crate) fn peeked(&self) -> Option<&str> {
        self.peek
            .as_ref()
            .and_then(|path| path.commit_src.as_ref())
            .map(|character| character.oid.as_str())
    }
}

fn set_pr_view(view: &mut View, path: &crate::infra::gh::Pr) {
    let Some(src) = view.pr_src.as_mut() else { return };
    let threads: Vec<crate::infra::gh::Thread> = path
        .threads
        .iter()
        .filter(|thread| thread.path.is_some())
        .cloned()
        .collect();
    let changed = src.threads != threads;

    src.base.clone_from(&path.base);
    src.commits.clone_from(&path.commits);
    src.threads = threads;

    if src
        .commit
        .as_ref()
        .is_some_and(|character| !path.commits.iter().any(|x| &x.oid == character))
    {
        src.commit = None;
    }

    let pushed = !src.head.is_empty() && src.head != path.head_oid;

    src.head.clone_from(&path.head_oid);

    if changed {
        view.layout.built = None;
    }

    if pushed {
        view.refresh(true);
    }
}
