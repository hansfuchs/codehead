use super::{Act, Fold, PRow, Sec, View};

impl View {
    pub(super) fn toggle_fold(&mut self, file: Fold, on: Option<bool>) {
        let now = self.scm.folded.contains(&file);

        if on.unwrap_or(!now) {
            self.scm.folded.insert(file);
        } else {
            self.scm.folded.remove(&file);
        }

        self.scm.rows = self.panel_rows();
    }

    /// The label row above panel row I.
    pub(super) fn parent(&self, i: usize) -> Option<usize> {
        parent_view(self, i)
    }

    /// The items of group label R (folded ones too): file paths, comment ids, unpushed commit oids.
    pub(super) fn group_items(&self, result: &PRow) -> (Vec<String>, Vec<String>, Vec<String>) {
        group_items_view(self, result)
    }

    /// Group label R has 1+ items, all of them picked.
    pub(super) fn group_picked(&self, result: &PRow) -> bool {
        let (file, character, key) = self.group_items(result);

        (!file.is_empty() || !character.is_empty() || !key.is_empty())
            && file.iter().all(|path| self.scm.picked.contains(path))
            && character.iter().all(|x| self.scm.comment_picks.contains(x))
            && key.iter().all(|x| self.scm.commit_picks.contains(x))
    }

    /// Space on group label R: its items picked; all picked already: 0 picked.
    pub(super) fn pick_group(&mut self, result: &PRow) -> Act {
        pick_group_view(self, result)
    }
}

fn group_items_commentfile(result: &PRow, view: &View) -> (Vec<String>, Vec<String>, Vec<String>) {
    let at = view
        .scm
        .rows
        .iter()
        .position(|x| x == result)
        .unwrap_or(view.scm.rows.len());

    let ids = view.scm.rows[at.saturating_add(1).min(view.scm.rows.len())..]
        .iter()
        .map_while(|x| {
            if let PRow::Comment(ci) = x {
                view.comment_state
                    .comments
                    .get(*ci)
                    .map(|character_4| character_4.id.clone())
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
        return view.scm.rows.iter().position(|result| *result == PRow::Commit(*ci));
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

fn group_items_view(view: &View, result: &PRow) -> (Vec<String>, Vec<String>, Vec<String>) {
    let path = |i: usize| view.changes.files[i].path.clone();
    let staged = |i: usize| view.changes.staged.get(&view.changes.files[i].path);

    let files = |file: &dyn Fn(usize) -> bool| {
        (0..view.changes.files.len())
            .filter(|&i| file(i))
            .map(path)
            .collect::<Vec<_>>()
    };

    let (here, _) = view.comments_here();

    let comments = |comment: &dyn Fn(&crate::features::changes::Comment) -> bool| {
        here.iter()
            .filter(|(i, _)| comment(&view.comment_state.comments[*i]))
            .map(|(i, _)| view.comment_state.comments[*i].id.clone())
            .collect::<Vec<_>>()
    };

    let unpushed = || {
        view.changes
            .commits
            .list
            .iter()
            .filter(|character| !character.pushed && !view.pushing.contains(&character.oid))
            .map(|character_value| character_value.oid.clone())
            .collect::<Vec<_>>()
    };

    match result {
        PRow::Section(Sec::Staged, _) => (files(&|i| staged(i).is_some()), vec![], vec![]),
        PRow::Section(Sec::Changes, _) => (files(&|i| staged(i) != Some(&true)), vec![], vec![]),
        PRow::Level(line, _) => (
            files(&|i| staged(i) != Some(&true) && view.level_of(i) == *line),
            vec![],
            vec![],
        ),
        PRow::Section(Sec::Comments, _) => (vec![], comments(&|_| true), vec![]),
        PRow::CState(resolved, _) => (vec![], comments(&|character_2| character_2.open() != *resolved), vec![]),
        PRow::CLevel(line_value, _) => (
            vec![],
            comments(&|character_3| character_3.open() && character_3.level == *line_value),
            vec![],
        ),
        // a comments' file: its comments, the rows under it (never folded)
        PRow::CommentFile(..) => group_items_commentfile(result, view),
        PRow::Section(Sec::Commits, _) | PRow::Push(false, _) => (vec![], vec![], unpushed()),
        _ => (vec![], vec![], vec![]),
    }
}

fn pick_group_view(view: &mut View, result: &PRow) -> Act {
    let (files, comments, commits) = view.group_items(result);

    if files.is_empty() && comments.is_empty() && commits.is_empty() {
        return Act::Toast(match result {
            PRow::Push(true, _) => "pushed: read only, 0 pick".into(),
            PRow::Section(Sec::Stash, _) => "stash: 0 pick (enter restores, x removes)".into(),
            _ => "0 items to pick".into(),
        });
    }

    let all = view.group_picked(result);
    let count = files.len() + comments.len() + commits.len();

    for path in files {
        if all {
            view.scm.picked.remove(&path)
        } else {
            view.scm.picked.insert(path)
        };
    }

    for comment in comments {
        if all {
            view.scm.comment_picks.remove(&comment)
        } else {
            view.scm.comment_picks.insert(comment)
        };
    }

    for character in commits {
        if all {
            view.scm.commit_picks.remove(&character)
        } else {
            view.scm.commit_picks.insert(character)
        };
    }

    view.layout.built = None;

    Act::Toast(if all {
        format!("{count} unpicked")
    } else {
        format!("{count} picked")
    })
}
