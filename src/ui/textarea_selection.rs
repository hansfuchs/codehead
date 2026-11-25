use super::{Pos, TextArea, is_word};

impl TextArea {
    pub(super) fn range(&self) -> Option<(Pos, Pos)> {
        let anchor = self.anchor?;

        if anchor == self.cur {
            return None;
        }

        Some(if anchor < self.cur {
            (anchor, self.cur)
        } else {
            (self.cur, anchor)
        })
    }

    pub(crate) fn selected_text(&self) -> Option<String> {
        collect_selected_text(self)
    }

    pub(super) fn delete_range(&mut self, mut from: Pos, mut to: Pos) {
        if let Some((start, _)) = self
            .atomic_ranges(from.row)
            .into_iter()
            .find(|&(start, end)| from.col > start && from.col < end)
        {
            from.col = start;
        }

        if let Some((_, end)) = self
            .atomic_ranges(to.row)
            .into_iter()
            .find(|&(start, end)| to.col > start && to.col < end)
        {
            to.col = end;
        }

        let tail: Vec<char> = self.lines[to.row][to.col..].to_vec();

        self.lines[from.row].truncate(from.col);
        self.lines[from.row].extend(tail);

        if to.row > from.row {
            self.lines.drain(from.row + 1..=to.row);
        }

        self.cur = from;
        self.anchor = None;
    }

    pub(super) fn delete_selection(&mut self) -> bool {
        if let Some((from, to)) = self.range() {
            self.delete_range(from, to);
            true
        } else {
            self.anchor = None;
            false
        }
    }

    pub(super) fn select_word(&mut self, pos: Pos) {
        select_word_at(self, pos);
    }

    pub(crate) fn select_all(&mut self) {
        self.anchor = Some(Pos::default());
        let last = self.lines.len() - 1;

        self.cur = Pos {
            row: last,
            col: self.lines[last].len(),
        };
    }
}

fn collect_selected_text(ta: &TextArea) -> Option<String> {
    let (start, end) = ta.range()?;
    let mut out = String::new();

    for row in start.row..=end.row {
        let line = &ta.lines[row];
        let from = if row == start.row { start.col } else { 0 };
        let to = if row == end.row { end.col } else { line.len() };

        out.extend(&line[from..to]);

        if row != end.row {
            out.push('\n');
        }
    }

    Some(out)
}

fn select_word_at(ta: &mut TextArea, pos: Pos) {
    let line = &ta.lines[pos.row];
    let (mut start, mut end) = (pos.col, pos.col);

    let class = |character: char| {
        if is_word(character) {
            0
        } else if character == ' ' {
            1
        } else {
            2
        }
    };

    let class_at = line
        .get(pos.col)
        .or(pos.col.checked_sub(1).and_then(|i| line.get(i)))
        .map(|&ch| class(ch));

    if let Some(class_of) = class_at {
        while start > 0 && class(line[start - 1]) == class_of {
            start -= 1;
        }

        while end < line.len() && class(line[end]) == class_of {
            end += 1;
        }
    }

    ta.anchor = Some(Pos {
        row: pos.row,
        col: start,
    });
    ta.cur = Pos { row: pos.row, col: end };
}
