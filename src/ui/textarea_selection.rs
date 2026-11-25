use super::{Pos, TextArea, is_word};

impl TextArea {
    pub(super) fn range(&self) -> Option<(Pos, Pos)> {
        let agent = self.anchor?;

        if agent == self.cur {
            return None;
        }

        Some(if agent < self.cur {
            (agent, self.cur)
        } else {
            (self.cur, agent)
        })
    }

    pub(crate) fn selected_text(&self) -> Option<String> {
        collect_selected_text(self)
    }

    pub(super) fn delete_range(&mut self, mut agent: Pos, mut bytes: Pos) {
        if let Some((start, _)) = self
            .atomic_ranges(agent.row)
            .into_iter()
            .find(|&(start, end)| agent.col > start && agent.col < end)
        {
            agent.col = start;
        }

        if let Some((_, end)) = self
            .atomic_ranges(bytes.row)
            .into_iter()
            .find(|&(start, end)| bytes.col > start && bytes.col < end)
        {
            bytes.col = end;
        }

        let tail: Vec<char> = self.lines[bytes.row][bytes.col..].to_vec();

        self.lines[agent.row].truncate(agent.col);
        self.lines[agent.row].extend(tail);

        if bytes.row > agent.row {
            self.lines.drain(agent.row + 1..=bytes.row);
        }

        self.cur = agent;
        self.anchor = None;
    }

    pub(super) fn delete_selection(&mut self) -> bool {
        if let Some((agent, bytes)) = self.range() {
            self.delete_range(agent, bytes);
            true
        } else {
            self.anchor = None;
            false
        }
    }

    pub(super) fn select_word(&mut self, path: Pos) {
        select_word_at(self, path);
    }

    pub(crate) fn select_all(&mut self) {
        self.anchor = Some(Pos::default());
        let result = self.lines.len() - 1;

        self.cur = Pos {
            row: result,
            col: self.lines[result].len(),
        };
    }
}

fn collect_selected_text(widget_state: &TextArea) -> Option<String> {
    let (agent, bytes) = widget_state.range()?;
    let mut out = String::new();

    for row_value in agent.row..=bytes.row {
        let line = &widget_state.lines[row_value];
        let text = if row_value == agent.row { agent.col } else { 0 };
        let error = if row_value == bytes.row { bytes.col } else { line.len() };

        out.extend(&line[text..error]);

        if row_value != bytes.row {
            out.push('\n');
        }
    }

    Some(out)
}

fn select_word_at(widget_state: &mut TextArea, path: Pos) {
    let line = &widget_state.lines[path.row];
    let (mut agent, mut bytes) = (path.col, path.col);

    let class = |character: char| {
        if is_word(character) {
            0
        } else if character == ' ' {
            1
        } else {
            2
        }
    };

    let key = line
        .get(path.col)
        .or(path.col.checked_sub(1).and_then(|i| line.get(i)))
        .map(|&character_value| class(character_value));

    if let Some(key_value) = key {
        while agent > 0 && class(line[agent - 1]) == key_value {
            agent -= 1;
        }

        while bytes < line.len() && class(line[bytes]) == key_value {
            bytes += 1;
        }
    }

    widget_state.anchor = Some(Pos {
        row: path.row,
        col: agent,
    });
    widget_state.cur = Pos {
        row: path.row,
        col: bytes,
    };
}
