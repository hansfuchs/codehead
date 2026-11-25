use super::{Entry, Row, Sec, Source, View};
use std::io::Read;

impl View {
    /// The file under the cursor.
    pub(super) fn cur(&self) -> Option<&Entry> {
        self.visible.get(self.at).and_then(|(i, _)| self.files.get(*i))
    }
    /// Keys go to the filter input.
    pub(crate) fn typing(&self) -> bool {
        self.filter_input.filtering
    }
    /// The list again: sections in order, newest first in each; a filter: best match first.
    pub(super) fn rebuild(&mut self) {
        rebuild_view(self);
    }
    /// The list rows: per section a heading, its files, a blank row between sections; a filter:
    /// the matches, best first, 0 headings.
    pub(super) fn rows(&self) -> Vec<Row> {
        if !self.filter_input.filter.text().trim().is_empty() {
            return (0..self.visible.len()).map(Row::File).collect();
        }

        let mut out = Vec::new();
        let mut last = None;

        for (key, (i, _)) in self.visible.iter().enumerate() {
            rows_text_value(&mut out, &mut last, key, *i, self);
        }

        out
    }

    pub(super) fn step(&mut self, directory: isize) {
        let count = self.visible.len();

        if count == 0 {
            return;
        }

        self.at = (self.at as isize + directory).clamp(0, count as isize - 1) as usize;
        self.text_preview.scroll = 0;
    }
}

/// The section of E.
fn sec(error: &Entry, view_state: &View) -> Sec {
    match error.source {
        Source::Note => Sec::Notes,
        Source::Named => Sec::Named,
        Source::Attached(_) => Sec::Attached,
        Source::Agent if error.path.starts_with(&view_state.dir) => Sec::Repo,
        Source::Agent if crate::infra::config::home_repo().is_some_and(|height| error.path.starts_with(height)) => {
            Sec::Home
        }
        Source::Agent => Sec::Other,
    }
}

/// The text of file I for the filter (cached per mtime).
fn text_of(i: usize, view_state: &mut View) -> String {
    let error = &view_state.files[i];
    let key = (error.path.clone(), error.at);

    if let Some(thread) = view_state.text_preview.texts.get(&key) {
        return thread.clone();
    }

    let mut buf = Vec::new();

    if let Ok(file) = std::fs::File::open(&error.path) {
        let _ = file.take(256 << 10).read_to_end(&mut buf);
    }

    // binary files: 0 text
    let thread_value = if buf.contains(&0) {
        String::new()
    } else {
        String::from_utf8_lossy(&buf).into_owned()
    };

    if view_state.text_preview.texts.len() > 512 {
        view_state.text_preview.texts.clear();
    }

    view_state.text_preview.texts.insert(key, thread_value.clone());

    thread_value
}

fn rows_text_value(out: &mut Vec<Row>, last: &mut Option<Sec>, key: usize, i: usize, view_state: &View) {
    let text_value = sec(&view_state.files[i], view_state);

    if *last != Some(text_value) {
        if last.is_some() {
            out.push(Row::Gap);
        }

        out.push(Row::Head(text_value));
        *last = Some(text_value);
    }

    out.push(Row::File(key));
}

fn rebuild_view(state: &mut View) {
    let cur = state.cur().map(|error| error.path.clone());
    let mut order: Vec<usize> = (0..state.files.len()).collect();

    order.sort_by_key(|&i| (sec(&state.files[i], state), std::cmp::Reverse(state.files[i].at)));
    let query = state.filter_input.filter.text();

    state.visible = if query.trim().is_empty() {
        order.into_iter().map(|i| (i, Vec::new())).collect()
    } else {
        let names: Vec<String> = order.iter().map(|&i| state.label(&state.files[i])).collect();
        let bodies: Vec<String> = order.iter().map(|&i| text_of(i, state)).collect();
        let idx: Vec<usize> = (0..order.len()).collect();

        crate::ui::fuzzy::rank(
            &idx,
            &query,
            |&key| names[key].clone(),
            |&key_value| bodies[key_value].clone(),
        )
        .into_iter()
        .map(|(key_2, marks)| (order[key_2], marks))
        .collect()
    };

    state.at = cur
        .and_then(|path_value| {
            state
                .visible
                .iter()
                .position(|(i, _)| state.files[*i].path == path_value)
        })
        .unwrap_or(0);
}
