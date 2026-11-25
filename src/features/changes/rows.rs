use super::{Body, Cat, FileDiff, HashMap, Kind, Painted, Row, Status, View, body_text, gh_lines, paint};

impl View {
    pub(super) fn build(&mut self, width: u16) {
        build_view(self, width);
    }

    /// The painted lines of hunk H of F (made once per diff of the file).
    pub(super) fn painted(&self, file: &FileDiff, height: usize) -> std::rc::Rc<Vec<Painted>> {
        let key = (file.hash, height);

        if let Some(path) = self.layout.paints.borrow().get(&key) {
            return path.clone();
        }

        let path = std::rc::Rc::new(file.hunks.get(height).map(|x| paint(file, x)).unwrap_or_default());

        self.layout.paints.borrow_mut().insert(key, path.clone());
        path
    }
}

fn build_file_rows(side: bool, shown: &[usize], view: &mut View) {
    let mut rows: Vec<Row> = Vec::new();

    if view.changes.stopped.is_some() {
        rows.push(Row::new(Body::Conflict, 0, None));
    }

    if !view.filter_state.filter.trim().is_empty() {
        rows.push(Row::new(Body::Hits(shown.len()), 0, None));
    }

    // group heads: the model's groups (title, why), else the path-rule categories; 0 heads
    // for filter hits (in relevance order)
    let mut last: Option<(Option<usize>, Cat)> = None;
    let mut num_w = HashMap::new();

    for &i in shown {
        let file = &view.changes.files[i];

        append_group_rows(view, &mut rows, &mut last, i, file);

        if !view.folded(file) {
            num_w.insert(
                i,
                file.hunks
                    .iter()
                    .flat_map(|height| &height.lines)
                    .filter_map(|line| line.new.max(line.old))
                    .max()
                    .unwrap_or(1)
                    .to_string()
                    .len()
                    .max(3),
            );
        }

        file_rows(&mut rows, i, file, side, view);
    }

    view.layout.num_w = num_w;
    view.layout.rows = rows;
}

fn append_group_rows(
    view: &View,
    rows: &mut Vec<Row>,
    last: &mut Option<(Option<usize>, Cat)>,
    i: usize,
    file: &FileDiff,
) {
    if view.filter_state.filter.trim().is_empty() {
        let group = view.ranked(&file.path).map(|x| x.0);

        let new_group = match (*last, group) {
            (None, _) => true,
            (Some((pg, _)), Some(gi)) => pg != Some(gi),
            (Some((pg, pc)), None) => pg.is_some() || pc != file.cat,
        };

        if new_group {
            rows.push(Row::new(Body::Blank, i, None));
            rows.push(Row::new(Body::Group(group, file.cat), i, None));
        }

        *last = Some((group, file.cat));
    }
}

/// The rows of file I (F): its head, then (unfolded) the why, the hunks, findings and
/// comments under their lines, the foot.
fn file_rows(rows: &mut Vec<Row>, i: usize, file: &FileDiff, side: bool, view: &View) {
    let first = file.first_line();

    rows.push(Row::new(Body::Head, i, Some(first)));

    if view.folded(file) {
        return;
    }

    if view.ranked(&file.path).is_some_and(|x| !x.2.is_empty()) {
        rows.push(Row::new(Body::Why, i, Some(first)));
    }

    if file.binary || file.hunks.is_empty() {
        rows.push(Row::new(Body::Note(file_note(file)), i, Some(first)));
    }

    // a new or deleted file: 1 side only, unified uses the room
    let side = side && !matches!(file.status, Status::New | Status::Deleted);

    for (hi, hunk) in file.hunks.iter().enumerate() {
        rows.push(Row::new(Body::Hunk(hi), i, Some(hunk.new.max(1))));
        let ls = &hunk.lines;

        if !side {
            rows.extend(
                ls.iter()
                    .enumerate()
                    .map(|(key, line)| Row::new(Body::Diff(hi, key), i, line.new.or(line.old))),
            );
            continue;
        }

        // side by side: a run of removed lines pairs with the added lines after it
        let key = 0;

        side_rows(rows, i, hi, ls, key);
    }

    // findings, then review comments: under the 1st diff row of their line, else at the box's end
    append_file_notes(rows, i, file, view);

    rows.push(Row::new(Body::Foot, i, None));
}

fn side_rows(rows: &mut Vec<Row>, i: usize, hi: usize, lines: &[super::DLine], mut offset: usize) {
    while offset < lines.len() {
        if lines[offset].kind == Kind::Ctx {
            rows.push(Row::new(
                Body::Side(hi, Some(offset), Some(offset)),
                i,
                lines[offset].new,
            ));
            offset += 1;
            continue;
        }

        let removed = lines[offset..].iter().take_while(|line| line.kind == Kind::Del).count();
        let added = lines[offset + removed..]
            .iter()
            .take_while(|line| line.kind == Kind::Add)
            .count();

        for j in 0..removed.max(added) {
            let (old_index, new_index) = (
                (j < removed).then_some(offset + j),
                (j < added).then_some(offset + removed + j),
            );
            let at = new_index
                .and_then(|x| lines[x].new)
                .or_else(|| (added > 0).then(|| lines[offset + removed].new).flatten())
                .or_else(|| old_index.and_then(|x| lines[x].old));

            rows.push(Row::new(Body::Side(hi, old_index, new_index), i, at));
        }

        offset += removed + added;
    }
}

fn append_file_notes(rows: &mut Vec<Row>, i: usize, file: &FileDiff, view: &View) {
    let start = rows
        .iter()
        .rposition(|result| result.head && result.file == i)
        .unwrap_or(0);
    let mut notes: Vec<(u32, Vec<Row>)> = Vec::new();

    collect_file_notes(i, file, &mut notes, view);

    let added =
        |result: &Row| result.comment.is_some() || result.finding.is_some() || matches!(result.body, Body::Gh(..));

    for (line, new) in notes {
        // the last row of the 1st run on that line (a removed line, then the added one)
        // 0 why / note row (they carry the 1st changed line too)
        let hit = |result: &Row| {
            !result.head
                && !result.hunk
                && !matches!(result.body, Body::Why | Body::Note(_))
                && !added(result)
                && result.at == Some(line)
        };

        let at = rows[start..].iter().position(hit).map_or(rows.len(), |path| {
            let first = start + path;

            first + 1 + rows[first + 1..].iter().take_while(|result| hit(result)).count()
        });

        // after the notes already under that row
        let at = at + rows[at..].iter().take_while(|result| added(result)).count();

        rows.splice(at..at, new);
    }
}

fn collect_file_notes(i: usize, file: &FileDiff, rows: &mut Vec<(u32, Vec<Row>)>, view: &View) {
    for (fi, x) in view
        .reviewer
        .review
        .iter()
        .flat_map(|result| result.findings.iter().enumerate())
        .filter(|(_, x)| x.file == file.path)
    {
        let count = 1 + x.body.lines().take(3).count();

        rows.push((
            x.line,
            (0..count)
                .map(|key| Row::new(Body::Finding(fi, key), i, Some(x.line)))
                .collect(),
        ));
    }

    // open comments only (resolved: the panel's RESOLVED); C folded: 1 row each
    let here: Vec<usize> = view
        .comment_state
        .comments
        .iter()
        .enumerate()
        .filter(|(ci, comment)| comment.open() && view.comment_path(*ci).is_some_and(|path| path == file.path))
        .map(|(ci, _)| ci)
        .collect();

    for (ci, comment) in here.into_iter().map(|ci| (ci, &view.comment_state.comments[ci])) {
        let count = view.clines(comment).len();

        rows.push((
            comment.line,
            (0..count)
                .map(|comment_value| Row::new(Body::Comment(ci, comment_value), i, Some(comment.line)))
                .collect(),
        ));
    }

    // PR mode: the GitHub threads on the file's lines
    for (ti, thread) in view
        .pr_src
        .iter()
        .flat_map(|text| text.threads.iter().enumerate())
        .filter(|(_, thread)| thread.path.as_deref() == Some(file.path.as_str()))
    {
        let count = gh_lines(thread, usize::MAX).len();
        let line = thread.line.unwrap_or(0);

        rows.push((
            line,
            (0..count)
                .map(|key| Row::new(Body::Gh(ti, key), i, Some(line)))
                .collect(),
        ));
    }
}

fn file_note(file: &FileDiff) -> &'static str {
    if file.binary {
        "binary file"
    } else {
        "0 line changes (mode or rename)"
    }
}

fn build_view(view: &mut View, width: u16) {
    let side = view.side_by_side(width);

    if view.layout.built == Some((width, side)) {
        return;
    }

    view.layout.built_w = width;
    view.classifier.fresh_rank = view
        .classifier
        .rank
        .as_ref()
        .is_some_and(|result| result.key == crate::domain::changes::rank::key(&view.changes.files));

    // reviewed / all per level
    let levels = view.levels();
    let mut counts = [(0, 0); 4];

    for (file, line) in view.changes.files.iter().zip(&levels) {
        counts[*line as usize].1 += 1;
        counts[*line as usize].0 += usize::from(view.is_reviewed(file));
    }

    view.classifier.counts = counts;

    // the shown files: the filter's hits (by relevance), else all by level (the panel's order)
    let shown: Vec<usize> = if view.filter_state.filter.trim().is_empty() {
        view.filter_state.hits.clear();
        let mut value: Vec<usize> = (0..view.changes.files.len()).collect();

        value.sort_by_key(|&i| levels[i] as usize);
        value
    } else {
        let hits = crate::ui::fuzzy::rank(
            &view.changes.files,
            &view.filter_state.filter,
            |file| file.path.clone(),
            body_text,
        );

        view.filter_state.hits = hits.iter().cloned().collect();
        hits.into_iter().map(|height| height.0).collect()
    };

    view.classifier.lv = levels;
    build_file_rows(side, &shown, view);
    view.layout.built = Some((width, side));

    // the cursor back on its file: the same row of it (a new diff: up to its last row)
    if let Some((path, off)) = view.pending_cursor.take()
        && let Some(i) = view.changes.files.iter().position(|file| file.path == path)
        && let Some(row) = view
            .layout
            .rows
            .iter()
            .position(|result| result.head && result.file == i)
    {
        let count = view.layout.rows[row..].iter().take_while(|x| x.file == i).count();

        view.at = row + off.min(count.saturating_sub(1));
    }

    view.at = view.at.min(view.layout.rows.len().saturating_sub(1));

    // a group head under the cursor: the 1st box
    if !view
        .layout
        .rows
        .get(view.at)
        .is_some_and(|result| result.head || result.at.is_some())
        && let Some(height) = view.layout.rows.iter().position(|result| result.head)
    {
        view.at = height;
    }
}
