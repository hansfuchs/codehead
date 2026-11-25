use std::ops::ControlFlow;

use super::{Pos, Seg, TextArea, cw, is_word, width_of, wrap};

impl TextArea {
    pub(super) fn left_of(&self, path: Pos) -> Pos {
        if path.col > 0 {
            Pos {
                row: path.row,
                col: path.col - 1,
            }
        } else if path.row > 0 {
            Pos {
                row: path.row - 1,
                col: self.lines[path.row - 1].len(),
            }
        } else {
            path
        }
    }

    pub(super) fn right_of(&self, path: Pos) -> Pos {
        if path.col < self.lines[path.row].len() {
            Pos {
                row: path.row,
                col: path.col + 1,
            }
        } else if path.row + 1 < self.lines.len() {
            Pos {
                row: path.row + 1,
                col: 0,
            }
        } else {
            path
        }
    }

    /// Start of what a word delete (⌥⌫, ctrl+⌫, ctrl+w) takes left of P: the string back to the
    /// whitespace before it; in a path (a `/` in the string) 1 segment, its `/` stays
    /// (`cd ~/a/bc` → `cd ~/a/`, again → `cd ~/`).
    pub(super) fn string_left(&self, path: Pos) -> Pos {
        if path.col == 0 {
            return self.left_of(path);
        }

        let line = &self.lines[path.row];
        let mut character = path.col;

        while character > 0 && line[character - 1].is_whitespace() {
            character -= 1;
        }

        // the slash(es) after a segment go with it
        while character > 0 && line[character - 1] == '/' {
            character -= 1;
        }

        while character > 0 && !line[character - 1].is_whitespace() && line[character - 1] != '/' {
            character -= 1;
        }

        Pos {
            row: path.row,
            col: character,
        }
    }

    pub(super) fn word_left(&self, path: Pos) -> Pos {
        if path.col == 0 {
            return self.left_of(path);
        }

        let line = &self.lines[path.row];
        let mut character = path.col;

        while character > 0 && !is_word(line[character - 1]) {
            character -= 1;
        }

        while character > 0 && is_word(line[character - 1]) {
            character -= 1;
        }

        Pos {
            row: path.row,
            col: character,
        }
    }

    pub(super) fn word_right(&self, path: Pos) -> Pos {
        let line = &self.lines[path.row];

        if path.col >= line.len() {
            return self.right_of(path);
        }

        let mut character = path.col;

        while character < line.len() && !is_word(line[character]) {
            character += 1;
        }

        while character < line.len() && is_word(line[character]) {
            character += 1;
        }

        Pos {
            row: path.row,
            col: character,
        }
    }

    pub(super) fn move_to(&mut self, path: Pos, select: bool) {
        move_cursor(self, path, select);
    }

    pub(super) fn atomic_ranges(&self, row: usize) -> Vec<(usize, usize)> {
        static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();

        let re = RE.get_or_init(|| {
            regex::Regex::new(r"\[(?:Image #[0-9]+|Pasted text #[0-9]+ \+[0-9]+ lines)\]").expect("valid regex")
        });

        let text: String = self.lines[row].iter().collect();

        re.find_iter(&text)
            .map(|message| {
                (
                    text[..message.start()].chars().count(),
                    text[..message.end()].chars().count(),
                )
            })
            .collect()
    }

    /// Rebuild the visual rows from the text at the last render width. An edit + ↑/↓ in 1 event
    /// batch reads the rows before the next render: stale rows point past the text.
    pub(super) fn layout(&mut self) {
        build_layout(self);
    }

    /// Visual row index + x of a position.
    pub(super) fn visual_of(&self, path: Pos) -> (usize, usize) {
        locate_visual(self, path)
    }

    /// Position at visual row VROW, display column X.
    pub(super) fn pos_at(&self, vrow: usize, x: usize) -> Pos {
        locate_text(self, vrow, x)
    }

    pub(super) fn vertical(&mut self, delta: isize, select: bool) {
        self.layout();

        if self.vis.is_empty() {
            return;
        }

        let (vrow, x) = self.visual_of(self.cur);
        let x = *self.want_x.get_or_insert(x);
        let target = vrow as isize + delta;

        let path = if target < 0 {
            Pos { row: 0, col: 0 }
        } else if target as usize >= self.vis.len() {
            let result = self.lines.len() - 1;

            Pos {
                row: result,
                col: self.lines[result].len(),
            }
        } else {
            self.pos_at(target as usize, x)
        };

        let keep = self.want_x;

        self.move_to(path, select);
        self.want_x = keep;
    }

    pub(super) fn line_edge(&mut self, end: bool, select: bool) {
        self.layout();
        let (vrow, _) = self.visual_of(self.cur);

        let path = match self.vis.get(vrow) {
            Some(text) if end => Pos {
                row: text.row,
                col: if text.last {
                    text.end
                } else {
                    text.end.saturating_sub(1).max(text.start)
                },
            },
            Some(text_value) => Pos {
                row: text_value.row,
                col: text_value.start,
            },
            // 0 layout yet (0 render): the logical row
            None => Pos {
                row: self.cur.row,
                col: if end { self.lines[self.cur.row].len() } else { 0 },
            },
        };

        self.move_to(path, select);
    }
}

fn move_cursor(widget_state: &mut TextArea, path: Pos, select: bool) {
    let path_value = if select {
        widget_state
            .atomic_ranges(path.row)
            .into_iter()
            .find(|&(agent, bytes)| path.col > agent && path.col < bytes)
            .map_or(path, |(agent_value, bytes_value)| Pos {
                row: path.row,
                col: if path < widget_state.cur {
                    agent_value
                } else {
                    bytes_value
                },
            })
    } else {
        path
    };

    if !select
        && widget_state.range().is_none()
        && let Some((agent_3, bytes_3)) =
            widget_state
                .atomic_ranges(path_value.row)
                .into_iter()
                .find(|&(agent_2, bytes_2)| {
                    (path_value.col > agent_2 && path_value.col < bytes_2)
                        || (path_value < widget_state.cur && path_value.col == bytes_2)
                        || (path_value > widget_state.cur && path_value.col == agent_2)
                })
    {
        let (anchor, cur) = if path_value < widget_state.cur {
            (bytes_3, agent_3)
        } else {
            (agent_3, bytes_3)
        };

        widget_state.anchor = Some(Pos {
            row: path_value.row,
            col: anchor,
        });
        widget_state.cur = Pos {
            row: path_value.row,
            col: cur,
        };
        widget_state.follow = true;
        widget_state.last_op = None;
        return;
    }

    if select {
        if widget_state.anchor.is_none() {
            widget_state.anchor = Some(widget_state.cur);
        }
    } else {
        widget_state.anchor = None;
    }

    widget_state.cur = path_value;
    widget_state.follow = true;
    widget_state.last_op = None;
}

fn build_layout(widget_state: &mut TextArea) {
    if widget_state.area.width == 0 {
        return;
    }

    let width = widget_state.area.width.saturating_sub(1) as usize;

    widget_state.vis.clear();

    for (row, line) in widget_state.lines.iter().enumerate() {
        let segs = wrap(line, width);
        let count = segs.len();

        for (i, (start, end)) in segs.into_iter().enumerate() {
            widget_state.vis.push(Seg {
                row,
                start,
                end,
                last: i + 1 == count,
            });
        }
    }
}

fn locate_visual(widget_state: &TextArea, path: Pos) -> (usize, usize) {
    let Some(line) = widget_state.lines.get(path.row) else {
        return (0, 0);
    };

    if path.col > line.len() {
        return (0, 0);
    }

    for (i, text) in widget_state.vis.iter().enumerate() {
        if text.row == path.row
            && text.start <= line.len()
            && path.col >= text.start
            && (path.col < text.end || (text.last && path.col == text.end))
        {
            return (i, width_of(&widget_state.lines[text.row][text.start..path.col]));
        }
    }

    (0, 0)
}

fn locate_text(widget_state: &TextArea, vrow: usize, x: usize) -> Pos {
    let Some(text) = widget_state.vis.get(vrow.min(widget_state.vis.len().saturating_sub(1))) else {
        return Pos::default();
    };

    let Some(line) = widget_state.lines.get(text.row) else {
        return Pos::default();
    };

    let (end, mut col, mut width) = (text.end.min(line.len()), text.start.min(line.len()), 0);

    while col < end {
        if let ControlFlow::Break(()) = advance_character(x, line, &mut col, &mut width) {
            break;
        }
    }

    if !text.last && col == text.end && col > text.start {
        col -= 1;
    }

    Pos { row: text.row, col }
}

fn advance_character(x: usize, line: &[char], col: &mut usize, width: &mut usize) -> ControlFlow<()> {
    let character = cw(line[*col]);

    if *width + character > x {
        // click on the right half of a wide char → after it
        if character > 1 && x >= *width + character / 2 {
            *col += 1;
        }

        return ControlFlow::Break(());
    }

    *width += character;
    *col += 1;

    ControlFlow::Continue(())
}
