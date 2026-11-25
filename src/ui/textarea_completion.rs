use super::{Op, Pos, Position, TextArea, is_word};

impl TextArea {
    /// `@query` right before the cursor (the `@` at a line start or after a space / bracket, 0 selection):
    /// the `@` position + the query. For the agent dropdown.
    pub(crate) fn mention(&self) -> Option<(Pos, String)> {
        self.trigger('@', |character| is_word(character) || matches!(character, '-' | '.'))
    }

    /// `>query` right before the cursor (0 spaces in the query): the `>` position + the query.
    /// For the command dropdown.
    pub(crate) fn command(&self) -> Option<(Pos, String)> {
        let key = crate::infra::config::command_key();

        self.trigger(key, |character| !character.is_whitespace() && character != key)
    }

    /// A path being typed: the word that ends at the cursor (0 spaces, quotes, brackets in it; the
    /// next char: none or a space / closer), if it holds a `/`. Its start + the word. Words of the
    /// `@` and `>` dropdowns: not paths. For the path dropdown.
    #[cfg(test)]
    pub(crate) fn path_word(&self) -> Option<(Pos, String)> {
        self.path_word_quoted().map(|(start, word, _)| (start, word))
    }

    /// [`path_word`], with spaces after an open `"` (`"~/My Notes/a`): the word starts after
    /// the quote. The 3rd value: inside such a quote.
    pub(crate) fn path_word_quoted(&self) -> Option<(Pos, String, bool)> {
        if self.range().is_some() {
            return None;
        }

        let line = &self.lines[self.cur.row];
        let before = &line[..self.cur.col];

        if before.iter().filter(|&&ch| ch == '"').count() % 2 == 1
            && let Some(quote) = before.iter().rposition(|&ch| ch == '"')
            && line.get(self.cur.col).is_none_or(|&ch| ch.is_whitespace() || ch == '"')
        {
            let word: String = line[quote + 1..self.cur.col].iter().collect();

            return word.contains('/').then_some((
                Pos {
                    row: self.cur.row,
                    col: quote + 1,
                },
                word,
                true,
            ));
        }

        let edge =
            |ch: char| ch.is_whitespace() || matches!(ch, '(' | ')' | '[' | ']' | '"' | '\'' | '`' | '<' | '>' | ',');

        if line.get(self.cur.col).is_some_and(|&ch| !edge(ch)) {
            return None;
        }

        let mut start = self.cur.col;

        while start > 0 && !edge(line[start - 1]) {
            start -= 1;
        }

        let word: String = line[start..self.cur.col].iter().collect();

        if !word.contains('/')
            || word.starts_with('@')
            || (start > 0 && line[start - 1] == crate::infra::config::command_key())
        {
            return None;
        }

        Some((
            Pos {
                row: self.cur.row,
                col: start,
            },
            word,
            false,
        ))
    }

    /// MARK + query (chars that pass QUERY) right before the cursor; MARK at a line start or after
    /// a space / bracket, 0 selection. The MARK position + the query.
    pub(super) fn trigger(&self, mark: char, query: impl Fn(char) -> bool) -> Option<(Pos, String)> {
        if self.range().is_some() {
            return None;
        }

        let line = &self.lines[self.cur.row];
        let mut start = self.cur.col;

        while start > 0 && line[start - 1] != mark && query(line[start - 1]) {
            start -= 1;
        }

        if start == 0 || line[start - 1] != mark {
            return None;
        }

        let at = start - 1;

        if at > 0 && !(line[at - 1].is_whitespace() || matches!(line[at - 1], '(' | '[' | '"' | '\'')) {
            return None;
        }

        Some((
            Pos {
                row: self.cur.row,
                col: at,
            },
            line[start..self.cur.col].iter().collect(),
        ))
    }

    /// The text from FROM to the cursor (same row, 0 selection). None: another row, after it.
    pub(crate) fn since(&self, from: Pos) -> Option<String> {
        if self.range().is_some() || from.row != self.cur.row || from.col > self.cur.col {
            return None;
        }

        Some(self.lines[self.cur.row][from.col..self.cur.col].iter().collect())
    }

    /// `#query` right before the cursor: the `#` position + the query (a shortcut name). For
    /// the shortcut dropdown.
    pub(crate) fn shortcut(&self) -> Option<(Pos, String)> {
        self.trigger('#', |character| is_word(character) || character == '-')
    }

    /// The `#name` token at screen cell AT (None: at the cursor): its `#` position + the name.
    /// For the shortcut popup.
    pub(crate) fn shortcut_at(&self, at: Option<Position>) -> Option<(Pos, String)> {
        locate_shortcut(self, at)
    }

    /// Replace FROM..cursor (same row) with TEXT (1 undo step).
    pub(crate) fn replace_to_cursor(&mut self, from: Pos, text: &str) {
        if from.row != self.cur.row || from.col > self.cur.col {
            return;
        }

        self.snapshot(Op::Other);
        self.anchor = None;
        self.delete_range(from, self.cur);
        self.insert_raw(text);
        self.follow = true;
        self.want_x = None;
    }

    /// Screen cell of POS (last render). None: scrolled out.
    pub(crate) fn screen_pos(&self, pos: Pos) -> Option<Position> {
        if self.vis.is_empty() {
            return Some(Position {
                x: self.area.x,
                y: self.area.y,
            });
        }

        let (vrow, x) = self.visual_of(pos);

        if vrow < self.scroll || vrow >= self.scroll + self.area.height as usize {
            return None;
        }

        Some(Position {
            x: self.area.x + x as u16,
            y: self.area.y + (vrow - self.scroll) as u16,
        })
    }
}

fn locate_shortcut(ta: &TextArea, at: Option<Position>) -> Option<(Pos, String)> {
    let pos = match at {
        Some(cell) => {
            if !ta.area.contains(cell) {
                return None;
            }

            let y = usize::from(cell.y - ta.area.y);

            ta.pos_at(ta.scroll + y, usize::from(cell.x - ta.area.x))
        }
        None => ta.cur,
    };

    let line = ta.lines.get(pos.row)?;
    let name = |character: char| is_word(character) || character == '-';

    // back to the `#` (the cursor right after the name counts too)
    let mut start = pos.col.min(line.len());

    while start > 0 && line[start - 1] != '#' && name(line[start - 1]) {
        start -= 1;
    }

    let hash = if line.get(start) == Some(&'#') {
        start
    } else if start > 0 && line[start - 1] == '#' {
        start - 1
    } else {
        return None;
    };

    if hash > 0 && !(line[hash - 1].is_whitespace() || matches!(line[hash - 1], '(' | '[' | '"' | '\'')) {
        return None;
    }

    let word: String = line[hash + 1..].iter().take_while(|ch| name(**ch)).collect();

    (!word.is_empty() && pos.col <= hash + 1 + word.chars().count()).then_some((
        Pos {
            row: pos.row,
            col: hash,
        },
        word,
    ))
}
