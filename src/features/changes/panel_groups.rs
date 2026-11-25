use super::{Act, Fold, PRow, Sec, View};

impl View {
    pub(super) fn toggle_fold(&mut self, fold: Fold, on: Option<bool>) {
        let now = self.scm.folded.contains(&fold);

        if on.unwrap_or(!now) {
            self.scm.folded.insert(fold);
        } else {
            self.scm.folded.remove(&fold);
        }

        self.scm.rows = self.panel_rows();
    }

    /// The label row above panel row I.
    pub(super) fn parent(&self, i: usize) -> Option<usize> {
        parent_view(self, i)
    }

    /// The items of group label LABEL (folded ones too): file paths, comment ids, unpushed commit oids.
    pub(super) fn group_items(&self, label: &PRow) -> (Vec<String>, Vec<String>, Vec<String>) {
        group_items_view(self, label)
    }

    /// Group label LABEL has 1+ items, all of them picked.
    pub(super) fn group_picked(&self, label: &PRow) -> bool {
        let (paths, comment_ids, oids) = self.group_items(label);

        (!paths.is_empty() || !comment_ids.is_empty() || !oids.is_empty())
            && paths.iter().all(|path| self.scm.picked.contains(path))
            && comment_ids.iter().all(|id| self.scm.comment_picks.contains(id))
            && oids.iter().all(|oid| self.scm.commit_picks.contains(oid))
    }

    /// Space on group label LABEL: its items picked; all picked already: 0 picked.
    pub(super) fn pick_group(&mut self, label: &PRow) -> Act {
        pick_group_view(self, label)
    }
}

fn group_items_commentfile(label: &PRow, view: &View) -> (Vec<String>, Vec<String>, Vec<String>) {
    let at = view
        .scm
        .rows
        .iter()
        .position(|row| row == label)
        .unwrap_or(view.scm.rows.len());

    let ids = view.scm.rows[at.saturating_add(1).min(view.scm.rows.len())..]
        .iter()
        .map_while(|row| {
            if let PRow::Comment(ci) = row {
                view.comment_state.comments.get(*ci).map(|comment| comment.id.clone())
            } else {
                None
            }
        })
        .collect();

    (vec![], ids, vec![])
}

fn parent_view(view: &View, i: usize) -> Option<usize> {
    let mine = &view.scm.rows.get(i)?;

    if let PRow::CommitFile(ci, _) = mine {
        return view.scm.rows.iter().position(|row| *row == PRow::Commit(*ci));
    }

    let is_file_of_level = matches!(mine, PRow::File(_, false));
    let is_comment = matches!(mine, PRow::CommentFile(..) | PRow::Comment(_));
    let is_commit = matches!(mine, PRow::Commit(_));
    let is_clevel = matches!(mine, PRow::CLevel(..));

    (0..i).rev().find(|&j| match &view.scm.rows[j] {
        PRow::Level(..) => is_file_of_level,
        PRow::CLevel(..) => is_comment,
        PRow::CState(..) => is_comment || is_clevel,
        PRow::Push(..) => is_commit,
        PRow::Section(..) => true,
        _ => false,
    })
}

fn group_items_view(view: &View, label: &PRow) -> (Vec<String>, Vec<String>, Vec<String>) {
    let path = |i: usize| view.changes.files[i].path.clone();
    let staged = |i: usize| view.changes.staged.get(&view.changes.files[i].path);

    let files = |keep: &dyn Fn(usize) -> bool| {
        (0..view.changes.files.len())
            .filter(|&i| keep(i))
            .map(path)
            .collect::<Vec<_>>()
    };

    let (here, _) = view.comments_here();

    let comments = |keep: &dyn Fn(&crate::features::changes::Comment) -> bool| {
        here.iter()
            .filter(|(i, _)| keep(&view.comment_state.comments[*i]))
            .map(|(i, _)| view.comment_state.comments[*i].id.clone())
            .collect::<Vec<_>>()
    };

    let unpushed = || {
        view.changes
            .commits
            .list
            .iter()
            .filter(|commit| !commit.pushed && !view.pushing.contains(&commit.oid))
            .map(|commit| commit.oid.clone())
            .collect::<Vec<_>>()
    };

    match label {
        PRow::Section(Sec::Staged, _) => (files(&|i| staged(i).is_some()), vec![], vec![]),
        PRow::Section(Sec::Changes, _) => (files(&|i| staged(i) != Some(&true)), vec![], vec![]),
        PRow::Level(level, _) => (
            files(&|i| staged(i) != Some(&true) && view.level_of(i) == *level),
            vec![],
            vec![],
        ),
        PRow::Section(Sec::Comments, _) => (vec![], comments(&|_| true), vec![]),
        PRow::CState(resolved, _) => (vec![], comments(&|comment| comment.open() != *resolved), vec![]),
        PRow::CLevel(level, _) => (
            vec![],
            comments(&|comment| comment.open() && comment.level == *level),
            vec![],
        ),
        // a comments' file: its comments, the rows under it (never folded)
        PRow::CommentFile(..) => group_items_commentfile(label, view),
        PRow::Section(Sec::Commits, _) | PRow::Push(false, _) => (vec![], vec![], unpushed()),
        _ => (vec![], vec![], vec![]),
    }
}

fn pick_group_view(view: &mut View, label: &PRow) -> Act {
    let (files, comments, commits) = view.group_items(label);

    if files.is_empty() && comments.is_empty() && commits.is_empty() {
        return Act::Toast(match label {
            PRow::Push(true, _) => "pushed: read only, 0 pick".into(),
            PRow::Section(Sec::Stash, _) => "stash: 0 pick (enter restores, x removes)".into(),
            _ => "0 items to pick".into(),
        });
    }

    let all = view.group_picked(label);
    let count = files.len() + comments.len() + commits.len();

    for path in files {
        if all {
            view.scm.picked.remove(&path)
        } else {
            view.scm.picked.insert(path)
        };
    }

    for id in comments {
        if all {
            view.scm.comment_picks.remove(&id)
        } else {
            view.scm.comment_picks.insert(id)
        };
    }

    for oid in commits {
        if all {
            view.scm.commit_picks.remove(&oid)
        } else {
            view.scm.commit_picks.insert(oid)
        };
    }

    view.layout.built = None;

    Act::Toast(if all {
        format!("{count} unpicked")
    } else {
        format!("{count} picked")
    })
}
