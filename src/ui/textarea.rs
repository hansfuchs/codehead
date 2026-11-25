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

use crate::{infra::clip, ui::theme};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Default)]
pub(crate) struct Pos {
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

pub(crate) enum Outcome {
    /// Key used by the input.
    Handled,
    /// Not an input key: the screen handles it (tab, esc, ctrl+enter, …).
    Ignored,
}

pub(crate) struct TextArea {
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
pub(crate) fn mention_colors(line: &[char], marks: &[(String, Color)]) -> Vec<Option<Color>> {
    let mut out = vec![None; line.len()];

    if marks.is_empty() {
        return out;
    }

    let name_char = |character: char| is_word(character) || character == '-';
    let mut i = 0;

    while i < line.len() {
        color_mention_at(line, marks, &mut out, name_char, &mut i);
    }

    out
}

fn color_mention_at(
    line: &[char],
    marks: &[(String, Color)],
    out: &mut [Option<Color>],
    name_char: impl Fn(char) -> bool,
    i: &mut usize,
) {
    if line[*i] != '@' || (*i > 0 && !(line[*i - 1].is_whitespace() || matches!(line[*i - 1], '(' | '[' | '"' | '\'')))
    {
        *i += 1;
        return;
    }

    let mut j = *i + 1;

    while j < line.len() && name_char(line[j]) {
        j += 1;
    }

    let name: String = line[*i + 1..j].iter().collect();

    if let Some((_, color)) = marks.iter().find(|(mark_name, _)| *mark_name == name) {
        out[*i..j].iter_mut().for_each(|slot| *slot = Some(*color));

        // `@new (replaces @old)`: the note in the color of NEW, `@old` in the color of OLD
        if let Some((end, old, oc)) = marks.iter().find_map(|(old, oc)| {
            let note: Vec<char> = crate::ui::mention::replaces_note(old).chars().collect();

            line[j..].starts_with(&note).then_some((j + note.len(), old, oc))
        }) {
            out[j + 1..end].iter_mut().for_each(|slot| *slot = Some(*color));
            let at = end - 1 - old.chars().count() - 1;

            out[at..end - 1].iter_mut().for_each(|x| *x = Some(*oc));
            j = end;
        }
    }

    *i = j.max(*i + 1);
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
pub(crate) fn stamp(at: Option<Instant>) {
    KEY_AT.with(|key_at| key_at.set(at));
}

/// Markdown list item at the start of LINE: (indent, marker width incl. its space, marker of the
/// next item). `- `, `* `, `+ ` (+ `[ ] ` / `[x] `), `1. `, `1) `.
fn list_item(line: &[char]) -> Option<(usize, usize, String)> {
    let indent = line.iter().take_while(|character| **character == ' ').count();
    let rest = &line[indent..];

    let bullet = match rest {
        [bytes @ ('-' | '*' | '+'), ' ', ..] => Some(*bytes),
        _ => None,
    };

    if let Some(mark) = bullet {
        if let ['[', ' ' | 'x' | 'X', ']', ' ', ..] = &rest[2..] {
            return Some((indent, 6, format!("{mark} [ ] ")));
        }

        return Some((indent, 2, format!("{mark} ")));
    }

    let digits = rest.iter().take_while(|ch| ch.is_ascii_digit()).count();

    if (1..=9).contains(&digits) && matches!(rest.get(digits), Some('.' | ')')) && rest.get(digits + 1) == Some(&' ') {
        let number: u64 = rest[..digits].iter().collect::<String>().parse().ok()?;

        return Some((indent, digits + 2, format!("{}{} ", number + 1, rest[digits])));
    }

    None
}

fn is_fence(line: &[char]) -> bool {
    let rest: Vec<&char> = line.iter().skip_while(|character| **character == ' ').collect();

    rest.len() >= 3 && rest[..3].iter().all(|ch| **ch == '`')
}

/// Per line: syntax colors of the fenced code (None: prose or a fence line), and fence lines.
fn fence_colors(lines: &[Vec<char>]) -> (Vec<Option<Vec<ratatui::style::Color>>>, Vec<bool>) {
    let mut colors = vec![None; lines.len()];
    let fence: Vec<bool> = lines.iter().map(|line| is_fence(line)).collect();
    let mut i = 0;

    while i < lines.len() {
        color_fence_block(lines, &mut colors, &fence, &mut i);
    }

    (colors, fence)
}

fn color_fence_block(lines: &[Vec<char>], colors: &mut [Option<Vec<Color>>], fence: &[bool], i: &mut usize) {
    if !fence[*i] {
        *i += 1;
        return;
    }

    let tag: String = lines[*i]
        .iter()
        .skip_while(|character| **character == ' ' || **character == '`')
        .collect::<String>()
        .trim()
        .to_string();
    let end = (*i + 1..lines.len()).find(|j| fence[*j]).unwrap_or(lines.len());

    if end > *i + 1 {
        let body: String = lines[*i + 1..end]
            .iter()
            .map(|line| line.iter().collect::<String>())
            .collect::<Vec<_>>()
            .join("\n");

        let lang = if tag.is_empty() {
            crate::ui::code::language(&body, &[])
        } else {
            tag.as_str()
        };

        let hl = crate::ui::code::highlight(&body, lang);

        for (offset, row) in (*i + 1..end).enumerate() {
            colors[row] = hl
                .get(offset)
                .map(|colored| colored.iter().map(|(color, _)| *color).collect());
        }
    }

    *i = end + 1;
}

/// TEXT without its trailing empty Markdown list items (a marker an Enter left, e.g. `7.`), only
/// after other lines, trimmed at the end. A sent message: 0 stray marker the view hides.
pub(crate) fn without_empty_items(text: &str) -> &str {
    let mut rest = text.trim_end();

    while let Some(i) = rest.rfind('\n') {
        let line: Vec<char> = rest[i + 1..].chars().chain([' ']).collect();

        match list_item(&line) {
            Some((indent, width, _)) if line.len() == indent + width => rest = rest[..i].trim_end(),
            _ => break,
        }
    }

    rest
}

/// $`CODEHEAD_KEYLOG`=<file>: every key of every input goes there (ms since the 1st key, code, modifiers).
/// For a report on keys the terminal sends in a way codehead does not expect.
fn keylog(key: &KeyEvent, now: Instant) {
    static LOG: std::sync::OnceLock<Option<(std::sync::Mutex<std::fs::File>, Instant)>> = std::sync::OnceLock::new();

    let log = LOG.get_or_init(|| {
        let path = std::env::var_os("CODEHEAD_KEYLOG").filter(|path| !path.is_empty())?;
        let file = std::fs::OpenOptions::new().create(true).append(true).open(path).ok()?;

        Some((std::sync::Mutex::new(file), now))
    });

    if let Some((file, t0)) = log
        && let Ok(mut file) = file.lock()
    {
        use std::io::Write;
        let _ = writeln!(
            file,
            "{:>8.1}ms {:?} {:?}",
            now.saturating_duration_since(*t0).as_secs_f64() * 1000.0,
            key.code,
            key.modifiers
        );
    }
}

fn cw(character: char) -> usize {
    character.width().unwrap_or(0)
}

fn width_of(text: &[char]) -> usize {
    text.iter().map(|&character| cw(character)).sum()
}

fn is_word(character: char) -> bool {
    character.is_alphanumeric() || character == '_'
}

/// The modifiers of the row keys (⌫: to the row's start, ←→: row start / end): ⌘ (super, kitty
/// protocol), alt off macOS. On macOS a terminal's alt is ⌥, the key of the word keys.
pub(crate) fn line_mod(modifiers: KeyModifiers) -> bool {
    modifiers.contains(KeyModifiers::SUPER) || (!cfg!(target_os = "macos") && modifiers.contains(KeyModifiers::ALT))
}

/// ctrl+f (⌘f with the kitty protocol): the only key that focuses a filter, in every list; in the
/// chat: the search bar.
pub(crate) fn is_filter_key(key: &KeyEvent) -> bool {
    key.code == KeyCode::Char('f')
        && key.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::SUPER)
        && !key.modifiers.contains(KeyModifiers::ALT)
}

/// Esc on unsaved edits (esc never drops edits unasked): DIRTY and 0 esc within 2 s before → false
/// (the caller warns: "esc again: drop"); a 2nd esc in time or 0 edits → true (drop / close now).
/// AT: the caller's last esc. Times: the read time of the key ([`stamp`]), not its handle time.
pub(crate) fn esc_drops(at: &mut Option<std::time::Instant>, dirty: bool) -> bool {
    let now = KEY_AT.with(std::cell::Cell::get).unwrap_or_else(Instant::now);

    if !dirty
        || at
            .take()
            .is_some_and(|last| now.saturating_duration_since(last) < std::time::Duration::from_secs(2))
    {
        return true;
    }

    *at = Some(now);
    false
}

/// Delete keys of a 1-line text kept as a String (dropdown filters): ⌫ 1 char; ⌥⌫ / ctrl+⌫ /
/// ctrl+w / ctrl+h the string left of the end (a path: 1 segment, as [`TextArea`]); ⌘⌫ (alt+⌫ off
/// macOS) / ctrl+u all. True: S changed or the key was 1 of these.
pub(crate) fn delete_key(text: &mut String, key: &KeyEvent) -> bool {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    let all = (key.code == KeyCode::Backspace && line_mod(key.modifiers)) || (ctrl && key.code == KeyCode::Char('u'));
    let word =
        (key.code == KeyCode::Backspace && (ctrl || alt)) || (ctrl && matches!(key.code, KeyCode::Char('w' | 'h')));

    if all {
        text.clear();
    } else if word {
        let line: Vec<char> = text.chars().collect();
        let mut end = line.len();

        while end > 0 && line[end - 1].is_whitespace() {
            end -= 1;
        }

        while end > 0 && line[end - 1] == '/' {
            end -= 1;
        }

        while end > 0 && !line[end - 1].is_whitespace() && line[end - 1] != '/' {
            end -= 1;
        }

        *text = line[..end].iter().collect();
    } else if key.code == KeyCode::Backspace {
        text.pop();
    } else {
        return false;
    }

    true
}

/// Soft word wrap of 1 line into char ranges of max. WIDTH display columns.
pub(crate) fn wrap(line: &[char], width: usize) -> Vec<(usize, usize)> {
    let width = width.max(1);
    let mut segs = Vec::new();
    let (mut start, mut i, mut used, mut space) = (0, 0, 0, None::<usize>);

    while i < line.len() {
        let char_width = cw(line[i]);

        if used + char_width > width && i > start {
            let brk = match space {
                Some(space_at) if space_at + 1 > start => space_at + 1,
                _ => i,
            };

            segs.push((start, brk));
            start = brk;
            i = brk;
            used = 0;
            space = None;
            continue;
        }

        if line[i] == ' ' {
            space = Some(i);
        }

        used += char_width;
        i += 1;
    }

    segs.push((start, line.len()));

    segs
}

impl TextArea {
    pub(crate) fn new(single: bool, placeholder: &str) -> TextArea {
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

    pub(crate) fn text(&self) -> String {
        self.lines
            .iter()
            .map(|line| line.iter().collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// The owner edited the text since the last `set_text` (1+ undo step).
    pub(crate) fn edited(&self) -> bool {
        !self.undo.is_empty()
    }

    /// New value from the program (load, send): a new undo history, like a browser input.
    pub(crate) fn set_text(&mut self, text: &str) {
        self.undo.clear();
        self.redo.clear();
        self.last_op = None;
        self.replace_all(text);
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
    pub(crate) fn is_single(&self) -> bool {
        self.single
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.lines.len() == 1 && self.lines[0].is_empty()
    }

    /// Cursor on the 1st visual row (wrap included): ↑ leaves the input.
    pub(crate) fn on_first_row(&self) -> bool {
        self.vis.is_empty() || self.visual_of(self.cur).0 == 0
    }

    /// Cursor on the last visual row: ↓ leaves the input.
    pub(crate) fn on_last_row(&self) -> bool {
        self.vis.is_empty() || self.visual_of(self.cur).0 + 1 >= self.vis.len()
    }

    /// Cursor at the start of the text, without a selection.
    pub(crate) fn move_start(&mut self) {
        self.move_to(Pos { row: 0, col: 0 }, false);
    }

    /// Cursor at the very end of the text.
    pub(crate) fn move_end(&mut self) {
        let last = self.lines.len() - 1;

        self.move_to(
            Pos {
                row: last,
                col: self.lines[last].len(),
            },
            false,
        );
    }

    pub(crate) fn area(&self) -> Rect {
        self.area
    }

    /// The cursor: (row, column in chars).
    pub(crate) fn cursor(&self) -> (usize, usize) {
        (self.cur.row, self.cur.col)
    }

    pub(crate) fn hide(&mut self) {
        self.area = Rect::default();
    }

    /// Display rows needed for WIDTH (input height for the title).
    pub(crate) fn rows_for(&self, width: u16) -> usize {
        let cols = width.saturating_sub(1) as usize;

        self.lines
            .iter()
            .map(|line| wrap(line, cols).len())
            .sum::<usize>()
            .max(1)
    }

    // ---------- selection ----------

    // ---------- undo ----------

    // ---------- edits ----------

    // ---------- movement ----------

    // ---------- input ----------

    // ---------- render ----------
}

#[cfg(test)]
#[path = "textarea_tests.rs"]
mod tests;

#[path = "textarea_draw_helpers.rs"]
mod draw_helpers;
#[path = "textarea_key.rs"]
mod key;
#[path = "textarea_key_helpers.rs"]
mod key_helpers;
#[path = "textarea_mouse.rs"]
mod mouse;
#[path = "textarea_render.rs"]
mod render;
#[path = "textarea_updates.rs"]
mod updates;

#[path = "textarea_completion.rs"]
mod completion;
#[path = "textarea_editing.rs"]
mod editing;
#[path = "textarea_history.rs"]
mod history;
#[path = "textarea_navigation.rs"]
mod navigation;
#[path = "textarea_selection.rs"]
mod selection;

#[cfg(test)]
#[path = "textarea_lists_tests.rs"]
mod lists_tests;

#[cfg(test)]
#[path = "textarea_history_tests.rs"]
mod history_tests;

#[cfg(test)]
#[path = "textarea_paste_tests.rs"]
mod paste_tests;
