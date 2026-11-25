use std::ops::ControlFlow;

use super::{Pos, Seg, TextArea, cw, is_word, width_of, wrap};

impl TextArea {
    pub(super) fn left_of(&self, pos: Pos) -> Pos {
        if pos.col > 0 {
            Pos {
                row: pos.row,
                col: pos.col - 1,
            }
        } else if pos.row > 0 {
            Pos {
                row: pos.row - 1,
                col: self.lines[pos.row - 1].len(),
            }
        } else {
            pos
        }
    }

    pub(super) fn right_of(&self, pos: Pos) -> Pos {
        if pos.col < self.lines[pos.row].len() {
            Pos {
                row: pos.row,
                col: pos.col + 1,
            }
        } else if pos.row + 1 < self.lines.len() {
            Pos {
                row: pos.row + 1,
                col: 0,
            }
        } else {
            pos
        }
    }

    /// Start of what a word delete (⌥⌫, ctrl+⌫, ctrl+w) takes left of P: the string back to the
    /// whitespace before it; in a path (a `/` in the string) 1 segment, its `/` stays
    /// (`cd ~/a/bc` → `cd ~/a/`, again → `cd ~/`).
    pub(super) fn string_left(&self, pos: Pos) -> Pos {
        if pos.col == 0 {
            return self.left_of(pos);
        }

        let line = &self.lines[pos.row];
        let mut col = pos.col;

        while col > 0 && line[col - 1].is_whitespace() {
            col -= 1;
        }

        // the slash(es) after a segment go with it
        while col > 0 && line[col - 1] == '/' {
            col -= 1;
        }

        while col > 0 && !line[col - 1].is_whitespace() && line[col - 1] != '/' {
            col -= 1;
        }

        Pos { row: pos.row, col }
    }

    pub(super) fn word_left(&self, pos: Pos) -> Pos {
        if pos.col == 0 {
            return self.left_of(pos);
        }

        let line = &self.lines[pos.row];
        let mut col = pos.col;

        while col > 0 && !is_word(line[col - 1]) {
            col -= 1;
        }

        while col > 0 && is_word(line[col - 1]) {
            col -= 1;
        }

        Pos { row: pos.row, col }
    }

    pub(super) fn word_right(&self, pos: Pos) -> Pos {
        let line = &self.lines[pos.row];

        if pos.col >= line.len() {
            return self.right_of(pos);
        }

        let mut col = pos.col;

        while col < line.len() && !is_word(line[col]) {
            col += 1;
        }

        while col < line.len() && is_word(line[col]) {
            col += 1;
        }

        Pos { row: pos.row, col }
    }

    pub(super) fn move_to(&mut self, pos: Pos, select: bool) {
        move_cursor(self, pos, select);
    }

    pub(super) fn atomic_ranges(&self, row: usize) -> Vec<(usize, usize)> {
        static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();

        let re = RE.get_or_init(|| {
            regex::Regex::new(r"\[(?:Image #[0-9]+|Pasted text #[0-9]+ \+[0-9]+ lines)\]").expect("valid regex")
        });

        let text: String = self.lines[row].iter().collect();

        re.find_iter(&text)
            .map(|hit| (text[..hit.start()].chars().count(), text[..hit.end()].chars().count()))
            .collect()
    }

    /// Rebuild the visual rows from the text at the last render width. An edit + ↑/↓ in 1 event
    /// batch reads the rows before the next render: stale rows point past the text.
    pub(super) fn layout(&mut self) {
        build_layout(self);
    }

    /// Visual row index + x of a position.
    pub(super) fn visual_of(&self, pos: Pos) -> (usize, usize) {
        locate_visual(self, pos)
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

        let pos = if target < 0 {
            Pos { row: 0, col: 0 }
        } else if target as usize >= self.vis.len() {
            let last = self.lines.len() - 1;

            Pos {
                row: last,
                col: self.lines[last].len(),
            }
        } else {
            self.pos_at(target as usize, x)
        };

        let keep = self.want_x;

        self.move_to(pos, select);
        self.want_x = keep;
    }

    pub(super) fn line_edge(&mut self, end: bool, select: bool) {
        self.layout();
        let (vrow, _) = self.visual_of(self.cur);

        let pos = match self.vis.get(vrow) {
            Some(seg) if end => Pos {
                row: seg.row,
                col: if seg.last {
                    seg.end
                } else {
                    seg.end.saturating_sub(1).max(seg.start)
                },
            },
            Some(seg) => Pos {
                row: seg.row,
                col: seg.start,
            },
            // 0 layout yet (0 render): the logical row
            None => Pos {
                row: self.cur.row,
                col: if end { self.lines[self.cur.row].len() } else { 0 },
            },
        };

        self.move_to(pos, select);
    }
}

fn move_cursor(ta: &mut TextArea, pos: Pos, select: bool) {
    let target = if select {
        ta.atomic_ranges(pos.row)
            .into_iter()
            .find(|&(start, end)| pos.col > start && pos.col < end)
            .map_or(pos, |(start, end)| Pos {
                row: pos.row,
                col: if pos < ta.cur { start } else { end },
            })
    } else {
        pos
    };

    if !select
        && ta.range().is_none()
        && let Some((start, end)) = ta.atomic_ranges(target.row).into_iter().find(|&(start, end)| {
            (target.col > start && target.col < end)
                || (target < ta.cur && target.col == end)
                || (target > ta.cur && target.col == start)
        })
    {
        let (anchor, cur) = if target < ta.cur { (end, start) } else { (start, end) };

        ta.anchor = Some(Pos {
            row: target.row,
            col: anchor,
        });
        ta.cur = Pos {
            row: target.row,
            col: cur,
        };
        ta.follow = true;
        ta.last_op = None;
        return;
    }

    if select {
        if ta.anchor.is_none() {
            ta.anchor = Some(ta.cur);
        }
    } else {
        ta.anchor = None;
    }

    ta.cur = target;
    ta.follow = true;
    ta.last_op = None;
}

fn build_layout(ta: &mut TextArea) {
    if ta.area.width == 0 {
        return;
    }

    let width = ta.area.width.saturating_sub(1) as usize;

    ta.vis.clear();

    for (row, line) in ta.lines.iter().enumerate() {
        let segs = wrap(line, width);
        let count = segs.len();

        for (i, (start, end)) in segs.into_iter().enumerate() {
            ta.vis.push(Seg {
                row,
                start,
                end,
                last: i + 1 == count,
            });
        }
    }
}

fn locate_visual(ta: &TextArea, pos: Pos) -> (usize, usize) {
    let Some(line) = ta.lines.get(pos.row) else {
        return (0, 0);
    };

    if pos.col > line.len() {
        return (0, 0);
    }

    for (i, seg) in ta.vis.iter().enumerate() {
        if seg.row == pos.row
            && seg.start <= line.len()
            && pos.col >= seg.start
            && (pos.col < seg.end || (seg.last && pos.col == seg.end))
        {
            return (i, width_of(&ta.lines[seg.row][seg.start..pos.col]));
        }
    }

    (0, 0)
}

fn locate_text(ta: &TextArea, vrow: usize, x: usize) -> Pos {
    let Some(seg) = ta.vis.get(vrow.min(ta.vis.len().saturating_sub(1))) else {
        return Pos::default();
    };

    let Some(line) = ta.lines.get(seg.row) else {
        return Pos::default();
    };

    let (end, mut col, mut width) = (seg.end.min(line.len()), seg.start.min(line.len()), 0);

    while col < end {
        if let ControlFlow::Break(()) = advance_character(x, line, &mut col, &mut width) {
            break;
        }
    }

    if !seg.last && col == seg.end && col > seg.start {
        col -= 1;
    }

    Pos { row: seg.row, col }
}

fn advance_character(x: usize, line: &[char], col: &mut usize, width: &mut usize) -> ControlFlow<()> {
    let char_width = cw(line[*col]);

    if *width + char_width > x {
        // click on the right half of a wide char → after it
        if char_width > 1 && x >= *width + char_width / 2 {
            *col += 1;
        }

        return ControlFlow::Break(());
    }

    *width += char_width;
    *col += 1;

    ControlFlow::Continue(())
}
