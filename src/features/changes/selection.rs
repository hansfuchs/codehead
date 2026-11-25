use super::{Act, Body, Comment, LKey, Op, Row, Spot, View, lines_label, lkey, patch, pick_of};
use std::fmt::Write as _;

impl View {
    /// The selection keys of row R: its diff lines.
    pub(super) fn keys(&self, result: &Row) -> Vec<LKey> {
        keys_view(self, result)
    }

    /// Row I has a selected line.
    pub(super) fn selected(&self, i: usize) -> bool {
        let Some(row) = self.layout.rows.get(i) else {
            return false;
        };

        let Some(set) = self
            .changes
            .files
            .get(row.file)
            .and_then(|file| self.selection_state.sel.get(&file.path))
        else {
            return false;
        };

        self.keys(row).iter().any(|key| set.contains(key))
    }

    /// Select (ON) or unselect the lines of row I.
    pub(super) fn select_row(&mut self, i: usize, on: bool) {
        select_row_view(self, i, on);
    }

    pub(super) fn spots(&self) -> Vec<Spot> {
        spots_view(self)
    }

    /// The selected lines per file as patch picks: (path, lines label, pick); 0 changed line
    /// picked (context only): 0 entry.
    pub(super) fn picks(&self) -> Vec<(String, String, patch::Pick)> {
        let mut paths: Vec<&String> = self.selection_state.sel.keys().collect();

        paths.sort();

        paths
            .into_iter()
            .filter_map(|path| {
                let file = self.changes.files.iter().find(|file| &file.path == path)?;
                let set = &self.selection_state.sel[path];
                let pk = pick_of(file, set);

                (!pk.is_empty()).then(|| (path.clone(), lines_label(set), pk))
            })
            .collect()
    }

    /// Stash ITEMS: out of the worktree, into the stash (on a thread).
    pub(super) fn stash_op(&mut self, items: Vec<(String, String, patch::Pick)>) -> Act {
        let rev = if self.changes.rev.is_empty() {
            "HEAD".to_string()
        } else {
            self.changes.rev.clone()
        };

        self.start_op(Op::Stash {
            thread: self.id.clone(),
            rev,
            items,
        })
    }

    /// What the owner's message from the changes view takes along: the selected lines, the open
    /// comments. None: neither.
    pub(crate) fn context(&self) -> Option<String> {
        context_view(self)
    }
}

fn select_key(set: &mut std::collections::BTreeSet<LKey>, key: LKey, on: bool) {
    if on {
        set.insert(key);
    } else {
        set.remove(&key);
    }
}

fn append_visible_spot(spots: &mut Vec<Spot>, row: &Row, view: &View) {
    let Some(file) = view.changes.files.get(row.file) else {
        return;
    };

    let line = row.at.unwrap_or_else(|| file.first_line());

    match spots.iter_mut().find(|spot| spot.path == file.path) {
        Some(spot) => spot.line = line,
        None => spots.push(Spot {
            path: file.path.clone(),
            line,
            lines: view
                .selection_state
                .sel
                .get(&file.path)
                .map(lines_label)
                .unwrap_or_default(),
        }),
    }
}

fn append_hidden_spot(spots: &mut Vec<Spot>, path: &str, selected: &std::collections::BTreeSet<LKey>) {
    if spots.iter().any(|spot| spot.path == path) {
        return;
    }

    let Some(key) = selected.iter().find(|key| !key.0).or(selected.iter().next()) else {
        return;
    };

    spots.push(Spot {
        path: path.to_string(),
        line: key.1,
        lines: lines_label(selected),
    });
}

fn keys_view(view: &View, result: &Row) -> Vec<LKey> {
    let Some(file) = view.changes.files.get(result.file) else {
        return Vec::new();
    };

    let line = |height: usize, key: usize| file.hunks.get(height).and_then(|x| x.lines.get(key)).and_then(lkey);

    match result.body {
        Body::Diff(height, key) => line(height, key).into_iter().collect(),
        Body::Side(height, agent, bytes) if agent == bytes => {
            agent.and_then(|key| line(height, key)).into_iter().collect()
        }
        Body::Side(height, agent, bytes) => agent
            .and_then(|key| line(height, key))
            .into_iter()
            .chain(bytes.and_then(|key| line(height, key)))
            .collect(),
        _ => Vec::new(),
    }
}

fn select_row_view(view: &mut View, i: usize, on: bool) {
    let Some(row) = view.layout.rows.get(i) else { return };
    let keys = view.keys(row);

    let Some(path) = view.changes.files.get(row.file).map(|file| file.path.clone()) else {
        return;
    };

    if keys.is_empty() {
        return;
    }

    let set = view.selection_state.sel.entry(path.clone()).or_default();

    for key in keys {
        select_key(set, key, on);
    }

    if set.is_empty() {
        view.selection_state.sel.remove(&path);
    }
}

fn spots_view(view: &View) -> Vec<Spot> {
    let mut spots = Vec::new();

    for (_, row) in view
        .layout
        .rows
        .iter()
        .enumerate()
        .filter(|(index, _)| view.selected(*index))
    {
        append_visible_spot(&mut spots, row, view);
    }

    for (path, selected) in &view.selection_state.sel {
        append_hidden_spot(&mut spots, path, selected);
    }

    spots
}

fn context_view(view: &View) -> Option<String> {
    let mut text = String::new();

    if !view.selection_state.sel.is_empty() {
        text.push_str("Selected lines (file:lines, new side; `removed`: old side):\n");
        let mut paths: Vec<&String> = view.selection_state.sel.keys().collect();

        paths.sort();

        for path in paths {
            let spot = Spot {
                path: path.clone(),
                line: 0,
                lines: lines_label(&view.selection_state.sel[path]),
            };

            let _ = write!(text, "- `{}`\n```diff\n{}\n```\n", spot.label(), view.quote(&spot));
        }
    }

    // only the comments the owner picked (space): 0 with each prompt; every comment: 1 batch
    // on ctrl+enter, or an agent reads them (MCP `comments`)
    let picked: Vec<&Comment> = view
        .comment_state
        .comments
        .iter()
        .filter(|comment| view.scm.comment_picks.contains(&comment.id))
        .collect();

    if !picked.is_empty() {
        text.push_str("Picked review comments (file:line, new side; `comment_status` resolves them):\n");

        append_picked_comments(&mut text, picked);
    }

    if let Some(path) = view.peek.as_ref()
        && let Some(more) = path.context()
    {
        text.push_str(more.trim_start_matches("[codehead: context from the changes view]\n"));
    }

    (!text.is_empty()).then(|| format!("[codehead: context from the changes view]\n{}", text.trim_end()))
}

fn append_picked_comments(text: &mut String, picked: Vec<&Comment>) {
    for character in picked {
        let state = if character.open() { "open" } else { "resolved" };

        let _ = writeln!(
            text,
            "- {} [{}, {state}] `{}:{}` by {}: {}",
            character.id,
            character.level.label(),
            character.path,
            character.at(),
            character.author(),
            character.text.replace('\n', " / ")
        );
    }
}
