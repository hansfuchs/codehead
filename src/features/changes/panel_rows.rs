use super::{Fold, Level, PRow, Panel, PathBuf, Sec, View};

impl View {
    pub(super) fn level_of(&self, i: usize) -> Level {
        self.classifier.lv.get(i).copied().unwrap_or(Level::Medium)
    }

    /// The comments of this view's checkout: (index, path in it); the open comments of other
    /// checkouts: (root, count), in order of the 1st comment.
    pub(in crate::features::changes) fn comments_here(&self) -> (LocalComments, CheckoutCounts) {
        comments_here_view(self)
    }

    /// Comment CI's path in this view's checkout (None: another checkout's).
    pub(in crate::features::changes) fn comment_path(&self, ci: usize) -> Option<String> {
        match crate::features::changes::place(self.comment_state.comments.get(ci)?) {
            (Some(result), _) if !crate::features::changes::same_dir(&result, &self.dir) => None,
            (_, rel) => Some(rel),
        }
    }

    /// The panel's rows now.
    pub(super) fn panel_rows(&self) -> Vec<PRow> {
        panel_rows_view(self)
    }
}

type LocalComments = Vec<(usize, String)>;
type CheckoutCounts = Vec<(PathBuf, usize)>;

fn panel_file_rows(panel: &Panel, order: &[usize], rows: &mut Vec<PRow>, pr: bool, view: &View) {
    let staged: Vec<usize> = order
        .iter()
        .copied()
        .filter(|&i| view.changes.staged.contains_key(&view.changes.files[i].path))
        .collect();

    if !pr {
        rows.push(PRow::Section(Sec::Staged, staged.len()));
    }

    if !pr && !panel.folded.contains(&Fold::Sec(Sec::Staged)) {
        rows.extend(staged.iter().map(|&i| PRow::File(i, true)));
    }

    let open: Vec<usize> = order
        .iter()
        .copied()
        .filter(|&i| view.changes.staged.get(&view.changes.files[i].path) != Some(&true))
        .collect();

    rows.push(PRow::Section(Sec::Changes, open.len()));

    if !panel.folded.contains(&Fold::Sec(Sec::Changes)) {
        panel_level_rows(panel, rows, view, &open);
    }
}

fn panel_level_rows(panel: &Panel, rows: &mut Vec<PRow>, view: &View, open: &[usize]) {
    for line in Level::ALL {
        let fs: Vec<usize> = open.iter().copied().filter(|&i| view.level_of(i) == line).collect();

        if fs.is_empty() {
            continue;
        }

        rows.push(PRow::Level(line, fs.len()));

        if !panel.folded.contains(&Fold::Level(line)) {
            rows.extend(fs.iter().map(|&i| PRow::File(i, false)));
        }
    }
}

fn panel_commit_rows(panel: &Panel, mut rows: Vec<PRow>, view: &View) -> Vec<PRow> {
    let cs = &view.changes.commits;

    rows.push(PRow::Section(Sec::Commits, cs.list.len()));

    if panel.folded.contains(&Fold::Sec(Sec::Commits)) {
        return rows;
    }

    // a branch: its changes against the base (commits + uncommitted) as 1 view
    if view.changes.base != "HEAD" && !view.changes.base.is_empty() {
        rows.push(PRow::All);
    }

    if cs.list.is_empty() {
        rows.push(PRow::Empty("0 commits on the branch"));
    }

    // a running push: its commits on their own, above UNPUSHED
    let going = |i: usize| !cs.list[i].pushed && view.pushing.contains(&cs.list[i].oid);
    let gone: Vec<usize> = (0..cs.list.len()).filter(|&i| going(i)).collect();

    if !gone.is_empty() || view.push_by.is_some() {
        rows.push(PRow::Pushing(gone.len()));
        rows.extend(gone.into_iter().map(PRow::PushingCommit));
    }

    for pushed in [false, true] {
        let at: Vec<usize> = (0..cs.list.len())
            .filter(|&i| cs.list[i].pushed == pushed && !going(i))
            .collect();

        if at.is_empty() {
            continue;
        }

        rows.push(PRow::Push(pushed, at.len()));

        if panel.folded.contains(&Fold::Push(pushed)) {
            continue;
        }

        // the shown commit: its files under it
        panel_commit_files(&mut rows, view, cs, at);
    }

    if cs.more {
        rows.push(PRow::Empty("older commits: not listed"));
    }

    rows
}

fn panel_commit_files(
    rows: &mut Vec<PRow>,
    view: &View,
    cs: &crate::domain::changes::commits::Commits,
    at: Vec<usize>,
) {
    for i in at {
        rows.push(PRow::Commit(i));

        let Some(peek) = view.peek.as_ref().filter(|x| {
            x.commit_src
                .as_ref()
                .is_some_and(|character| character.oid == cs.list[i].oid)
        }) else {
            continue;
        };

        append_commit_empty(rows, peek);

        rows.extend((0..peek.changes.files.len()).map(|fi| PRow::CommitFile(i, fi)));
    }
}

fn append_commit_empty(rows: &mut Vec<PRow>, view: &View) {
    if view.changes.files.is_empty() {
        rows.push(PRow::Empty(if view.loaded.is_none() && view.error.is_none() {
            "reading the commit…"
        } else {
            "0 files"
        }));
    }
}

fn panel_comment_rows(panel: &Panel, rows: &mut Vec<PRow>, view: &View) {
    let (here, away) = view.comments_here();
    let open = here
        .iter()
        .filter(|(i, _)| view.comment_state.comments[*i].open())
        .count();

    rows.push(PRow::Section(Sec::Comments, open));

    if !panel.folded.contains(&Fold::Sec(Sec::Comments)) {
        if here.is_empty() && away.is_empty() {
            rows.push(PRow::Empty("c on lines: a comment"));
        }

        // OPEN (per severity, as the changes' levels: its files, their comments), RESOLVED
        // (its files, their comments)
        panel_comment_states(panel, rows, view, &here);

        rows.extend(
            away.into_iter()
                .map(|(root, count_value)| PRow::Elsewhere(root, count_value)),
        );
    }
}

fn panel_comment_states(panel: &Panel, rows: &mut Vec<PRow>, view: &View, here: &[(usize, String)]) {
    for resolved in [false, true] {
        let at: Vec<(usize, &str)> = here
            .iter()
            .filter(|(i, _)| view.comment_state.comments[*i].open() != resolved)
            .map(|(i, path_3)| (*i, path_3.as_str()))
            .collect();

        if at.is_empty() {
            continue;
        }

        rows.push(PRow::CState(resolved, at.len()));

        if panel.folded.contains(&Fold::CState(resolved)) {
            continue;
        }

        if resolved {
            append_comment_files(rows, &at);
            continue;
        }

        panel_comment_files(panel, rows, view, &at);
    }
}

fn panel_comment_files(panel: &Panel, rows: &mut Vec<PRow>, view: &View, at: &[(usize, &str)]) {
    for line_value in Level::ALL {
        let of: Vec<(usize, &str)> = at
            .iter()
            .copied()
            .filter(|(i, _)| view.comment_state.comments[*i].level == line_value)
            .collect();

        if of.is_empty() {
            continue;
        }

        rows.push(PRow::CLevel(line_value, of.len()));

        if !panel.folded.contains(&Fold::CLevel(line_value)) {
            append_comment_files(rows, &of);
        }
    }
}

fn append_comment_files(rows: &mut Vec<PRow>, at: &[(usize, &str)]) {
    let mut paths: Vec<&str> = Vec::new();

    for &(_, path) in at {
        if !paths.contains(&path) {
            paths.push(path);
        }
    }

    for path in paths {
        let of: Vec<usize> = at
            .iter()
            .filter(|(_, path_2)| *path_2 == path)
            .map(|(i, _)| *i)
            .collect();

        rows.push(PRow::CommentFile(path.to_string(), of.len()));
        rows.extend(of.into_iter().map(PRow::Comment));
    }
}

fn comments_here_view(view: &View) -> (LocalComments, CheckoutCounts) {
    let mut here = Vec::new();
    let mut away: Vec<(PathBuf, usize)> = Vec::new();

    for (i, comment) in view.comment_state.comments.iter().enumerate() {
        append_checkout_comment(view, &mut here, &mut away, i, comment);
    }

    (here, away)
}

fn append_checkout_comment(
    view: &View,
    here: &mut Vec<(usize, String)>,
    away: &mut Vec<(PathBuf, usize)>,
    i: usize,
    comment: &crate::domain::changes::Comment,
) {
    match crate::features::changes::place(comment) {
        (Some(result), _) if !crate::features::changes::same_dir(&result, &view.dir) => {
            if !comment.open() {
                return;
            }

            match away.iter_mut().find(|(agent, _)| *agent == result) {
                Some((_, count)) => *count += 1,
                None => away.push((result, 1)),
            }
        }
        (_, rel) => here.push((i, rel)),
    }
}

fn panel_rows_view(view: &View) -> Vec<PRow> {
    let path_value = &view.scm;
    let mut order: Vec<usize> = (0..view.changes.files.len()).collect();

    order.sort_by_key(|&i| view.level_of(i) as usize);
    let mut value = Vec::new();
    let pr = view.pr_src.is_some();

    if pr {
        value.push(PRow::Pick);
    }

    panel_file_rows(path_value, &order, &mut value, pr, view);

    // the comments of this checkout (their path in it); the open ones of other checkouts:
    // 1 row per checkout
    panel_comment_rows(path_value, &mut value, view);

    if pr {
        return value;
    }

    value.push(PRow::Section(Sec::Stash, view.stash_state.stash.len()));

    if !path_value.folded.contains(&Fold::Sec(Sec::Stash)) {
        if view.stash_state.stash.is_empty() {
            value.push(PRow::Empty("p on lines / files: stash them"));
        }

        value.extend((0..view.stash_state.stash.len()).map(PRow::Stash));
    }

    // the branch's commits: unpushed, then pushed (newest first in each)
    panel_commit_rows(path_value, value, view)
}
