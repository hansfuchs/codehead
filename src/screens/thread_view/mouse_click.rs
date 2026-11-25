use super::{Duration, HashSet, Hit, Instant, Pane, Path, Position, View, open_url, word_at};
impl View {
    pub(super) fn mouse_row(&mut self, at: Position) {
        let Some(row) = self.row_at(at.y) else {
            self.selection.sel = None;
            return;
        };

        let col = at.x - self.layout.stream_rect.x;

        if mouse_link(self, row, col) || mouse_copy(self, row, col) || mouse_hit(self, row) {
            return;
        }

        mouse_select(self, row, col);
    }
}
fn toggle_fold(folds: &mut HashSet<usize>, key: usize) {
    if !folds.remove(&key) {
        folds.insert(key);
    }
}

fn mouse_link(view: &mut View, row: usize, col: u16) -> bool {
    let drawn_row = &view.stream.drawn.rows[row];

    if let Some(url) = drawn_row
        .links
        .iter()
        .find(|(start, end, _)| col >= *start && col < *end)
        .map(|(_, _, url)| url.clone())
    {
        // a file of a message: the files view on it
        if let Some(file) = url.strip_prefix("file://") {
            let path = file.split("#L").next().unwrap_or(file);

            view.panes.files.show(Path::new(path));
            view.set_pane(Pane::Files);
            return true;
        }

        open_url(&url);
        view.toast(format!("open {url}"));
        return true;
    }

    false
}

fn mouse_copy(view: &mut View, row: usize, col: u16) -> bool {
    let drawn_row = &view.stream.drawn.rows[row];

    if let Some(text) = drawn_row
        .copies
        .iter()
        .find(|(start, end, _)| col >= *start && col < *end)
        .map(|(_, _, text)| text.clone())
    {
        view.copy(&text);
        return true;
    }

    false
}

fn mouse_hit(view: &mut View, row: usize) -> bool {
    match view.stream.drawn.hits.get(&row).cloned() {
        Some(Hit::Thought(key)) => {
            toggle_fold(&mut view.stream.open_thoughts, key);

            view.stream.dirty = true;
            return true;
        }
        Some(Hit::Run(key)) => {
            toggle_fold(&mut view.stream.open, key);

            view.stream.dirty = true;
            return true;
        }
        Some(Hit::PickUp(i)) => {
            view.pick_up(i);
            return true;
        }
        Some(Hit::Choice(i)) => {
            mouse_choice(view, i);
            return true;
        }
        None => {}
    }

    false
}

fn mouse_choice(view: &mut View, i: usize) {
    if let Some(ask) = view.thread.asks.first().cloned()
        && !view.ask.sent
    {
        let act = view.ask.click(&ask, i);

        view.ask_act(act);
    }
}

fn mouse_select(view: &mut View, row: usize, col: u16) {
    let count = match view.selection.clicks {
        Some((at, cell, clicks)) if at.elapsed() < Duration::from_millis(400) && cell == (row, col) => clicks % 3 + 1,
        _ => 1,
    };

    view.selection.clicks = Some((Instant::now(), (row, col), count));
    let text: String = view.stream.drawn.rows[row]
        .line
        .spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect();

    // selection: 1 click starts it, 2 select the word, 3 the row
    view.selection.sel = match count {
        2 => {
            let (start, end) = word_at(&text, col);

            Some(((row, start), (row, end)))
        }
        3 => Some(((row, 0), (row, text.chars().count().saturating_sub(1) as u16))),
        _ => None,
    };

    if let Some(selected) = view.selection.sel.and_then(|_| view.selected()) {
        view.copy(&selected);
    }

    view.selection.dragging = count == 1;
    view.selection.drag_from = Some((row, col));
}
