use super::{
    Cat, Changes, Comment, FileDiff, HashMap, HashSet, Instant, SIDE_MIN, View, comments_stamp, committed, give_ids,
    load_commit, load_from, load_pr, merge_comments, read_comments, resolve_committed, stash, update_comments,
};
use crate::infra::bus::{Git, Topic};

impl View {
    /// The stash file changed (an agent named an entry, a stash ended): read it again; new
    /// entries: their agent gets asked for a name (NOW: read even with 0 change seen).
    pub(super) fn sync_stash(&mut self, now: bool) -> bool {
        sync_stash_view(self, now)
    }

    /// The view's changes of the comments onto their file (merged with writes of agents since
    /// the view's read; the view's own write: 0 read again).
    pub(super) fn save_comments(&mut self) {
        let (base, mine) = (
            std::mem::take(&mut self.comment_state.comments_base),
            std::mem::take(&mut self.comment_state.comments),
        );

        let merged = update_comments(&self.id, |all| {
            *all = merge_comments(&base, &mine, std::mem::take(all));
            give_ids(all);
            all.clone()
        });

        self.comment_state.comments_base = merged.clone();
        self.comment_state.comments = merged;
        self.comment_state.comments_at = comments_stamp(&self.id);
    }

    /// The comments file changed (an agent wrote comments): read it again.
    pub(super) fn sync_comments(&mut self) {
        let at = comments_stamp(&self.id);

        if at == self.comment_state.comments_at {
            return;
        }

        self.comment_state.comments_at = at;
        let comment = read_comments(&self.id);

        self.comment_state.comments_base.clone_from(&comment);

        if comment != self.comment_state.comments {
            self.comment_state.comments = comment;
            self.layout.built = None;
        }
    }

    /// Staged files exist (a commit can start).
    pub(crate) fn has_staged(&self) -> bool {
        !self.changes.staged.is_empty()
    }

    /// A bus topic since the last call touches this checkout: its work tree, its repo's refs, its
    /// own HEAD / index (another worktree's: 0).
    pub(super) fn git_changed(&mut self) -> bool {
        git_changed_view(self)
    }

    /// Read git in the background (NOW: even if a read happened; else only the 1st read: later
    /// ones come from bus topics).
    pub(crate) fn refresh(&mut self, now: bool) {
        refresh_view(self, now);
    }

    /// A finished read applies. True: something changed.
    pub(crate) fn poll(&mut self) -> bool {
        poll_view(self)
    }

    pub(crate) fn is_reviewed(&self, file: &FileDiff) -> bool {
        self.marks.get(&file.path) == Some(&file.hash)
    }

    /// Reviewed boxes / all boxes.
    pub(crate) fn progress(&self) -> (usize, usize) {
        (
            self.changes.files.iter().filter(|file| self.is_reviewed(file)).count(),
            self.changes.files.len(),
        )
    }

    pub(super) fn folded(&self, file: &FileDiff) -> bool {
        let default = self.is_reviewed(file) || file.cat == Cat::Generated || file.binary;

        default != self.toggled.contains(&file.path)
    }

    pub(super) fn side_by_side(&self, width: u16) -> bool {
        self.side.unwrap_or(width >= SIDE_MIN)
    }
}

fn poll_branch(mut character: Changes, view: &mut View) -> bool {
    if resolve_committed(
        &mut view.comment_state.comments,
        &std::mem::take(&mut character.committed),
    ) {
        view.save_comments();
        view.layout.built = None;
    }

    // the same diffs (the view's order is the agent's, the read's the path rules'):
    // 0 new rows; the git state (staged, ahead, a stopped rebase) still applies
    let hashes = |fs: &[FileDiff]| {
        let mut value: Vec<u64> = fs.iter().map(|file| file.hash).collect();

        value.sort_unstable();
        value
    };

    if hashes(&character.files) == hashes(&view.changes.files) && view.error.is_none() {
        let git = (
            &character.base,
            &character.staged,
            &character.stopped,
            character.ahead,
            character.behind,
            &character.commits,
        );
        let changed = git
            != (
                &view.changes.base,
                &view.changes.staged,
                &view.changes.stopped,
                view.changes.ahead,
                view.changes.behind,
                &view.changes.commits,
            );

        if changed {
            view.layout.built = view
                .layout
                .built
                .filter(|_| character.stopped.is_some() == view.changes.stopped.is_some());
            let files = std::mem::take(&mut view.changes.files);

            view.changes = Changes { files, ..character };
            view.sync_commits();
        }

        return changed;
    }

    // the cursor stays on its row of its file
    let cur = view.cursor_spot();

    // a selection holds on files whose diff stayed
    retain_selected_files(&character, view);

    if view.layout.paints.borrow().len() > 4096 {
        view.layout.paints.borrow_mut().clear();
    }

    view.changes = character;
    view.sync_commits();
    view.error = None;
    view.layout.built = None;
    view.apply_rank(false);
    view.pending_cursor = cur;

    true
}

fn retain_selected_files(changes_value: &Changes, view: &mut View) {
    let kept: HashSet<(&str, u64)> = changes_value
        .files
        .iter()
        .map(|file| (file.path.as_str(), file.hash))
        .collect();
    let old: HashMap<&str, u64> = view
        .changes
        .files
        .iter()
        .map(|file| (file.path.as_str(), file.hash))
        .collect();

    view.selection_state.sel.retain(|path, _| {
        old.get(path.as_str())
            .is_some_and(|height| kept.contains(&(path.as_str(), *height)))
    });
}

fn sync_stash_view(view: &mut View, now: bool) -> bool {
    let at = stash::stamp(&view.id);

    if !now && at == view.stash_state.stash_at {
        return false;
    }

    view.stash_state.stash_at = at;
    view.stash_state.stash = stash::read(&view.id);
    let mut asked = false;

    for error in view
        .stash_state
        .stash
        .iter_mut()
        .filter(|error| !error.asked && error.name.is_empty())
    {
        error.asked = true;
        asked = true;
        view.operation_state.outbox.push(stash::name_ask(error));
    }

    if asked {
        stash::write(&view.id, &view.stash_state.stash);
        view.stash_state.stash_at = stash::stamp(&view.id);
    }

    true
}

fn git_changed_view(view: &mut View) -> bool {
    let mut hit = false;

    for thread in view.topics.try_iter() {
        let Some((top, common, own)) = &view.git else { continue };

        hit |= topic_hits_repository(&thread, top, common, own);
    }

    hit
}

fn topic_hits_repository(
    thread: &crate::infra::bus::Topic,
    top: &std::path::PathBuf,
    common: &std::path::PathBuf,
    own: &std::path::PathBuf,
) -> bool {
    match &thread {
        Topic::Work(width) => width == top,
        Topic::Git(group, Git::Worktree(count)) => {
            group == common
                && (if count.is_empty() {
                    own == common
                } else {
                    own.file_name().is_some_and(|file| file == count.as_str())
                })
        }
        Topic::Git(group, _) => group == common,
        Topic::Rescan(result) => top.starts_with(result) || common.starts_with(result),
        Topic::Thread(..) => false,
    }
}

fn refresh_view(view: &mut View, now: bool) {
    if view.loading.is_some() || (!now && view.loaded.is_some()) {
        return;
    }

    // PR mode: a read on a new head only (gh calls are counted)
    if !now && view.pr_src.is_some() && (view.loaded.is_some() || view.error.is_some()) {
        return;
    }

    // a commit: its diff never changes, 1 read
    if !now && view.commit_src.is_some() && (view.loaded.is_some() || view.error.is_some()) {
        return;
    }

    let (tx, rx) = crate::infra::bus::channel();

    if let Some(character) = view.commit_src.clone() {
        let dir = view.dir.clone();

        std::thread::spawn(move || {
            let _ = tx.send(load_commit(&dir, &character));
        });

        view.loading = Some(rx);
        return;
    }

    if let Some(src) = view.pr_src.clone() {
        std::thread::spawn(move || {
            let _ = tx.send(load_pr(&src));
        });

        view.loading = Some(rx);
        return;
    }

    let (dir, all) = (view.dir.clone(), view.all);
    let (id, open): (String, Vec<Comment>) = (
        view.id.clone(),
        view.comment_state
            .comments
            .iter()
            .filter(|character| character.open())
            .cloned()
            .collect(),
    );

    std::thread::spawn(move || {
        let mut result = load_from(&dir, all);

        // open comments on files with 0 uncommitted changes (in their checkouts): resolved
        if let Ok(character) = &mut result
            && !open.is_empty()
        {
            let work = crate::engine::store::load(&id).map_or_else(|| dir.clone(), |thread| thread.work_dir());

            character.committed = committed(&open, &work);
        }

        let _ = tx.send(result);
    });

    view.loading = Some(rx);
}

fn poll_view(view: &mut View) -> bool {
    let Some(rx) = &view.loading else {
        return false;
    };

    let Ok(result) = rx.try_recv() else { return false };

    view.loading = None;
    view.loaded = Some(Instant::now());

    // another process's push ended: the read after it has the commits' state
    if view.push_hold {
        view.push_hold = false;
        view.pushing.clear();
        view.pushing_at = None;
    }

    match result {
        Ok(character) => poll_branch(character, view),
        Err(error) => {
            let changed = view.error.as_deref() != Some(error.as_str());

            view.error = Some(error);
            changed
        }
    }
}
