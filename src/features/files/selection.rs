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

        for (position, (i, _)) in self.visible.iter().enumerate() {
            push_file_row(&mut out, &mut last, position, *i, self);
        }

        out
    }

    pub(super) fn step(&mut self, delta: isize) {
        let count = self.visible.len();

        if count == 0 {
            return;
        }

        self.at = (self.at as isize + delta).clamp(0, count as isize - 1) as usize;
        self.text_preview.scroll = 0;
    }
}

/// The section of E.
fn sec(entry: &Entry, view: &View) -> Sec {
    match entry.source {
        Source::Note => Sec::Notes,
        Source::Named => Sec::Named,
        Source::Attached(_) => Sec::Attached,
        Source::Agent if entry.path.starts_with(&view.dir) => Sec::Repo,
        Source::Agent if crate::infra::config::home_repo().is_some_and(|home| entry.path.starts_with(home)) => {
            Sec::Home
        }
        Source::Agent => Sec::Other,
    }
}

/// The text of file I for the filter (cached per mtime).
fn text_of(i: usize, view: &mut View) -> String {
    let entry = &view.files[i];
    let key = (entry.path.clone(), entry.at);

    if let Some(text) = view.text_preview.texts.get(&key) {
        return text.clone();
    }

    let mut buf = Vec::new();

    if let Ok(file) = std::fs::File::open(&entry.path) {
        let _ = file.take(256 << 10).read_to_end(&mut buf);
    }

    // binary files: 0 text
    let text = if buf.contains(&0) {
        String::new()
    } else {
        String::from_utf8_lossy(&buf).into_owned()
    };

    if view.text_preview.texts.len() > 512 {
        view.text_preview.texts.clear();
    }

    view.text_preview.texts.insert(key, text.clone());

    text
}

fn push_file_row(out: &mut Vec<Row>, last: &mut Option<Sec>, position: usize, i: usize, view: &View) {
    let section = sec(&view.files[i], view);

    if *last != Some(section) {
        if last.is_some() {
            out.push(Row::Gap);
        }

        out.push(Row::Head(section));
        *last = Some(section);
    }

    out.push(Row::File(position));
}

fn rebuild_view(view: &mut View) {
    let cur = view.cur().map(|entry| entry.path.clone());
    let mut order: Vec<usize> = (0..view.files.len()).collect();

    order.sort_by_key(|&i| (sec(&view.files[i], view), std::cmp::Reverse(view.files[i].at)));
    let query = view.filter_input.filter.text();

    view.visible = if query.trim().is_empty() {
        order.into_iter().map(|i| (i, Vec::new())).collect()
    } else {
        let names: Vec<String> = order.iter().map(|&i| view.label(&view.files[i])).collect();
        let bodies: Vec<String> = order.iter().map(|&i| text_of(i, view)).collect();
        let idx: Vec<usize> = (0..order.len()).collect();

        crate::ui::fuzzy::rank(
            &idx,
            &query,
            |&position| names[position].clone(),
            |&position| bodies[position].clone(),
        )
        .into_iter()
        .map(|(position, marks)| (order[position], marks))
        .collect()
    };

    view.at = cur
        .and_then(|path| view.visible.iter().position(|(i, _)| view.files[*i].path == path))
        .unwrap_or(0);
}
