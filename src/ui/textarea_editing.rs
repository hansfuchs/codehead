use super::{BURST, Instant, KeyCode, Op, Pos, SAME_PRESS, TextArea, is_fence, list_item};

impl TextArea {
    pub(super) fn insert_raw(&mut self, text: &str) {
        let text = text.replace("\r\n", "\n").replace('\r', "\n").replace('\t', "    ");
        let text = if self.single { text.replace('\n', " ") } else { text };
        let mut parts = text.split('\n');
        let tail: Vec<char> = self.lines[self.cur.row].split_off(self.cur.col);

        if let Some(first) = parts.next() {
            let chars: Vec<char> = first.chars().filter(|character| !character.is_control()).collect();

            self.cur.col += chars.len();
            self.lines[self.cur.row].extend(chars);
        }

        for part in parts {
            let chars: Vec<char> = part
                .chars()
                .filter(|character_value| !character_value.is_control())
                .collect();

            self.cur.row += 1;
            self.cur.col = chars.len();
            self.lines.insert(self.cur.row, chars);
        }

        self.lines[self.cur.row].extend(tail);
    }

    pub(crate) fn insert(&mut self, text: &str) {
        let op = if text.chars().count() == 1 && !text.contains([' ', '\n']) {
            Op::Type
        } else {
            Op::Other
        };

        self.snapshot(op);
        self.delete_selection();
        self.insert_raw(text);
        self.follow = true;
        self.want_x = None;
    }

    pub(super) fn in_fence(&self, row: usize) -> bool {
        self.lines[..row].iter().filter(|line| is_fence(line)).count() % 2 == 1
    }

    /// A paste: code outside a fence goes in as a fenced block with its language tag (0 match: 0 tag).
    pub(crate) fn paste(&mut self, text: &str) {
        let text = text.replace("\r\n", "\n").replace('\r', "\n");

        if self.single || self.in_fence(self.cur.row) || !crate::ui::code::looks_code(&text) {
            return self.insert(&text);
        }

        let row = &self.lines[self.cur.row];
        let before = if self.cur.col > 0 { "\n" } else { "" };
        let after = if self.cur.col < row.len() { "\n" } else { "" };
        let tag = crate::ui::code::guess(&text).unwrap_or("");

        self.insert(&format!(
            "{before}```{tag}\n{}\n```{after}",
            text.trim_end_matches('\n')
        ));
    }

    /// Delete from the cursor to the start (END: the end) of its visual row. A selection: the selection.
    /// At the row's edge already: 1 char (joins the lines, like ⌘⌫ in macOS text fields).
    pub(super) fn kill_to_edge(&mut self, end: bool) {
        kill_row_edge(self, end);
    }

    /// Op of a backspace / delete: 1 char → Delete (runs coalesce), word / selection → Other.
    pub(super) fn del_op(&self, word: bool) -> Op {
        if word || self.range().is_some() {
            Op::Other
        } else {
            Op::Delete
        }
    }

    pub(super) fn backspace(&mut self, word: bool) {
        delete_before(self, word);
    }

    pub(super) fn delete(&mut self, word: bool) {
        delete_after(self, word);
    }

    /// Enter: a new line. In a Markdown list item: the next item at the same layer (same indent,
    /// same marker; numbers count up). An empty item: 1 layer out (the 1st layer: the marker goes).
    pub(crate) fn newline(&mut self) {
        if self.single {
            return;
        }

        self.snapshot(Op::Other);
        self.delete_selection();
        let Pos { row, col } = self.cur;

        match list_item(&self.lines[row]) {
            Some((indent, width, _)) if self.lines[row].len() == indent + width && col == indent + width => {
                self.outdent(indent, width);
            }
            Some((indent, width_value, next)) if col >= indent + width_value => {
                self.insert_raw(&format!("\n{}{next}", " ".repeat(indent)));
            }
            _ => self.insert_raw("\n"),
        }

        self.follow = true;
        self.want_x = None;
    }

    /// Space on a Markdown list item, cursor right after its marker (empty item or text after the
    /// cursor): 1 layer in (2 spaces of indent), the counterpart of backspace. false: not such an
    /// item, the space goes in as text.
    pub(super) fn indent_item(&mut self) -> bool {
        if self.single || self.range().is_some() {
            return false;
        }

        let row = self.cur.row;

        let Some((indent, width, _)) = list_item(&self.lines[row]) else {
            return false;
        };

        if self.cur.col != indent + width {
            return false;
        }

        self.snapshot(Op::Other);
        self.anchor = None;
        self.lines[row].splice(0..0, [' ', ' ']);
        self.cur.col += 2;
        self.follow = true;

        true
    }

    /// List item of the cursor row (INDENT, marker width W): 2 spaces of indent go, at indent 0
    /// (an empty item only) the marker goes.
    pub(super) fn outdent(&mut self, indent: usize, width: usize) {
        // a click leaves anchor = cursor (0 selection): past the new line end after the drain
        self.anchor = None;
        let row = self.cur.row;

        if indent == 0 {
            self.lines[row].drain(..width);
            self.cur.col = 0;
        } else {
            let to = (indent - 1) / 2 * 2;

            self.lines[row].drain(to..indent);
            self.cur.col -= indent - to;
        }
    }

    /// macOS (text substitution "add period with double-space") turns a 2nd space into `.` + space,
    /// some terminals as ⌫ `.` space, some with the 2nd space itself before or after it. All arrive
    /// as 1 burst: undo the period, keep the 2 spaces the person typed.
    pub(super) fn undo_auto_period(&mut self, now: Instant) {
        let height = &self.recent;
        let count = height.len();

        if count < 3
            || height[count - 1].1 != KeyCode::Char(' ')
            || height[count - 2].1 != KeyCode::Char('.')
            || height[count - 1].0 - height[count - 2].0 > BURST
        {
            return;
        }

        let near = |i: usize| height[i + 1].0 - height[i].0 <= BURST;
        let erased = height[count - 3].1 == KeyCode::Backspace
            && near(count - 3)
            && count >= 4
            && height[count - 4].1 == KeyCode::Char(' ');

        if !erased && height[count - 3].1 != KeyCode::Char(' ') {
            return;
        }

        // the 2nd space came as a key too, right before the burst (space space `.` space): `.` + space go
        let doubled = !erased
            && count >= 4
            && height[count - 4].1 == KeyCode::Char(' ')
            && height[count - 2].0 - height[count - 3].0 <= SAME_PRESS;
        let Pos { row, col } = self.cur;
        let line = &mut self.lines[row];

        if col < 2 || line[col - 1] != ' ' || line[col - 2] != '.' {
            return;
        }

        if doubled {
            // `a  . ` → `a  `
            line.drain(col - 2..col);
            self.cur.col -= 2;
        } else if erased {
            line[col - 2] = ' ';
        } else {
            line.remove(col - 2);
            self.cur.col -= 1;
        }

        self.period_undone = Some(now);
    }
}

fn kill_row_edge(widget_state: &mut TextArea, end: bool) {
    if widget_state.range().is_none() {
        let from = widget_state.cur;

        widget_state.line_edge(end, false);
        let to = widget_state.cur;

        widget_state.cur = from;

        if to == from {
            delete_edge(widget_state, end);
            return;
        }

        widget_state.anchor = Some(to);
    }

    let pushed = widget_state.snapshot(Op::Other);

    widget_state.delete_selection();
    widget_state.drop_noop(pushed);
}

fn delete_before(widget_state: &mut TextArea, word: bool) {
    // Markdown list item, cursor right after its marker: 1 layer out (with text after the
    // cursor too); at indent 0 only an empty item (its marker goes), else 1 char as usual
    if widget_state.range().is_none()
        && !widget_state.single
        && let Some((indent, width, _)) = list_item(&widget_state.lines[widget_state.cur.row])
        && widget_state.cur.col == indent + width
        && (indent > 0 || widget_state.lines[widget_state.cur.row].len() == indent + width)
    {
        widget_state.snapshot(Op::Other);
        widget_state.outdent(indent, width);
        widget_state.follow = true;
        return;
    }

    if widget_state.range().is_none() {
        let row = widget_state.cur.row;

        if let Some((agent_value, bytes_value)) = widget_state
            .atomic_ranges(row)
            .into_iter()
            .find(|&(agent, bytes)| widget_state.cur.col > agent && widget_state.cur.col <= bytes)
        {
            widget_state.anchor = Some(Pos { row, col: agent_value });
            widget_state.cur.col = widget_state.cur.col.max(bytes_value);
        }
    }

    let pushed = widget_state.snapshot(widget_state.del_op(word));

    if !widget_state.delete_selection() {
        let end = widget_state.cur;

        let start = if word {
            widget_state.string_left(end)
        } else {
            widget_state.left_of(end)
        };

        if start != end {
            widget_state.delete_range(start, end);
        }
    }

    widget_state.drop_noop(pushed);
    widget_state.follow = true;
}

fn delete_after(widget_state: &mut TextArea, word: bool) {
    if widget_state.range().is_none()
        && let Some((agent_value, bytes_value)) = widget_state
            .atomic_ranges(widget_state.cur.row)
            .into_iter()
            .find(|&(agent, bytes)| widget_state.cur.col >= agent && widget_state.cur.col < bytes)
    {
        widget_state.anchor = Some(Pos {
            row: widget_state.cur.row,
            col: bytes_value,
        });
        widget_state.cur.col = agent_value;
    }

    let pushed = widget_state.snapshot(widget_state.del_op(word));

    if !widget_state.delete_selection() {
        let start = widget_state.cur;

        let end = if word {
            widget_state.word_right(start)
        } else {
            widget_state.right_of(start)
        };

        if start != end {
            widget_state.delete_range(start, end);
        }
    }

    widget_state.drop_noop(pushed);
    widget_state.follow = true;
}

fn delete_edge(widget_state: &mut TextArea, end: bool) {
    if end {
        widget_state.delete(false);
    } else {
        widget_state.backspace(false);
    }
}
