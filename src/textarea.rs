//! Text input: single line (title) or multiline with soft word wrap (prompt).
//! Mouse: click places the cursor, drag selects (copied on release), double click = word,
//! triple click = line, wheel scrolls. Keys: shift+move selects, backspace removes a selection,
//! ctrl+shift+a (all), ctrl+c/x/v (⌘ too), ctrl+z / ⌘z undo, ctrl+shift+z / ⌘⇧z / ctrl+y redo, ctrl+a/e row start/end, ctrl+u/k delete to row start/end, ctrl+←→ words, bracketed paste (ctrl+shift+v in the terminal).

use std::time::{Duration, Instant};

use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use unicode_width::UnicodeWidthChar;

use crate::{clip, theme};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct Pos {
    pub row: usize,
    pub col: usize,
}

#[derive(Clone, Copy, Debug)]
struct Seg {
    row: usize,
    start: usize,
    end: usize,
    last: bool,
}

#[derive(Clone)]
struct Snapshot {
    lines: Vec<Vec<char>>,
    cur: Pos,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Op {
    Type,
    /// 1 char removed (backspace / delete): a run of them = 1 undo step.
    Delete,
    Other,
}

pub enum Outcome {
    /// Key used by the input.
    Handled,
    /// Not an input key: the screen handles it (tab, esc, ctrl+enter, …).
    Ignored,
}

pub struct TextArea {
    lines: Vec<Vec<char>>,
    cur: Pos,
    anchor: Option<Pos>,
    single: bool,
    scroll: usize,
    follow: bool,
    want_x: Option<usize>,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    last_op: Option<Op>,
    area: Rect,
    vis: Vec<Seg>,
    drag: bool,
    clicks: Option<(Instant, Pos, u8)>,
    pub placeholder: String,
    /// Text copied by the last mouse selection (for a toast).
    pub copied: Option<usize>,
    /// Base style (default: the input panel). The chat prompt uses the plain terminal bg.
    pub style: Option<Style>,
    /// Last keys + their time: spots the OS "period on double space" burst.
    recent: Vec<(Instant, KeyCode)>,
    /// Time of the last undone auto period: a space of the same burst after it is dropped.
    period_undone: Option<Instant>,
    /// `@name` mentions drawn in these colors (agent name, color).
    pub marks: Vec<(String, Color)>,
}

/// Per char of LINE: the color of the `@name` mention it belongs to (a name of MARKS, the `@` at a
/// line start or after a space / bracket, the name not followed by a name char).
pub fn mention_colors(line: &[char], marks: &[(String, Color)]) -> Vec<Option<Color>> {
    let mut out = vec![None; line.len()];
    if marks.is_empty() {
        return out;
    }
    let name_char = |c: char| is_word(c) || c == '-';
    let mut i = 0;
    while i < line.len() {
        if line[i] != '@' || (i > 0 && !(line[i - 1].is_whitespace() || matches!(line[i - 1], '(' | '[' | '"' | '\''))) {
            i += 1;
            continue;
        }
        let mut j = i + 1;
        while j < line.len() && name_char(line[j]) {
            j += 1;
        }
        let name: String = line[i + 1..j].iter().collect();
        if let Some((_, c)) = marks.iter().find(|(n, _)| *n == name) {
            out[i..j].iter_mut().for_each(|x| *x = Some(*c));
            // `@new (replaces @old)`: the note in the color of NEW, `@old` in the color of OLD
            if let Some((end, old, oc)) = marks.iter().find_map(|(old, oc)| {
                let note: Vec<char> = crate::mention::replaces_note(old).chars().collect();
                line[j..].starts_with(&note).then_some((j + note.len(), old, oc))
            }) {
                out[j + 1..end].iter_mut().for_each(|x| *x = Some(*c));
                let at = end - 1 - old.chars().count() - 1;
                out[at..end - 1].iter_mut().for_each(|x| *x = Some(*oc));
                j = end;
            }
        }
        i = j.max(i + 1);
    }
    out
}

/// Keys closer than this came from 1 burst (the OS / terminal), not from a person.
const BURST: Duration = Duration::from_millis(12);
/// Keys of 1 key press that the terminal sends in 2 writes (a draw can sit between them).
const SAME_PRESS: Duration = Duration::from_millis(60);

thread_local! {
    /// Read time of the key in work (the event loop sets it): bursts stay bursts, however long
    /// the keys before them took to handle.
    static KEY_AT: std::cell::Cell<Option<Instant>> = const { std::cell::Cell::new(None) };
}

/// Read time of the next keys (None: the time of the call).
pub fn stamp(at: Option<Instant>) {
    KEY_AT.with(|c| c.set(at));
}

/// Markdown list item at the start of LINE: (indent, marker width incl. its space, marker of the
/// next item). `- `, `* `, `+ ` (+ `[ ] ` / `[x] `), `1. `, `1) `.
fn list_item(line: &[char]) -> Option<(usize, usize, String)> {
    let indent = line.iter().take_while(|c| **c == ' ').count();
    let rest = &line[indent..];
    let bullet = match rest {
        [b @ ('-' | '*' | '+'), ' ', ..] => Some(*b),
        _ => None,
    };
    if let Some(b) = bullet {
        if let ['[', ' ' | 'x' | 'X', ']', ' ', ..] = &rest[2..] {
            return Some((indent, 6, format!("{b} [ ] ")));
        }
        return Some((indent, 2, format!("{b} ")));
    }
    let digits = rest.iter().take_while(|c| c.is_ascii_digit()).count();
    if (1..=9).contains(&digits) && matches!(rest.get(digits), Some('.' | ')')) && rest.get(digits + 1) == Some(&' ') {
        let n: u64 = rest[..digits].iter().collect::<String>().parse().ok()?;
        return Some((indent, digits + 2, format!("{}{} ", n + 1, rest[digits])));
    }
    None
}

/// LINE opens or closes a ``` fence.
fn is_fence(line: &[char]) -> bool {
    let t: Vec<&char> = line.iter().skip_while(|c| **c == ' ').collect();
    t.len() >= 3 && t[..3].iter().all(|c| **c == '`')
}

/// Per line: syntax colors of the fenced code (None: prose or a fence line), and fence lines.
fn fence_colors(lines: &[Vec<char>]) -> (Vec<Option<Vec<ratatui::style::Color>>>, Vec<bool>) {
    let mut colors = vec![None; lines.len()];
    let fence: Vec<bool> = lines.iter().map(|l| is_fence(l)).collect();
    let mut i = 0;
    while i < lines.len() {
        if !fence[i] {
            i += 1;
            continue;
        }
        let tag: String = lines[i].iter().skip_while(|c| **c == ' ' || **c == '`').collect::<String>().trim().to_string();
        let end = (i + 1..lines.len()).find(|j| fence[*j]).unwrap_or(lines.len());
        if end > i + 1 {
            let body: String = lines[i + 1..end].iter().map(|l| l.iter().collect::<String>()).collect::<Vec<_>>().join("\n");
            let lang = if tag.is_empty() { crate::code::language(&body, &[]) } else { tag.as_str() };
            let hl = crate::code::highlight(&body, lang);
            for (k, row) in (i + 1..end).enumerate() {
                colors[row] = hl.get(k).map(|l| l.iter().map(|(c, _)| *c).collect());
            }
        }
        i = end + 1;
    }
    (colors, fence)
}

/// TEXT without its trailing empty Markdown list items (a marker an Enter left, e.g. `7.`), only
/// after other lines, trimmed at the end. A sent message: 0 stray marker the view hides.
pub fn without_empty_items(text: &str) -> &str {
    let mut t = text.trim_end();
    while let Some(i) = t.rfind('\n') {
        let line: Vec<char> = t[i + 1..].chars().chain([' ']).collect();
        match list_item(&line) {
            Some((indent, w, _)) if line.len() == indent + w => t = t[..i].trim_end(),
            _ => break,
        }
    }
    t
}

/// $CODEHEAD_KEYLOG=<file>: every key of every input goes there (ms since the 1st key, code, modifiers).
/// For a report on keys the terminal sends in a way codehead does not expect.
fn keylog(k: &KeyEvent, now: Instant) {
    static LOG: std::sync::OnceLock<Option<(std::sync::Mutex<std::fs::File>, Instant)>> = std::sync::OnceLock::new();
    let log = LOG.get_or_init(|| {
        let path = std::env::var_os("CODEHEAD_KEYLOG").filter(|p| !p.is_empty())?;
        let f = std::fs::OpenOptions::new().create(true).append(true).open(path).ok()?;
        Some((std::sync::Mutex::new(f), now))
    });
    if let Some((f, t0)) = log
        && let Ok(mut f) = f.lock()
    {
        use std::io::Write;
        let _ = writeln!(f, "{:>8.1}ms {:?} {:?}", now.saturating_duration_since(*t0).as_secs_f64() * 1000.0, k.code, k.modifiers);
    }
}

fn cw(c: char) -> usize {
    c.width().unwrap_or(0)
}

fn width_of(s: &[char]) -> usize {
    s.iter().map(|&c| cw(c)).sum()
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// The modifiers of the row keys (⌫: to the row's start, ←→: row start / end): ⌘ (super, kitty
/// protocol), alt off macOS. On macOS a terminal's alt is ⌥, the key of the word keys.
pub fn line_mod(m: KeyModifiers) -> bool {
    m.contains(KeyModifiers::SUPER) || (!cfg!(target_os = "macos") && m.contains(KeyModifiers::ALT))
}

/// Esc on unsaved edits (esc never drops edits unasked): DIRTY and 0 esc within 2 s before → false
/// (the caller warns: "esc again: drop"); a 2nd esc in time or 0 edits → true (drop / close now).
/// AT: the caller's last esc.
pub fn esc_drops(at: &mut Option<std::time::Instant>, dirty: bool) -> bool {
    if !dirty || at.take().is_some_and(|t| t.elapsed() < std::time::Duration::from_secs(2)) {
        return true;
    }

    *at = Some(std::time::Instant::now());
    false
}

/// Delete keys of a 1-line text kept as a String (dropdown filters): ⌫ 1 char; ⌥⌫ / ctrl+⌫ /
/// ctrl+w / ctrl+h the string left of the end (a path: 1 segment, as [`TextArea`]); ⌘⌫ (alt+⌫ off
/// macOS) / ctrl+u all. True: S changed or the key was 1 of these.
pub fn delete_key(s: &mut String, k: &KeyEvent) -> bool {
    let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
    let alt = k.modifiers.contains(KeyModifiers::ALT);
    let all = (k.code == KeyCode::Backspace && line_mod(k.modifiers)) || (ctrl && k.code == KeyCode::Char('u'));
    let word = (k.code == KeyCode::Backspace && (ctrl || alt)) || (ctrl && matches!(k.code, KeyCode::Char('w' | 'h')));
    if all {
        s.clear();
    } else if word {
        let l: Vec<char> = s.chars().collect();
        let mut c = l.len();
        while c > 0 && l[c - 1].is_whitespace() {
            c -= 1;
        }
        while c > 0 && l[c - 1] == '/' {
            c -= 1;
        }
        while c > 0 && !l[c - 1].is_whitespace() && l[c - 1] != '/' {
            c -= 1;
        }
        *s = l[..c].iter().collect();
    } else if k.code == KeyCode::Backspace {
        s.pop();
    } else {
        return false;
    }
    true
}

/// Soft word wrap of 1 line into char ranges of max. WIDTH display columns.
pub fn wrap(line: &[char], width: usize) -> Vec<(usize, usize)> {
    let width = width.max(1);
    let mut segs = Vec::new();
    let (mut start, mut i, mut w, mut space) = (0, 0, 0, None::<usize>);
    while i < line.len() {
        let c = cw(line[i]);
        if w + c > width && i > start {
            let brk = match space {
                Some(s) if s + 1 > start => s + 1,
                _ => i,
            };
            segs.push((start, brk));
            start = brk;
            i = brk;
            w = 0;
            space = None;
            continue;
        }
        if line[i] == ' ' {
            space = Some(i);
        }
        w += c;
        i += 1;
    }
    segs.push((start, line.len()));
    segs
}

impl TextArea {
    pub fn new(single: bool, placeholder: &str) -> TextArea {
        TextArea {
            lines: vec![Vec::new()],
            cur: Pos::default(),
            anchor: None,
            single,
            scroll: 0,
            follow: true,
            want_x: None,
            undo: Vec::new(),
            redo: Vec::new(),
            last_op: None,
            area: Rect::default(),
            vis: Vec::new(),
            drag: false,
            clicks: None,
            placeholder: placeholder.to_string(),
            copied: None,
            style: None,
            recent: Vec::new(),
            period_undone: None,
            marks: Vec::new(),
        }
    }

    pub fn text(&self) -> String {
        self.lines.iter().map(|l| l.iter().collect::<String>()).collect::<Vec<_>>().join("\n")
    }

    /// The owner edited the text since the last `set_text` (1+ undo step).
    pub fn edited(&self) -> bool {
        !self.undo.is_empty()
    }

    /// New value from the program (load, send): a new undo history, like a browser input.
    pub fn set_text(&mut self, text: &str) {
        self.undo.clear();
        self.redo.clear();
        self.last_op = None;
        self.replace_all(text);
    }

    /// New value as 1 undo step (chat history ↑↓): ctrl+z returns to the text before.
    pub fn load_text(&mut self, text: &str) {
        if text == self.text() {
            return;
        }
        self.snapshot(Op::Other);
        self.replace_all(text);
        self.last_op = None;
    }

    fn replace_all(&mut self, text: &str) {
        self.lines = vec![Vec::new()];
        self.cur = Pos::default();
        self.anchor = None;
        self.insert_raw(text);
        self.follow = true;
        self.want_x = None;
    }

    /// A 1-line input.
    pub fn is_single(&self) -> bool {
        self.single
    }

    pub fn is_empty(&self) -> bool {
        self.lines.len() == 1 && self.lines[0].is_empty()
    }

    /// Cursor on the 1st visual row (wrap included): ↑ leaves the input.
    pub fn on_first_row(&self) -> bool {
        self.vis.is_empty() || self.visual_of(self.cur).0 == 0
    }

    /// Cursor on the last visual row: ↓ leaves the input.
    pub fn on_last_row(&self) -> bool {
        self.vis.is_empty() || self.visual_of(self.cur).0 + 1 >= self.vis.len()
    }

    /// Cursor at the start of the text, without a selection.
    pub fn move_start(&mut self) {
        self.move_to(Pos { row: 0, col: 0 }, false);
    }

    /// Cursor at the very end of the text.
    pub fn move_end(&mut self) {
        let r = self.lines.len() - 1;
        self.move_to(Pos { row: r, col: self.lines[r].len() }, false);
    }

    pub fn area(&self) -> Rect {
        self.area
    }

    /// The cursor: (row, column in chars).
    pub fn cursor(&self) -> (usize, usize) {
        (self.cur.row, self.cur.col)
    }

    pub fn hide(&mut self) {
        self.area = Rect::default();
    }

    /// Display rows needed for WIDTH (input height for the title).
    pub fn rows_for(&self, width: u16) -> usize {
        let w = width.saturating_sub(1) as usize;
        self.lines.iter().map(|l| wrap(l, w).len()).sum::<usize>().max(1)
    }

    /// `@query` right before the cursor (the `@` at a line start or after a space / bracket, 0 selection):
    /// the `@` position + the query. For the agent dropdown.
    pub fn mention(&self) -> Option<(Pos, String)> {
        self.trigger('@', |c| is_word(c) || matches!(c, '-' | '.'))
    }

    /// `>query` right before the cursor (0 spaces in the query): the `>` position + the query.
    /// For the command dropdown.
    pub fn command(&self) -> Option<(Pos, String)> {
        let k = crate::config::command_key();
        self.trigger(k, |c| !c.is_whitespace() && c != k)
    }

    /// A path being typed: the word that ends at the cursor (0 spaces, quotes, brackets in it; the
    /// next char: none or a space / closer), if it holds a `/`. Its start + the word. Words of the
    /// `@` and `>` dropdowns: not paths. For the path dropdown.
    #[cfg(test)]
    pub fn path_word(&self) -> Option<(Pos, String)> {
        self.path_word_quoted().map(|(p, w, _)| (p, w))
    }

    /// [`path_word`], with spaces after an open `"` (`"~/My Notes/a`): the word starts after
    /// the quote. The 3rd value: inside such a quote.
    pub fn path_word_quoted(&self) -> Option<(Pos, String, bool)> {
        if self.range().is_some() {
            return None;
        }
        let line = &self.lines[self.cur.row];
        let before = &line[..self.cur.col];
        if before.iter().filter(|&&c| c == '"').count() % 2 == 1
            && let Some(q) = before.iter().rposition(|&c| c == '"')
            && line.get(self.cur.col).is_none_or(|&c| c.is_whitespace() || c == '"')
        {
            let word: String = line[q + 1..self.cur.col].iter().collect();
            return word.contains('/').then_some((Pos { row: self.cur.row, col: q + 1 }, word, true));
        }
        let edge = |c: char| c.is_whitespace() || matches!(c, '(' | ')' | '[' | ']' | '"' | '\'' | '`' | '<' | '>' | ',');
        if line.get(self.cur.col).is_some_and(|&c| !edge(c)) {
            return None;
        }
        let mut c = self.cur.col;
        while c > 0 && !edge(line[c - 1]) {
            c -= 1;
        }
        let word: String = line[c..self.cur.col].iter().collect();
        if !word.contains('/') || word.starts_with('@') || (c > 0 && line[c - 1] == crate::config::command_key()) {
            return None;
        }
        Some((Pos { row: self.cur.row, col: c }, word, false))
    }

    /// MARK + query (chars that pass QUERY) right before the cursor; MARK at a line start or after
    /// a space / bracket, 0 selection. The MARK position + the query.
    fn trigger(&self, mark: char, query: impl Fn(char) -> bool) -> Option<(Pos, String)> {
        if self.range().is_some() {
            return None;
        }
        let line = &self.lines[self.cur.row];
        let mut c = self.cur.col;
        while c > 0 && line[c - 1] != mark && query(line[c - 1]) {
            c -= 1;
        }
        if c == 0 || line[c - 1] != mark {
            return None;
        }
        let at = c - 1;
        if at > 0 && !(line[at - 1].is_whitespace() || matches!(line[at - 1], '(' | '[' | '"' | '\'')) {
            return None;
        }
        Some((Pos { row: self.cur.row, col: at }, line[c..self.cur.col].iter().collect()))
    }

    /// The text from FROM to the cursor (same row, 0 selection). None: another row, after it.
    pub fn since(&self, from: Pos) -> Option<String> {
        if self.range().is_some() || from.row != self.cur.row || from.col > self.cur.col {
            return None;
        }
        Some(self.lines[self.cur.row][from.col..self.cur.col].iter().collect())
    }

    /// `#query` right before the cursor: the `#` position + the query (a shortcut name). For
    /// the shortcut dropdown.
    pub fn shortcut(&self) -> Option<(Pos, String)> {
        self.trigger('#', |c| is_word(c) || c == '-')
    }

    /// The `#name` token at screen cell AT (None: at the cursor): its `#` position + the name.
    /// For the shortcut popup.
    pub fn shortcut_at(&self, at: Option<Position>) -> Option<(Pos, String)> {
        let p = match at {
            Some(a) => {
                if !self.area.contains(a) {
                    return None;
                }
                let y = usize::from(a.y - self.area.y);
                self.pos_at(self.scroll + y, usize::from(a.x - self.area.x))
            }
            None => self.cur,
        };
        let line = self.lines.get(p.row)?;
        let name = |c: char| is_word(c) || c == '-';
        // back to the `#` (the cursor right after the name counts too)
        let mut c = p.col.min(line.len());
        while c > 0 && line[c - 1] != '#' && name(line[c - 1]) {
            c -= 1;
        }
        let hash = if line.get(c) == Some(&'#') { c } else if c > 0 && line[c - 1] == '#' { c - 1 } else { return None };
        if hash > 0 && !(line[hash - 1].is_whitespace() || matches!(line[hash - 1], '(' | '[' | '"' | '\'')) {
            return None;
        }
        let word: String = line[hash + 1..].iter().take_while(|c| name(**c)).collect();
        (!word.is_empty() && p.col <= hash + 1 + word.chars().count()).then_some((Pos { row: p.row, col: hash }, word))
    }

    /// Replace FROM..cursor (same row) with TEXT (1 undo step).
    pub fn replace_to_cursor(&mut self, from: Pos, text: &str) {
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

    /// Screen cell of P (last render). None: scrolled out.
    pub fn screen_pos(&self, p: Pos) -> Option<Position> {
        if self.vis.is_empty() {
            return Some(Position { x: self.area.x, y: self.area.y });
        }
        let (vrow, x) = self.visual_of(p);
        if vrow < self.scroll || vrow >= self.scroll + self.area.height as usize {
            return None;
        }
        Some(Position { x: self.area.x + x as u16, y: self.area.y + (vrow - self.scroll) as u16 })
    }

    // ---------- selection ----------
    fn range(&self) -> Option<(Pos, Pos)> {
        let a = self.anchor?;
        if a == self.cur {
            return None;
        }
        Some(if a < self.cur { (a, self.cur) } else { (self.cur, a) })
    }

    pub fn selected_text(&self) -> Option<String> {
        let (a, b) = self.range()?;
        let mut out = String::new();
        for r in a.row..=b.row {
            let line = &self.lines[r];
            let s = if r == a.row { a.col } else { 0 };
            let e = if r == b.row { b.col } else { line.len() };
            out.extend(&line[s..e]);
            if r != b.row {
                out.push('\n');
            }
        }
        Some(out)
    }

    fn delete_range(&mut self, mut a: Pos, mut b: Pos) {
        if let Some((start,_)) = self.atomic_ranges(a.row).into_iter().find(|&(start,end)| a.col > start && a.col < end) { a.col = start; }
        if let Some((_,end)) = self.atomic_ranges(b.row).into_iter().find(|&(start,end)| b.col > start && b.col < end) { b.col = end; }
        let tail: Vec<char> = self.lines[b.row][b.col..].to_vec();
        self.lines[a.row].truncate(a.col);
        self.lines[a.row].extend(tail);
        if b.row > a.row {
            self.lines.drain(a.row + 1..=b.row);
        }
        self.cur = a;
        self.anchor = None;
    }

    fn delete_selection(&mut self) -> bool {
        match self.range() {
            Some((a, b)) => {
                self.delete_range(a, b);
                true
            }
            None => {
                self.anchor = None;
                false
            }
        }
    }

    // ---------- undo ----------
    /// Push an undo step (a run of Type / Delete = 1 step). True: step pushed.
    fn snapshot(&mut self, op: Op) -> bool {
        if op != Op::Other && self.last_op == Some(op) {
            return false;
        }
        self.undo.push(Snapshot { lines: self.lines.clone(), cur: self.cur });
        if self.undo.len() > 200 {
            self.undo.remove(0);
        }
        self.redo.clear();
        self.last_op = Some(op);
        true
    }

    fn restore(&mut self, from_undo: bool) {
        let (src, dst) = if from_undo { (&mut self.undo, &mut self.redo) } else { (&mut self.redo, &mut self.undo) };
        if let Some(s) = src.pop() {
            dst.push(Snapshot { lines: self.lines.clone(), cur: self.cur });
            self.lines = s.lines;
            self.cur = s.cur;
            self.anchor = None;
            self.last_op = None;
            self.follow = true;
            self.want_x = None;
        }
    }

    /// After an edit: 0 change → drop the step SNAPSHOT pushed (if it pushed 1).
    fn drop_noop(&mut self, pushed: bool) {
        if pushed && self.undo.last().is_some_and(|s| s.lines == self.lines) {
            self.undo.pop();
            self.last_op = None;
        }
    }

    // ---------- edits ----------
    fn insert_raw(&mut self, text: &str) {
        let text = text.replace("\r\n", "\n").replace('\r', "\n").replace('\t', "    ");
        let text = if self.single { text.replace('\n', " ") } else { text };
        let mut parts = text.split('\n');
        let tail: Vec<char> = self.lines[self.cur.row].split_off(self.cur.col);
        if let Some(first) = parts.next() {
            let chars: Vec<char> = first.chars().filter(|c| !c.is_control()).collect();
            self.cur.col += chars.len();
            self.lines[self.cur.row].extend(chars);
        }
        for part in parts {
            let chars: Vec<char> = part.chars().filter(|c| !c.is_control()).collect();
            self.cur.row += 1;
            self.cur.col = chars.len();
            self.lines.insert(self.cur.row, chars);
        }
        self.lines[self.cur.row].extend(tail);
    }

    pub fn insert(&mut self, text: &str) {
        let op = if text.chars().count() == 1 && !text.contains([' ', '\n']) { Op::Type } else { Op::Other };
        self.snapshot(op);
        self.delete_selection();
        self.insert_raw(text);
        self.follow = true;
        self.want_x = None;
    }

    /// Row ROW is inside a ``` fence (its own fence lines excluded).
    fn in_fence(&self, row: usize) -> bool {
        self.lines[..row].iter().filter(|l| is_fence(l)).count() % 2 == 1
    }

    /// A paste: code outside a fence goes in as a fenced block with its language tag (0 match: 0 tag).
    pub fn paste(&mut self, text: &str) {
        let text = text.replace("\r\n", "\n").replace('\r', "\n");
        if self.single || self.in_fence(self.cur.row) || !crate::code::looks_code(&text) {
            return self.insert(&text);
        }
        let row = &self.lines[self.cur.row];
        let before = if self.cur.col > 0 { "\n" } else { "" };
        let after = if self.cur.col < row.len() { "\n" } else { "" };
        let tag = crate::code::guess(&text).unwrap_or("");
        self.insert(&format!("{before}```{tag}\n{}\n```{after}", text.trim_end_matches('\n')));
    }

    /// Delete from the cursor to the start (END: the end) of its visual row. A selection: the selection.
    /// At the row's edge already: 1 char (joins the lines, like ⌘⌫ in macOS text fields).
    fn kill_to_edge(&mut self, end: bool) {
        if self.range().is_none() {
            let from = self.cur;
            self.line_edge(end, false);
            let to = self.cur;
            self.cur = from;
            if to == from {
                return if end { self.delete(false) } else { self.backspace(false) };
            }
            self.anchor = Some(to);
        }
        let pushed = self.snapshot(Op::Other);
        self.delete_selection();
        self.drop_noop(pushed);
    }

    /// Op of a backspace / delete: 1 char → Delete (runs coalesce), word / selection → Other.
    fn del_op(&self, word: bool) -> Op {
        if word || self.range().is_some() { Op::Other } else { Op::Delete }
    }

    fn backspace(&mut self, word: bool) {
        // Markdown list item, cursor right after its marker: 1 layer out (with text after the
        // cursor too); at indent 0 only an empty item (its marker goes), else 1 char as usual
        if self.range().is_none() && !self.single
            && let Some((indent, w, _)) = list_item(&self.lines[self.cur.row])
            && self.cur.col == indent + w
            && (indent > 0 || self.lines[self.cur.row].len() == indent + w)
        {
            self.snapshot(Op::Other);
            self.outdent(indent, w);
            self.follow = true;
            return;
        }
        if self.range().is_none() {
            let row = self.cur.row;
            if let Some((a, b)) = self.atomic_ranges(row).into_iter().find(|&(a, b)| {
                self.cur.col > a && self.cur.col <= b
            }) {
                self.anchor = Some(Pos { row, col: a });
                self.cur.col = self.cur.col.max(b);
            }
        }
        let pushed = self.snapshot(self.del_op(word));
        if !self.delete_selection() {
            let end = self.cur;
            let start = if word { self.string_left(end) } else { self.left_of(end) };
            if start != end {
                self.delete_range(start, end);
            }
        }
        self.drop_noop(pushed);
        self.follow = true;
    }

    fn delete(&mut self, word: bool) {
        if self.range().is_none()
            && let Some((a, b)) = self.atomic_ranges(self.cur.row).into_iter().find(|&(a, b)| self.cur.col >= a && self.cur.col < b)
        {
            self.anchor = Some(Pos { row: self.cur.row, col: b });
            self.cur.col = a;
        }
        let pushed = self.snapshot(self.del_op(word));
        if !self.delete_selection() {
            let start = self.cur;
            let end = if word { self.word_right(start) } else { self.right_of(start) };
            if start != end {
                self.delete_range(start, end);
            }
        }
        self.drop_noop(pushed);
        self.follow = true;
    }

    // ---------- movement ----------
    fn left_of(&self, p: Pos) -> Pos {
        if p.col > 0 {
            Pos { row: p.row, col: p.col - 1 }
        } else if p.row > 0 {
            Pos { row: p.row - 1, col: self.lines[p.row - 1].len() }
        } else {
            p
        }
    }

    fn right_of(&self, p: Pos) -> Pos {
        if p.col < self.lines[p.row].len() {
            Pos { row: p.row, col: p.col + 1 }
        } else if p.row + 1 < self.lines.len() {
            Pos { row: p.row + 1, col: 0 }
        } else {
            p
        }
    }

    /// Start of what a word delete (⌥⌫, ctrl+⌫, ctrl+w) takes left of P: the string back to the
    /// whitespace before it; in a path (a `/` in the string) 1 segment, its `/` stays
    /// (`cd ~/a/bc` → `cd ~/a/`, again → `cd ~/`).
    fn string_left(&self, p: Pos) -> Pos {
        if p.col == 0 {
            return self.left_of(p);
        }
        let l = &self.lines[p.row];
        let mut c = p.col;
        while c > 0 && l[c - 1].is_whitespace() {
            c -= 1;
        }
        // the slash(es) after a segment go with it
        while c > 0 && l[c - 1] == '/' {
            c -= 1;
        }
        while c > 0 && !l[c - 1].is_whitespace() && l[c - 1] != '/' {
            c -= 1;
        }
        Pos { row: p.row, col: c }
    }

    fn word_left(&self, p: Pos) -> Pos {
        if p.col == 0 {
            return self.left_of(p);
        }
        let l = &self.lines[p.row];
        let mut c = p.col;
        while c > 0 && !is_word(l[c - 1]) {
            c -= 1;
        }
        while c > 0 && is_word(l[c - 1]) {
            c -= 1;
        }
        Pos { row: p.row, col: c }
    }

    fn word_right(&self, p: Pos) -> Pos {
        let l = &self.lines[p.row];
        if p.col >= l.len() {
            return self.right_of(p);
        }
        let mut c = p.col;
        while c < l.len() && !is_word(l[c]) {
            c += 1;
        }
        while c < l.len() && is_word(l[c]) {
            c += 1;
        }
        Pos { row: p.row, col: c }
    }

    fn move_to(&mut self, p: Pos, select: bool) {
        let p = if select {
            self.atomic_ranges(p.row).into_iter().find(|&(a,b)| p.col > a && p.col < b)
                .map_or(p, |(a,b)| Pos { row: p.row, col: if p < self.cur { a } else { b } })
        } else { p };
        if !select && self.range().is_none()
            && let Some((a, b)) = self.atomic_ranges(p.row).into_iter().find(|&(a, b)| {
                (p.col > a && p.col < b) || (p < self.cur && p.col == b) || (p > self.cur && p.col == a)
            })
        {
            let (anchor, cur) = if p < self.cur { (b, a) } else { (a, b) };
            self.anchor = Some(Pos { row: p.row, col: anchor });
            self.cur = Pos { row: p.row, col: cur };
            self.follow = true;
            self.last_op = None;
            return;
        }
        if select {
            if self.anchor.is_none() {
                self.anchor = Some(self.cur);
            }
        } else {
            self.anchor = None;
        }
        self.cur = p;
        self.follow = true;
        self.last_op = None;
    }

    fn atomic_ranges(&self, row: usize) -> Vec<(usize, usize)> {
        static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
        let re = RE.get_or_init(|| regex::Regex::new(r"\[(?:Image #[0-9]+|Pasted text #[0-9]+ \+[0-9]+ lines)\]").expect("valid regex"));
        let text: String = self.lines[row].iter().collect();
        re.find_iter(&text).map(|m| (text[..m.start()].chars().count(), text[..m.end()].chars().count())).collect()
    }

    /// Rebuild the visual rows from the text at the last render width. An edit + ↑/↓ in 1 event
    /// batch reads the rows before the next render: stale rows point past the text.
    fn layout(&mut self) {
        if self.area.width == 0 {
            return;
        }
        let width = self.area.width.saturating_sub(1) as usize;
        self.vis.clear();
        for (row, l) in self.lines.iter().enumerate() {
            let segs = wrap(l, width);
            let n = segs.len();
            for (i, (start, end)) in segs.into_iter().enumerate() {
                self.vis.push(Seg { row, start, end, last: i + 1 == n });
            }
        }
    }

    /// Visual row index + x of a position.
    fn visual_of(&self, p: Pos) -> (usize, usize) {
        let Some(line) = self.lines.get(p.row) else { return (0, 0) };
        if p.col > line.len() {
            return (0, 0);
        }
        for (i, s) in self.vis.iter().enumerate() {
            if s.row == p.row && s.start <= line.len() &&p.col >= s.start && (p.col < s.end || (s.last && p.col == s.end)) {
                return (i, width_of(&self.lines[s.row][s.start..p.col]));
            }
        }
        (0, 0)
    }

    /// Position at visual row VROW, display column X.
    fn pos_at(&self, vrow: usize, x: usize) -> Pos {
        let Some(s) = self.vis.get(vrow.min(self.vis.len().saturating_sub(1))) else { return Pos::default() };
        let Some(line) = self.lines.get(s.row) else { return Pos::default() };
        let (end, mut col, mut w) = (s.end.min(line.len()), s.start.min(line.len()), 0);
        while col < end {
            let c = cw(line[col]);
            if w + c > x {
                // click on the right half of a wide char → after it
                if c > 1 && x >= w + c / 2 {
                    col += 1;
                }
                break;
            }
            w += c;
            col += 1;
        }
        if !s.last && col == s.end && col > s.start {
            col -= 1;
        }
        Pos { row: s.row, col }
    }

    fn vertical(&mut self, delta: isize, select: bool) {
        self.layout();
        if self.vis.is_empty() {
            return;
        }
        let (vrow, x) = self.visual_of(self.cur);
        let x = *self.want_x.get_or_insert(x);
        let target = vrow as isize + delta;
        let p = if target < 0 {
            Pos { row: 0, col: 0 }
        } else if target as usize >= self.vis.len() {
            let r = self.lines.len() - 1;
            Pos { row: r, col: self.lines[r].len() }
        } else {
            self.pos_at(target as usize, x)
        };
        let keep = self.want_x;
        self.move_to(p, select);
        self.want_x = keep;
    }

    fn line_edge(&mut self, end: bool, select: bool) {
        self.layout();
        let (vrow, _) = self.visual_of(self.cur);
        let p = match self.vis.get(vrow) {
            Some(s) if end => Pos { row: s.row, col: if s.last { s.end } else { s.end.saturating_sub(1).max(s.start) } },
            Some(s) => Pos { row: s.row, col: s.start },
            None => self.cur,
        };
        self.move_to(p, select);
    }

    fn select_word(&mut self, p: Pos) {
        let l = &self.lines[p.row];
        let (mut a, mut b) = (p.col, p.col);
        let class = |c: char| if is_word(c) { 0 } else if c == ' ' { 1 } else { 2 };
        let k = l.get(p.col).or(p.col.checked_sub(1).and_then(|i| l.get(i))).map(|&c| class(c));
        if let Some(k) = k {
            while a > 0 && class(l[a - 1]) == k {
                a -= 1;
            }
            while b < l.len() && class(l[b]) == k {
                b += 1;
            }
        }
        self.anchor = Some(Pos { row: p.row, col: a });
        self.cur = Pos { row: p.row, col: b };
    }

    pub fn select_all(&mut self) {
        self.anchor = Some(Pos::default());
        let r = self.lines.len() - 1;
        self.cur = Pos { row: r, col: self.lines[r].len() };
    }

    // ---------- input ----------
    pub fn key(&mut self, k: KeyEvent) -> Outcome {
        self.key_at(k, KEY_AT.with(|c| c.get()).unwrap_or_else(Instant::now))
    }

    /// Enter: a new line. In a Markdown list item: the next item at the same layer (same indent,
    /// same marker; numbers count up). An empty item: 1 layer out (the 1st layer: the marker goes).
    pub fn newline(&mut self) {
        if self.single {
            return;
        }
        self.snapshot(Op::Other);
        self.delete_selection();
        let Pos { row, col } = self.cur;
        match list_item(&self.lines[row]) {
            Some((indent, w, _)) if self.lines[row].len() == indent + w && col == indent + w => self.outdent(indent, w),
            Some((indent, w, next)) if col >= indent + w => self.insert_raw(&format!("\n{}{next}", " ".repeat(indent))),
            _ => self.insert_raw("\n"),
        }
        self.follow = true;
        self.want_x = None;
    }

    /// Space on a Markdown list item, cursor right after its marker (empty item or text after the
    /// cursor): 1 layer in (2 spaces of indent), the counterpart of backspace. false: not such an
    /// item, the space goes in as text.
    fn indent_item(&mut self) -> bool {
        if self.single || self.range().is_some() {
            return false;
        }
        let row = self.cur.row;
        let Some((indent, w, _)) = list_item(&self.lines[row]) else { return false };
        if self.cur.col != indent + w {
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
    fn outdent(&mut self, indent: usize, w: usize) {
        // a click leaves anchor = cursor (0 selection): past the new line end after the drain
        self.anchor = None;
        let row = self.cur.row;
        if indent == 0 {
            self.lines[row].drain(..w);
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
    fn undo_auto_period(&mut self, now: Instant) {
        let h = &self.recent;
        let n = h.len();
        if n < 3 || h[n - 1].1 != KeyCode::Char(' ') || h[n - 2].1 != KeyCode::Char('.') || h[n - 1].0 - h[n - 2].0 > BURST {
            return;
        }
        let near = |i: usize| h[i + 1].0 - h[i].0 <= BURST;
        let erased = h[n - 3].1 == KeyCode::Backspace && near(n - 3) && n >= 4 && h[n - 4].1 == KeyCode::Char(' ');
        if !erased && h[n - 3].1 != KeyCode::Char(' ') {
            return;
        }
        // the 2nd space came as a key too, right before the burst (space space `.` space): `.` + space go
        let doubled = !erased && n >= 4 && h[n - 4].1 == KeyCode::Char(' ') && h[n - 2].0 - h[n - 3].0 <= SAME_PRESS;
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

    fn key_at(&mut self, k: KeyEvent, now: Instant) -> Outcome {
        keylog(&k, now);
        let space = k.code == KeyCode::Char(' ') && k.modifiers.difference(KeyModifiers::SHIFT).is_empty();
        // the 2nd space as a key right after the burst: the 2 spaces are in already
        if space && self.period_undone.take().is_some_and(|t| now - t <= SAME_PRESS) {
            return Outcome::Handled;
        }
        self.period_undone = None;
        self.recent.push((now, k.code));
        if self.recent.len() > 5 {
            self.recent.remove(0);
        }
        let out = self.key_inner(k);
        if space {
            self.undo_auto_period(now);
        }
        out
    }

    fn key_inner(&mut self, k: KeyEvent) -> Outcome {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        let alt = k.modifiers.contains(KeyModifiers::ALT);
        let shift = k.modifiers.contains(KeyModifiers::SHIFT);
        // ⌘ (kitty protocol: super) = ctrl for the edit keys (undo, redo, all, copy, cut, paste)
        let sup = k.modifiers.contains(KeyModifiers::SUPER);
        let page = (self.area.height.max(2) - 1) as isize;
        if !matches!(k.code, KeyCode::Up | KeyCode::Down | KeyCode::PageUp | KeyCode::PageDown) {
            self.want_x = None;
        }
        match k.code {
            KeyCode::Char(c) if ctrl || sup => match c.to_ascii_lowercase() {
                'a' if shift || sup => self.select_all(),
                // ⌘← / ⌘→ in macOS terminals arrive as ctrl+a / ctrl+e (readline): row start / end
                'a' => self.line_edge(false, false),
                'c' => {
                    if let Some(t) = self.selected_text() {
                        clip::copy(&t);
                    }
                }
                'x' => {
                    if let Some(t) = self.selected_text() {
                        clip::copy(&t);
                        self.snapshot(Op::Other);
                        self.delete_selection();
                    }
                }
                'v' => {
                    if let Some(t) = clip::paste() {
                        self.insert(&t);
                    }
                }
                'z' if shift || c == 'Z' => self.restore(false),
                'z' => self.restore(true),
                'y' => self.restore(false),
                // readline keys: ctrl only (⌘ + these = 0 effect)
                'e' | 'w' | 'u' | 'k' | 'h' if !ctrl => return Outcome::Ignored,
                'e' => self.line_edge(true, false),
                'w' => self.backspace(true),
                // ctrl+⌫ in terminals without the kitty protocol: ^H
                'h' => self.backspace(true),
                // ⌘⌫ in macOS terminals (ctrl+u, readline): delete to the start of the row; ctrl+k: to its end
                'u' => self.kill_to_edge(false),
                'k' => self.kill_to_edge(true),
                _ => return Outcome::Ignored,
            },
            // macOS terminals send option+←/→ as ESC b / ESC f (meta), option+⌫ as ESC DEL, option+fn+⌫ as ESC d
            KeyCode::Char('b') if alt => self.move_to(self.word_left(self.cur), shift),
            KeyCode::Char('f') if alt => self.move_to(self.word_right(self.cur), shift),
            KeyCode::Char('d') if alt => self.delete(true),
            KeyCode::Char('B') if alt => self.move_to(self.word_left(self.cur), true),
            KeyCode::Char('F') if alt => self.move_to(self.word_right(self.cur), true),
            KeyCode::Char(' ') if !alt && self.indent_item() => {}
            KeyCode::Char(c) if !alt => self.insert(&c.to_string()),
            KeyCode::Enter if !self.single && !ctrl && !alt => self.newline(),
            // the row: ⌘ (kitty protocol: super), alt off macOS. ⌫: from the cursor to the row's
            // start, ←→: to the row's start / end
            KeyCode::Backspace if line_mod(k.modifiers) => self.kill_to_edge(false),
            KeyCode::Delete if line_mod(k.modifiers) => self.kill_to_edge(true),
            KeyCode::Left if line_mod(k.modifiers) => self.line_edge(false, shift),
            KeyCode::Right if line_mod(k.modifiers) => self.line_edge(true, shift),
            // the string: ctrl, ⌥ (= alt on macOS). ⌫: the string left of the cursor (in a path:
            // 1 segment), ←→: word jumps
            KeyCode::Backspace => self.backspace(ctrl || alt),
            KeyCode::Delete => self.delete(ctrl || alt),
            KeyCode::Left if ctrl || alt => self.move_to(self.word_left(self.cur), shift),
            KeyCode::Right if ctrl || alt => self.move_to(self.word_right(self.cur), shift),
            KeyCode::Left => match (shift, self.range()) {
                (false, Some((a, _))) => self.move_to(a, false),
                _ => self.move_to(self.left_of(self.cur), shift),
            },
            KeyCode::Right => match (shift, self.range()) {
                (false, Some((_, b))) => self.move_to(b, false),
                _ => self.move_to(self.right_of(self.cur), shift),
            },
            KeyCode::Up if !self.single => self.vertical(-1, shift),
            KeyCode::Down if !self.single => self.vertical(1, shift),
            KeyCode::PageUp if !self.single => self.vertical(-page, shift),
            KeyCode::PageDown if !self.single => self.vertical(page, shift),
            KeyCode::Home if ctrl => self.move_to(Pos::default(), shift),
            KeyCode::End if ctrl => {
                let r = self.lines.len() - 1;
                self.move_to(Pos { row: r, col: self.lines[r].len() }, shift)
            }
            KeyCode::Home => self.line_edge(false, shift),
            KeyCode::End => self.line_edge(true, shift),
            _ => return Outcome::Ignored,
        }
        Outcome::Handled
    }

    /// Mouse event; false = outside the input (and no drag running).
    pub fn mouse(&mut self, m: MouseEvent) -> bool {
        let inside = self.area.contains(Position { x: m.column, y: m.row });
        let at = |s: &TextArea| {
            let y = (m.row as isize - s.area.y as isize).clamp(0, s.area.height.saturating_sub(1) as isize) as usize;
            let x = m.column.saturating_sub(s.area.x) as usize;
            s.pos_at(s.scroll + y, x)
        };
        match m.kind {
            MouseEventKind::Down(MouseButton::Left) if inside => {
                let p = at(self);
                if m.modifiers.contains(KeyModifiers::SHIFT) {
                    self.move_to(p, true);
                    return true;
                }
                let n = match self.clicks {
                    Some((t, q, n)) if q == p && t.elapsed() < Duration::from_millis(400) => n % 3 + 1,
                    _ => 1,
                };
                self.clicks = Some((Instant::now(), p, n));
                self.last_op = None;
                self.want_x = None;
                match n {
                    2 => self.select_word(p),
                    3 => {
                        self.anchor = Some(Pos { row: p.row, col: 0 });
                        self.cur = Pos { row: p.row, col: self.lines[p.row].len() };
                    }
                    _ => {
                        self.move_to(p, false);
                        if self.range().is_none() { self.anchor = Some(p); }
                    }
                }
                self.drag = n == 1;
                self.follow = true;
                true
            }
            MouseEventKind::Drag(MouseButton::Left) if self.drag => {
                if m.row < self.area.y {
                    self.scroll = self.scroll.saturating_sub(1);
                } else if m.row >= self.area.y + self.area.height {
                    self.scroll = (self.scroll + 1).min(self.vis.len().saturating_sub(1));
                }
                self.cur = at(self);
                self.follow = true;
                true
            }
            MouseEventKind::Up(MouseButton::Left) if self.drag || inside => {
                self.drag = false;
                if let Some(t) = self.selected_text()
                    && clip::copy(&t)
                {
                    self.copied = Some(t.chars().count());
                }
                true
            }
            MouseEventKind::ScrollUp if inside => {
                self.scroll = self.scroll.saturating_sub(3);
                self.follow = false;
                true
            }
            MouseEventKind::ScrollDown if inside => {
                let max = self.vis.len().saturating_sub(self.area.height as usize);
                self.scroll = (self.scroll + 3).min(max);
                self.follow = false;
                true
            }
            _ => false,
        }
    }

    #[cfg(test)]
    fn render_for_test(&mut self, width: u16) {
        let backend = ratatui::backend::TestBackend::new(width, 5);
        let mut term = ratatui::Terminal::new(backend).unwrap();
        term.draw(|f| self.render(f, Rect::new(0, 0, width, 5), false)).unwrap();
    }

    // ---------- render ----------
    pub fn render(&mut self, f: &mut Frame, area: Rect, focused: bool) {
        self.area = area;
        self.vis.clear();
        self.layout();
        let width = area.width.saturating_sub(1) as usize;
        let h = area.height as usize;
        let (crow, cx) = self.visual_of(self.cur);
        if self.follow {
            if crow < self.scroll {
                self.scroll = crow;
            } else if crow >= self.scroll + h {
                self.scroll = crow + 1 - h;
            }
        }
        self.scroll = self.scroll.min(self.vis.len().saturating_sub(1));
        let base = self.style.unwrap_or_else(theme::panel);
        f.render_widget(Paragraph::new("").style(base), area);
        if self.is_empty() && !self.placeholder.is_empty() {
            f.render_widget(Paragraph::new(self.placeholder.as_str()).style(base.fg(theme::pal().muted)), area);
        } else {
            let sel = self.range();
            let sel_style = Style::default().bg(theme::pal().accent).fg(theme::pal().on_accent);
            let mut lines = Vec::new();
            let (code, fence) = if self.single { (Vec::new(), Vec::new()) } else { fence_colors(&self.lines) };
            for s in self.vis.iter().skip(self.scroll).take(h) {
                let line = &self.lines[s.row];
                let colors = mention_colors(line, &self.marks);
                let code_row = code.get(s.row).and_then(|c| c.as_ref());
                let fence_row = fence.get(s.row).copied().unwrap_or(false);
                // `- ` / `* ` / `+ ` items outside code show as `•` (the text keeps the marker)
                let bullet = (code_row.is_none() && !fence_row && !self.single)
                    .then(|| list_item(line).filter(|(_, w, _)| *w <= 6 && matches!(line.get(line.iter().take_while(|c| **c == ' ').count()), Some('-' | '*' | '+'))).map(|(indent, _, _)| indent))
                    .flatten();
                let mut spans: Vec<Span> = Vec::new();
                let (mut buf, mut buf_style) = (String::new(), base);
                for (col, &ch) in line.iter().enumerate().take(s.end).skip(s.start) {
                    let p = Pos { row: s.row, col };
                    let st = if sel.is_some_and(|(a, b)| p >= a && p < b) {
                        sel_style
                    } else if let Some(c) = colors[col] {
                        base.fg(c).add_modifier(Modifier::BOLD)
                    } else if let Some(c) = code_row.and_then(|r| r.get(col)) {
                        base.fg(*c)
                    } else if fence_row {
                        base.fg(theme::pal().muted)
                    } else {
                        base
                    };
                    if st != buf_style && !buf.is_empty() {
                        spans.push(Span::styled(std::mem::take(&mut buf), buf_style));
                    }
                    buf_style = st;
                    buf.push(if bullet == Some(col) { '•' } else { ch });
                }
                // a selected line break shows as 1 selected cell
                let nl_selected = s.last && sel.is_some_and(|(a, b)| Pos { row: s.row, col: s.end } >= a && Pos { row: s.row, col: s.end } < b);
                if !buf.is_empty() {
                    spans.push(Span::styled(buf, buf_style));
                }
                if nl_selected {
                    spans.push(Span::styled(" ", sel_style));
                }
                lines.push(Line::from(spans));
            }
            f.render_widget(Paragraph::new(lines).style(base), area);
        }
        if focused && crow >= self.scroll && crow < self.scroll + h {
            f.set_cursor_position(Position { x: area.x + cx.min(width) as u16, y: area.y + (crow - self.scroll) as u16 });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ta(text: &str) -> TextArea {
        let mut t = TextArea::new(false, "");
        t.set_text(text);
        t
    }

    #[test]
    fn image_marker_backspace_is_atomic_after_paste_and_undo_restores_it() {
        let mut t = ta("");
        t.insert("日本 [Image #1] ");
        // the space after the marker: 1 char, the marker stays
        t.backspace(false);
        assert_eq!(t.text(), "日本 [Image #1]");
        t.backspace(false);
        assert_eq!(t.text(), "日本 ");
        t.restore(true);
        assert_eq!(t.text(), "日本 [Image #1]");
        t = ta("日本 [Image #1] ");
        t.cur = Pos { row: 0, col: 7 };
        t.anchor = None;
        t.backspace(false);
        assert_eq!(t.text(), "日本  ");
    }

    #[test]
    fn arrows_and_shift_arrows_select_complete_image_markers() {
        let mut t = ta("a [Image #2] z");
        t.cur = Pos { row: 0, col: 2 };
        t.key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
        assert_eq!(t.selected_text().as_deref(), Some("[Image #2]"));
        t.key(KeyEvent::new(KeyCode::Delete, KeyModifiers::NONE));
        assert_eq!(t.text(), "a  z");
        t.restore(true);
        t.cur = Pos { row: 0, col: 12 }; t.anchor = None;
        t.key(KeyEvent::new(KeyCode::Left, KeyModifiers::SHIFT));
        assert_eq!(t.selected_text().as_deref(), Some("[Image #2]"));
    }

    #[test]
    fn delete_inside_a_marker_removes_the_whole_marker() {
        let mut t = ta("[Image #3]");
        t.cur = Pos { row: 0, col: 4 };
        t.delete(false);
        assert_eq!(t.text(), "");
        t.restore(true);
        assert_eq!(t.text(), "[Image #3]");
    }

    #[test]
    fn arrows_after_an_edit_without_a_render_read_fresh_rows() {
        // 1 event batch: the text shrinks, then ↑ / ↓ / end before the next render
        let mut t = ta("first line\nsecond line that is long\nthird");
        t.render_for_test(12);
        t.set_text("x");
        for code in [KeyCode::Up, KeyCode::Down, KeyCode::End, KeyCode::Home] {
            t.key(KeyEvent::new(code, KeyModifiers::NONE));
        }
        assert_eq!(t.text(), "x");
        assert!(t.cur.row == 0 && t.cur.col <= 1);
    }

    #[test]
    fn wrap_breaks_after_spaces() {
        let l: Vec<char> = "hello world foo".chars().collect();
        assert_eq!(wrap(&l, 11), vec![(0, 6), (6, 15)]);
        assert_eq!(wrap(&l, 8), vec![(0, 6), (6, 12), (12, 15)]);
        let long: Vec<char> = "abcdefgh".chars().collect();
        assert_eq!(wrap(&long, 3), vec![(0, 3), (3, 6), (6, 8)]);
    }

    #[test]
    fn backspace_removes_whole_selection() {
        let mut t = ta("one\ntwo three");
        t.anchor = Some(Pos { row: 0, col: 1 });
        t.cur = Pos { row: 1, col: 3 };
        assert_eq!(t.selected_text().unwrap(), "ne\ntwo");
        t.backspace(false);
        assert_eq!(t.text(), "o three");
        t.restore(true);
        assert_eq!(t.text(), "one\ntwo three");
    }

    #[test]
    fn cmd_backspace_deletes_to_row_start() {
        let mut t = ta("one two\nthree four");
        t.render_for_test(40);
        t.cur = Pos { row: 1, col: 5 };
        t.kill_to_edge(false);
        assert_eq!(t.text(), "one two\n four");
        // at the row start: joins with the line above
        t.kill_to_edge(false);
        assert_eq!(t.text(), "one two four");
        t.restore(true);
        assert_eq!(t.text(), "one two\n four");
        t.cur = Pos { row: 0, col: 3 };
        t.kill_to_edge(true);
        assert_eq!(t.text(), "one\n four");
    }

    #[test]
    fn double_space_never_adds_a_period() {
        let k = |c: KeyCode| KeyEvent::new(c, KeyModifiers::NONE);
        let t0 = Instant::now();
        let ms = |n: u64| t0 + Duration::from_millis(n);
        // macOS: 2nd space → `.` + space in 1 burst
        let mut t = ta("word");
        t.move_end();
        t.key_at(k(KeyCode::Char(' ')), ms(0));
        t.key_at(k(KeyCode::Char('.')), ms(150));
        t.key_at(k(KeyCode::Char(' ')), ms(151));
        assert_eq!(t.text(), "word  ");
        // terminal variant: ⌫ `.` space
        let mut t = ta("word");
        t.move_end();
        t.key_at(k(KeyCode::Char(' ')), ms(0));
        t.key_at(k(KeyCode::Backspace), ms(150));
        t.key_at(k(KeyCode::Char('.')), ms(150));
        t.key_at(k(KeyCode::Char(' ')), ms(151));
        assert_eq!(t.text(), "word  ");
        // the 2nd space as a key, then `.` + space (2 writes of 1 key press): 2 spaces, not 3
        let mut t = ta("word");
        t.move_end();
        t.key_at(k(KeyCode::Char(' ')), ms(0));
        t.key_at(k(KeyCode::Char(' ')), ms(150));
        t.key_at(k(KeyCode::Char('.')), ms(170));
        t.key_at(k(KeyCode::Char(' ')), ms(170));
        assert_eq!(t.text(), "word  ");
        // `.` + space, then the 2nd space as a key: 2 spaces, not 3
        let mut t = ta("word");
        t.move_end();
        t.key_at(k(KeyCode::Char(' ')), ms(0));
        t.key_at(k(KeyCode::Char('.')), ms(150));
        t.key_at(k(KeyCode::Char(' ')), ms(150));
        t.key_at(k(KeyCode::Char(' ')), ms(165));
        assert_eq!(t.text(), "word  ");
        // a 3rd space typed by the person: kept
        t.key_at(k(KeyCode::Char(' ')), ms(400));
        assert_eq!(t.text(), "word   ");
        // a person typing " . ": kept
        let mut t = ta("a");
        t.move_end();
        t.key_at(k(KeyCode::Char(' ')), ms(0));
        t.key_at(k(KeyCode::Char('.')), ms(120));
        t.key_at(k(KeyCode::Char(' ')), ms(240));
        assert_eq!(t.text(), "a . ");
        // end of a sentence: kept
        let mut t = ta("end");
        t.move_end();
        t.key_at(k(KeyCode::Char('.')), ms(0));
        t.key_at(k(KeyCode::Char(' ')), ms(1));
        assert_eq!(t.text(), "end. ");
    }

    #[test]
    fn sent_text_drops_trailing_empty_list_items() {
        // the owner's message: a stray `7. ` line an Enter left at the end
        assert_eq!(without_empty_items("points:\n7. set auto-allow\n8. yes, v1\n7. "), "points:\n7. set auto-allow\n8. yes, v1");
        assert_eq!(without_empty_items("a\n- x\n- \n  - [ ] \n\n"), "a\n- x");
        assert_eq!(without_empty_items("a\n3)"), "a");
        // items with text, plain lines, a lone marker as the whole message: kept
        assert_eq!(without_empty_items("a\n1. b  "), "a\n1. b");
        assert_eq!(without_empty_items("a\nv1."), "a\nv1.");
        assert_eq!(without_empty_items("-"), "-");
    }

    #[test]
    fn markdown_lists_continue_and_outdent() {
        let k = |c: KeyCode| KeyEvent::new(c, KeyModifiers::NONE);
        let mut t = ta("- a");
        t.move_end();
        t.key(k(KeyCode::Enter));
        assert_eq!(t.text(), "- a\n- ");
        let mut t = ta("- a\n  - b");
        t.move_end();
        t.key(k(KeyCode::Enter));
        assert_eq!(t.text(), "- a\n  - b\n  - ");
        // backspace on the empty item: 1 layer out, then the marker goes in 1 press
        t.key(k(KeyCode::Backspace));
        assert_eq!(t.text(), "- a\n  - b\n- ");
        t.key(k(KeyCode::Backspace));
        assert_eq!(t.text(), "- a\n  - b\n");
        // undo: 1 step per press
        t.key(KeyEvent::new(KeyCode::Char('z'), KeyModifiers::CONTROL));
        assert_eq!(t.text(), "- a\n  - b\n- ");
        // enter on an empty item: 1 layer out too
        let mut t = ta("    - ");
        t.move_end();
        t.key(k(KeyCode::Enter));
        assert_eq!(t.text(), "  - ");
        // numbers count up, check boxes stay open, text after the cursor moves along
        let mut t = ta("9. x");
        t.move_end();
        t.key(k(KeyCode::Enter));
        assert_eq!(t.text(), "9. x\n10. ");
        let mut t = ta("* [x] done");
        t.move_end();
        t.newline();
        assert_eq!(t.text(), "* [x] done\n* [ ] ");
        let mut t = ta("- ab");
        t.move_end();
        t.key(k(KeyCode::Left));
        t.key(k(KeyCode::Enter));
        assert_eq!(t.text(), "- a\n- b");
        // plain text, a cursor inside the marker, `-x`: plain new line
        for (text, want) in [("ab", "ab\n"), ("-x", "-x\n")] {
            let mut t = ta(text);
            t.move_end();
            t.key(k(KeyCode::Enter));
            assert_eq!(t.text(), want);
        }
        let mut t = ta("- a");
        t.move_start();
        t.key(k(KeyCode::Enter));
        assert_eq!(t.text(), "\n- a");
        // backspace after text of an item: 1 char
        let mut t = ta("- a");
        t.move_end();
        t.key(k(KeyCode::Backspace));
        assert_eq!(t.text(), "- ");
        // backspace on an empty item after a click (anchor = cursor): 0 panic
        let mut t = ta("- a\n- ");
        t.move_end();
        t.anchor = Some(t.cur);
        t.key(k(KeyCode::Backspace));
        t.key(k(KeyCode::Char('x')));
        assert_eq!(t.text(), "- a\nx");
    }

    #[test]
    fn space_on_empty_list_item_indents() {
        let k = |c: KeyCode| KeyEvent::new(c, KeyModifiers::NONE);
        let mut t = ta("- a");
        t.move_end();
        t.key(k(KeyCode::Enter));
        // space on the empty item: 1 layer in, backspace: 1 layer out again
        t.key(k(KeyCode::Char(' ')));
        assert_eq!(t.text(), "- a\n  - ");
        t.key(k(KeyCode::Char(' ')));
        assert_eq!(t.text(), "- a\n    - ");
        t.key(k(KeyCode::Backspace));
        assert_eq!(t.text(), "- a\n  - ");
        // the next key goes after the marker
        t.key(k(KeyCode::Char('b')));
        assert_eq!(t.text(), "- a\n  - b");
        // undo: the char, then the indent
        t.key(KeyEvent::new(KeyCode::Char('z'), KeyModifiers::CONTROL));
        assert_eq!(t.text(), "- a\n  - ");
        // numbers + check boxes too
        let mut t = ta("1. ");
        t.move_end();
        t.key(k(KeyCode::Char(' ')));
        assert_eq!(t.text(), "  1. ");
        // an item with text, a cursor inside the marker: plain space
        let mut t = ta("- a");
        t.move_end();
        t.key(k(KeyCode::Char(' ')));
        assert_eq!(t.text(), "- a ");
        let mut t = ta("- ");
        t.move_start();
        t.key(k(KeyCode::Char(' ')));
        assert_eq!(t.text(), " - ");
    }

    #[test]
    fn space_and_backspace_right_after_the_marker_of_an_item_with_text() {
        let k = |c: KeyCode| KeyEvent::new(c, KeyModifiers::NONE);
        let undo = KeyEvent::new(KeyCode::Char('z'), KeyModifiers::CONTROL);
        let redo = KeyEvent::new(KeyCode::Char('y'), KeyModifiers::CONTROL);
        // cursor right after `- ` of `- foo`: space → 1 layer in, the cursor stays before `foo`
        let mut t = ta("- a\n- foo");
        t.move_end();
        for _ in 0..3 {
            t.key(k(KeyCode::Left));
        }
        t.key(k(KeyCode::Char(' ')));
        assert_eq!(t.text(), "- a\n  - foo");
        t.key(k(KeyCode::Char(' ')));
        assert_eq!(t.text(), "- a\n    - foo");
        t.key(k(KeyCode::Char('x')));
        assert_eq!(t.text(), "- a\n    - xfoo");
        t.key(k(KeyCode::Backspace));
        // backspace right after the marker: 1 layer out
        t.key(k(KeyCode::Backspace));
        assert_eq!(t.text(), "- a\n  - foo");
        t.key(k(KeyCode::Backspace));
        assert_eq!(t.text(), "- a\n- foo");
        // indent 0 + text: backspace as before (1 char)
        t.key(k(KeyCode::Backspace));
        assert_eq!(t.text(), "- a\n-foo");
        // undo: 1 step per press, back through the layers; redo forward
        t.key(undo);
        assert_eq!(t.text(), "- a\n- foo");
        t.key(undo);
        assert_eq!(t.text(), "- a\n  - foo");
        t.key(undo);
        assert_eq!(t.text(), "- a\n    - foo");
        t.key(redo);
        assert_eq!(t.text(), "- a\n  - foo");
        // numbers and check boxes: right after the whole marker
        let mut t = ta("1. one");
        t.move_start();
        for _ in 0..3 {
            t.key(k(KeyCode::Right));
        }
        t.key(k(KeyCode::Char(' ')));
        assert_eq!(t.text(), "  1. one");
        let mut t = ta("* [ ] task");
        t.move_start();
        for _ in 0..6 {
            t.key(k(KeyCode::Right));
        }
        t.key(k(KeyCode::Char(' ')));
        assert_eq!(t.text(), "  * [ ] task");
        t.key(k(KeyCode::Backspace));
        assert_eq!(t.text(), "* [ ] task");
        // inside the text: plain space / backspace
        let mut t = ta("  - foo");
        t.move_end();
        t.key(k(KeyCode::Left));
        t.key(k(KeyCode::Char(' ')));
        assert_eq!(t.text(), "  - fo o");
        t.key(k(KeyCode::Backspace));
        assert_eq!(t.text(), "  - foo");
        // a single-line input: 0 list keys
        let mut t = TextArea::new(true, "");
        t.set_text("- foo");
        t.move_start();
        t.key(k(KeyCode::Right));
        t.key(k(KeyCode::Right));
        t.key(k(KeyCode::Char(' ')));
        assert_eq!(t.text(), "-  foo");
    }

    #[test]
    fn key_stamps_keep_a_slow_handled_burst_together() {
        let k = |c: KeyCode| KeyEvent::new(c, KeyModifiers::NONE);
        let t0 = Instant::now();
        let mut t = ta("word");
        t.move_end();
        stamp(Some(t0));
        t.key(k(KeyCode::Char(' ')));
        // the burst read at 150ms; handled later (slow dropdown work): the read time counts
        stamp(Some(t0 + Duration::from_millis(150)));
        t.key(k(KeyCode::Char('.')));
        std::thread::sleep(Duration::from_millis(20));
        stamp(Some(t0 + Duration::from_millis(151)));
        t.key(k(KeyCode::Char(' ')));
        stamp(None);
        assert_eq!(t.text(), "word  ");
    }

    #[test]
    fn mentions_get_the_agent_color() {
        let marks = vec![("amon".to_string(), Color::Red), ("lilith".to_string(), Color::Blue)];
        let l: Vec<char> = "ask @amon, (@lilith) mail@amon @amonx".chars().collect();
        let c = mention_colors(&l, &marks);
        let colored: String = l.iter().zip(&c).filter(|(_, c)| c.is_some()).map(|(ch, _)| *ch).collect();
        assert_eq!(colored, "@amon@lilith");
        assert_eq!(c[4], Some(Color::Red));
        assert_eq!(c[12], Some(Color::Blue));
        // a replace note: in the color of the new agent, @lilith in its own color
        let l: Vec<char> = "@amon (replaces @lilith) go".chars().collect();
        let c = mention_colors(&l, &marks);
        let want = |i: usize| if (16..23).contains(&i) { Color::Blue } else { Color::Red };
        assert!(c[..24].iter().enumerate().all(|(i, x)| i == 5 || *x == Some(want(i))), "{c:?}");
        assert_eq!(c[5], None);
        assert_eq!(c[24..], [None, None, None]);
    }

    #[test]
    fn mention_query_before_cursor() {
        let mut t = ta("hi @am");
        t.move_end();
        assert_eq!(t.mention(), Some((Pos { row: 0, col: 3 }, "am".into())));
        t.replace_to_cursor(Pos { row: 0, col: 3 }, "@amon ");
        assert_eq!(t.text(), "hi @amon ");
        assert_eq!(t.mention(), None);
        let mut t = ta("mail@x");
        t.move_end();
        assert_eq!(t.mention(), None);
        let mut t = ta("@");
        t.move_end();
        assert_eq!(t.mention(), Some((Pos { row: 0, col: 0 }, String::new())));
    }

    fn press(t: &mut TextArea, c: KeyCode, m: KeyModifiers) -> bool {
        matches!(t.key(KeyEvent::new(c, m)), Outcome::Handled)
    }

    fn typed(t: &mut TextArea, s: &str) {
        for c in s.chars() {
            press(t, KeyCode::Char(c), KeyModifiers::NONE);
        }
    }

    #[test]
    fn cmd_z_undoes_and_cmd_shift_z_redoes() {
        let sup = KeyModifiers::SUPER;
        let sup_shift = KeyModifiers::SUPER | KeyModifiers::SHIFT;
        let ctrl_shift = KeyModifiers::CONTROL | KeyModifiers::SHIFT;
        for (undo, redo) in [
            ((KeyCode::Char('z'), sup), (KeyCode::Char('z'), sup_shift)),
            ((KeyCode::Char('z'), sup), (KeyCode::Char('Z'), sup_shift)),
            ((KeyCode::Char('z'), KeyModifiers::CONTROL), (KeyCode::Char('Z'), ctrl_shift)),
            ((KeyCode::Char('z'), KeyModifiers::CONTROL), (KeyCode::Char('y'), KeyModifiers::CONTROL)),
        ] {
            let mut t = TextArea::new(false, "");
            typed(&mut t, "hi");
            press(&mut t, undo.0, undo.1);
            assert_eq!(t.text(), "");
            press(&mut t, redo.0, redo.1);
            assert_eq!(t.text(), "hi");
        }
    }

    #[test]
    fn cmd_char_never_types() {
        let mut t = ta("x");
        for c in ['q', 'e', 'w', 'u', 'k'] {
            assert!(!press(&mut t, KeyCode::Char(c), KeyModifiers::SUPER));
        }
        assert_eq!(t.text(), "x");
        press(&mut t, KeyCode::Char('a'), KeyModifiers::SUPER);
        assert_eq!(t.selected_text().as_deref(), Some("x"));
    }

    #[test]
    fn backspaces_undo_as_1_step() {
        let mut t = ta("hello world");
        t.move_end();
        for _ in 0..5 {
            press(&mut t, KeyCode::Backspace, KeyModifiers::NONE);
        }
        assert_eq!(t.text(), "hello ");
        press(&mut t, KeyCode::Char('z'), KeyModifiers::CONTROL);
        assert_eq!(t.text(), "hello world");
        assert!(t.undo.is_empty());
    }

    #[test]
    fn noop_backspace_adds_0_steps() {
        let mut t = TextArea::new(false, "");
        press(&mut t, KeyCode::Backspace, KeyModifiers::NONE);
        press(&mut t, KeyCode::Delete, KeyModifiers::NONE);
        press(&mut t, KeyCode::Char('u'), KeyModifiers::CONTROL);
        assert!(t.undo.is_empty());
    }

    #[test]
    fn set_text_resets_history() {
        let mut t = TextArea::new(false, "");
        typed(&mut t, "draft");
        t.set_text("");
        press(&mut t, KeyCode::Char('z'), KeyModifiers::CONTROL);
        assert_eq!(t.text(), "");
    }

    #[test]
    fn load_text_is_1_undo_step() {
        let mut t = TextArea::new(false, "");
        typed(&mut t, "draft");
        t.load_text("older prompt");
        typed(&mut t, "!");
        assert_eq!(t.text(), "older prompt!");
        press(&mut t, KeyCode::Char('z'), KeyModifiers::CONTROL);
        assert_eq!(t.text(), "older prompt");
        press(&mut t, KeyCode::Char('z'), KeyModifiers::CONTROL);
        assert_eq!(t.text(), "draft");
    }

    #[test]
    fn code_paste_gets_a_fence_with_its_tag_once() {
        let mut t = ta("see ");
        t.line_edge(true, false);
        t.paste("fn main() {\n    let v: Vec<u8> = vec![];\n}");
        assert_eq!(t.text(), "see \n```rs\nfn main() {\n    let v: Vec<u8> = vec![];\n}\n```");
        // inside the fence: as is
        let mut u = TextArea::new(false, "");
        u.insert("```\n");
        u.paste("fn a() {\n}");
        assert_eq!(u.text(), "```\nfn a() {\n}");
        // prose: as is; unknown code: 0 tag
        let mut p = TextArea::new(false, "");
        p.paste("hello there\nhow are you");
        assert_eq!(p.text(), "hello there\nhow are you");
        let mut q = TextArea::new(false, "");
        q.paste(";;;\n{}");
        assert_eq!(q.text(), "```\n;;;\n{}\n```");
    }

    #[test]
    fn paste_replaces_selection_and_keeps_lines() {
        let mut t = ta("abc");
        t.select_all();
        t.insert("x\r\ny");
        assert_eq!(t.text(), "x\ny");
        let mut s = TextArea::new(true, "");
        s.insert("a\nb");
        assert_eq!(s.text(), "a b");
    }

    /// ⌥⌫ / ctrl+⌫ / ctrl+h / ctrl+w: the string left of the cursor; in a path 1 segment.
    /// ⌘⌫ (alt+⌫ off macOS): to the row's start. ⌘←→ (alt off macOS): row start / end.
    #[test]
    fn word_and_row_deletes_in_every_terminal_encoding() {
        let word = [
            (KeyCode::Backspace, KeyModifiers::CONTROL),
            (KeyCode::Char('h'), KeyModifiers::CONTROL),
            (KeyCode::Char('w'), KeyModifiers::CONTROL),
        ];
        for (c, m) in word {
            let mut t = ta("git commit -m foo.bar-baz");
            t.move_end();
            press(&mut t, c, m);
            assert_eq!(t.text(), "git commit -m ", "{c:?} {m:?}");
            // a path: 1 segment, its `/` stays
            let mut t = ta("cd ~/projects/code-head");
            t.move_end();
            press(&mut t, c, m);
            assert_eq!(t.text(), "cd ~/projects/");
            press(&mut t, c, m);
            assert_eq!(t.text(), "cd ~/");
            press(&mut t, c, m);
            assert_eq!(t.text(), "cd ");
        }
        // ⌥ (alt) on macOS: the string; off macOS: the row
        let mut t = ta("one two-three");
        t.move_end();
        press(&mut t, KeyCode::Backspace, KeyModifiers::ALT);
        assert_eq!(t.text(), if cfg!(target_os = "macos") { "one " } else { "" });
        // ⌘⌫ (super) and ctrl+u (⌘⌫ of macOS terminals): to the row's start
        for (c, m) in [(KeyCode::Backspace, KeyModifiers::SUPER), (KeyCode::Char('u'), KeyModifiers::CONTROL)] {
            let mut t = ta("first\nsecond row");
            t.render_for_test(40);
            t.move_end();
            press(&mut t, c, m);
            assert_eq!(t.text(), "first\n", "{c:?}");
        }
        // ⌘← row start, ⌘→ row end; ctrl+← a word
        let mut t = ta("alpha beta gamma");
        t.render_for_test(40);
        t.move_end();
        press(&mut t, KeyCode::Left, KeyModifiers::SUPER);
        assert_eq!(t.cur.col, 0);
        press(&mut t, KeyCode::Right, KeyModifiers::SUPER);
        assert_eq!(t.cur.col, 16);
        press(&mut t, KeyCode::Left, KeyModifiers::CONTROL);
        assert_eq!(t.cur.col, 11);
        press(&mut t, KeyCode::Left, KeyModifiers::ALT);
        assert_eq!(t.cur.col, if cfg!(target_os = "macos") { 6 } else { 0 });
    }

    #[test]
    fn delete_key_of_a_string_filter() {
        let k = |c, m| KeyEvent::new(c, m);
        let mut s = String::from("src/app.rs");
        assert!(delete_key(&mut s, &k(KeyCode::Backspace, KeyModifiers::CONTROL)));
        assert_eq!(s, "src/");
        assert!(delete_key(&mut s, &k(KeyCode::Backspace, KeyModifiers::NONE)));
        assert_eq!(s, "src");
        s = "a b".into();
        assert!(delete_key(&mut s, &k(KeyCode::Backspace, KeyModifiers::SUPER)));
        assert!(s.is_empty());
        assert!(!delete_key(&mut s, &k(KeyCode::Char('u'), KeyModifiers::NONE)));
    }
}
