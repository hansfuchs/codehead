//! Shortcuts: `#name` in a prompt stands for a text the owner wrote (settings → shortcuts),
//! e.g. `#review` → "bring in another agent to review the code you wrote …". The texts live in
//! `<config>/shortcuts.toml` (`name = '''text'''`), read again when the file changes.
//!
//! `#` in a prompt: a dropdown of the shortcuts, filtered as the name is typed; a 2nd box beside
//! it shows the current shortcut's text. ↑↓: `#name` into the text at once (live preview), tab /
//! enter: the text in place of `#name`. A `#name` left in the prompt: expanded on send. The mouse
//! over a `#name` of the prompt (or the cursor in it): a popup with its text.

use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use unicode_width::UnicodeWidthStr;

use crate::ui::dropdown;
use crate::ui::textarea::{Pos, TextArea};
use crate::ui::theme;

/// The shortcuts for query Q: names that start with it, then names that hold it.
fn matches(list: &[Shortcut], query: &str) -> Vec<usize> {
    let query = query.to_lowercase();
    let mut found: Vec<usize> = (0..list.len())
        .filter(|&i| list[i].name.to_lowercase().starts_with(&query))
        .collect();

    found.extend(
        (0..list.len()).filter(|&i| {
            !list[i].name.to_lowercase().starts_with(&query) && list[i].name.to_lowercase().contains(&query)
        }),
    );

    found
}

/// Rows of TEXT wrapped to W columns (max. MAX rows; more: `…` on the last).
fn wrap(text: &str, width: usize, max: usize) -> Vec<String> {
    let mut rows = Vec::new();

    for line in text.lines() {
        wrap_cur(width, &mut rows, line);
    }

    if rows.len() > max {
        rows.truncate(max);

        if let Some(last) = rows.last_mut() {
            last.push('…');
        }
    }

    rows
}

fn wrap_cur(width: usize, rows: &mut Vec<String>, line: &str) {
    if line.is_empty() {
        rows.push(String::new());
        return;
    }

    let mut cur = String::new();

    for word in line.split(' ') {
        if !cur.is_empty() && cur.width() + 1 + word.width() > width {
            rows.push(std::mem::take(&mut cur));
        }

        if !cur.is_empty() {
            cur.push(' ');
        }

        cur.push_str(word);

        while cur.width() > width {
            let head: String = cur.chars().take(width).collect();

            cur = cur.chars().skip(width).collect();
            rows.push(head);
        }
    }

    rows.push(cur);
}

/// A box with TEXT (a shortcut's text) in R: frame with LABEL, the text wrapped.
pub(crate) fn text_box(frame: &mut Frame, area: Rect, label: &str, text: &str) {
    let inner = dropdown::frame(
        frame,
        area,
        label,
        "",
        dropdown::FrameState {
            up: false,
            down: false,
            active: false,
        },
    );
    let rows = wrap(
        text,
        usize::from(inner.width.saturating_sub(2)),
        usize::from(inner.height),
    );
    let lines: Vec<Line> = rows
        .into_iter()
        .map(|line| Line::styled(format!(" {line}"), theme::text()))
        .collect();

    frame.render_widget(Paragraph::new(lines), inner);
}

/// Width and rows of the text box for TEXT: max. 64 columns, 12 rows.
pub(crate) fn box_size(text: &str, bounds: Rect) -> (u16, usize) {
    let wanted = text
        .lines()
        .map(UnicodeWidthStr::width)
        .max()
        .unwrap_or(0)
        .clamp(24, 62) as u16
        + 4;
    let width = wanted.min(bounds.width);
    let rows = wrap(text, usize::from(width.saturating_sub(4)), 12).len().max(1);

    (width, rows)
}

/// The `#` dropdown of a prompt.
#[derive(Default)]
pub(crate) struct Pick {
    cursor: usize,
    offset: usize,
    /// esc on the dropdown of the `#` at this position: closed until that `#` is gone.
    closed: Option<Pos>,
    /// The query the cursor belongs to (new text: the 1st row again).
    seen: String,
    preview: dropdown::Preview,
    /// Rows of the last draw (mouse): area, index into the matches.
    rows: Vec<(Rect, usize)>,
    area: Rect,
    /// The row under the mouse: its text shows in the box.
    hover: Option<usize>,
}

struct Open {
    at: Pos,
    /// The query as typed (a preview in the text: the typed one).
    query: String,
    list: Vec<Shortcut>,
    matches: Vec<usize>,
}

impl Pick {
    /// The dropdown shows for TA.
    pub(crate) fn is_open(&mut self, ta: &TextArea) -> bool {
        self.open(ta).is_some()
    }

    fn open(&mut self, ta: &TextArea) -> Option<Open> {
        let found = ta.shortcut();

        let (at, query) = match self.preview.typed(ta) {
            Some((pos, typed)) if found.as_ref().is_some_and(|shortcut| shortcut.0 == pos) => {
                (pos, typed.chars().skip(1).collect())
            }
            _ => found?,
        };

        if self.closed == Some(at) {
            return None;
        }

        self.closed = None;
        let list = all();
        let matched = matches(&list, &query);

        if matched.is_empty() {
            return None;
        }

        if self.seen != query {
            self.seen.clone_from(&query);
            self.cursor = 0;
            self.offset = 0;
        }

        self.cursor = self.cursor.min(matched.len() - 1);

        Some(Open {
            at,
            query,
            list,
            matches: matched,
        })
    }

    /// The shortcut's text in place of `#query`.
    fn pick(&mut self, ta: &mut TextArea, open: &Open, i: usize) {
        let Some(shortcut) = open.matches.get(i).map(|&index| &open.list[index]) else {
            return;
        };

        ta.replace_to_cursor(open.at, &shortcut.text);
        self.preview.clear();
        self.cursor = 0;
        self.hover = None;
    }

    pub(crate) fn hide(&mut self) {
        self.rows.clear();
        self.area = Rect::default();
    }

    /// Row of the last draw at P (hover).
    pub(crate) fn row_at(&self, at: Position) -> Option<Rect> {
        self.rows.iter().map(|(rect, _)| *rect).find(|rect| rect.contains(at))
    }
}

/// The popup of the `#name` (a shortcut) under the mouse AT, else at TA's cursor: its text in a
/// box above it, inside BOUNDS. 0 popup while the `#` dropdown is open.
/// CURSOR: TA has the keys (its cursor counts).
pub(crate) fn render_popup(
    frame: &mut Frame,
    ta: &TextArea,
    at: Option<Position>,
    cursor: bool,
    bounds: Rect,
    pick_open: bool,
) {
    if pick_open {
        return;
    }

    let hit = at
        .and_then(|pos_at| ta.shortcut_at(Some(pos_at)))
        .or_else(|| if cursor { ta.shortcut_at(None) } else { None });
    let Some((pos, name)) = hit else { return };
    let Some(text) = get(&name) else { return };
    let Some(cursor_pos) = ta.screen_pos(pos) else { return };
    let (width, rows) = box_size(&text, bounds);
    let anchor = Rect {
        x: cursor_pos.x,
        y: cursor_pos.y,
        width: 1,
        height: 1,
    };

    let Some(row) = dropdown::place_max(anchor, width, rows, 12, bounds, true) else {
        return;
    };

    text_box(frame, row, &format!("#{name}"), &text);
}

/// Settings → shortcuts: the list left, the name + text of the picked one right.
pub(crate) struct Tab {
    list: Vec<Shortcut>,
    at: usize,
    focus: Focus,
    name: TextArea,
    text: TextArea,
    /// The last invalid input / failed save.
    error: Option<String>,
    /// d once: armed, d again: deleted.
    armed: bool,
    /// The last esc in the name / text: a 2nd within 2 s drops the edits.
    esc_at: Option<std::time::Instant>,
    list_rect: Rect,
    name_rect: Rect,
    text_rect: Rect,
    rows: Vec<(Rect, usize)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Focus {
    List,
    Name,
    Text,
}

impl Default for Tab {
    fn default() -> Self {
        Self::new()
    }
}

impl Tab {
    pub(crate) fn new() -> Tab {
        let mut name = TextArea::new(true, "name: letters, digits, _ -");

        name.style = Some(theme::panel());
        let mut text = TextArea::new(false, "the text #name stands for");

        text.style = Some(theme::panel());
        let mut tab = Tab {
            list: Vec::new(),
            at: 0,
            focus: Focus::List,
            name,
            text,
            error: None,
            armed: false,
            esc_at: None,
            list_rect: Rect::default(),
            name_rect: Rect::default(),
            text_rect: Rect::default(),
            rows: Vec::new(),
        };

        tab.enter();

        tab
    }

    /// The tab shows: the file read again.
    pub(crate) fn enter(&mut self) {
        self.list = all();
        self.at = self.at.min(self.list.len());
        self.focus = Focus::List;
        self.load();
    }

    /// The inputs show the picked shortcut.
    fn load(&mut self) {
        let (name, text) = self
            .list
            .get(self.at)
            .map(|shortcut| (shortcut.name.clone(), shortcut.text.clone()))
            .unwrap_or_default();

        self.name.set_text(&name);
        self.text.set_text(&text);
        self.error = None;
    }

    /// The inputs into the list and the file. False: an invalid name (the error shows).
    fn store(&mut self) -> bool {
        let Some(cur) = self.list.get(self.at).cloned() else {
            return true;
        };

        let name = self.name.text().trim().to_string();
        let text = self.text.text().trim_end().to_string();

        if !valid_name(&name) {
            self.error = Some("name: letters, digits, _ or - only".into());
            return false;
        }

        if name != cur.name && self.list.iter().any(|shortcut| shortcut.name == name) {
            self.error = Some(format!("#{name} exists already"));
            return false;
        }

        if name == cur.name && text == cur.text {
            return true;
        }

        self.list[self.at] = Shortcut {
            name: name.clone(),
            text,
        };

        if let Err(error) = save(&self.list) {
            self.error = Some(format!("save: {error}"));
            return false;
        }

        self.list = all();
        self.at = self.list.iter().position(|shortcut| shortcut.name == name).unwrap_or(0);
        self.error = None;

        true
    }

    fn add(&mut self) {
        let mut number = 1;

        while self
            .list
            .iter()
            .any(|shortcut| shortcut.name == format!("new-{number}"))
        {
            number += 1;
        }

        self.list.push(Shortcut {
            name: format!("new-{number}"),
            text: String::new(),
        });
        self.at = self.list.len() - 1;
        self.load();
        self.name.select_all();
        self.focus = Focus::Name;
    }

    fn delete(&mut self) {
        if self.at >= self.list.len() {
            return;
        }

        self.list.remove(self.at);

        if let Err(error) = save(&self.list) {
            self.error = Some(format!("save: {error}"));
        }

        self.at = self.at.min(self.list.len().saturating_sub(1));
        self.load();
    }

    pub(crate) fn paste(&mut self, text: &str) {
        match self.focus {
            Focus::Name => self.name.insert(text),
            Focus::Text => self.text.insert(text),
            Focus::List => {}
        }
    }

    pub(crate) fn row_at(&self, at: Position) -> Option<Rect> {
        self.rows.iter().map(|(rect, _)| *rect).find(|rect| rect.contains(at))
    }

    pub(crate) fn help(&self) -> &'static [(&'static str, &'static str)] {
        match self.focus {
            Focus::List if self.armed => &[("d", "again: delete"), ("↑↓", "keep")],
            Focus::List => &[
                ("↑↓", "move"),
                ("enter →", "edit text"),
                ("r", "rename"),
                ("n", "new"),
                ("d d", "delete"),
                ("↑ top", "sub-tabs"),
            ],
            Focus::Name => &[("enter ↓", "the text"), ("esc", "back")],
            Focus::Text => &[
                ("ctrl+s", "save"),
                ("esc", "back"),
                ("↑ 1st row", "the name"),
                ("enter", "new line"),
            ],
        }
    }
}

#[cfg(test)]
#[path = "shortcuts_tests.rs"]
mod tests;

pub(crate) use crate::domain::shortcuts::*;

#[path = "shortcuts_draw.rs"]
mod draw;
#[path = "shortcuts_key.rs"]
mod key;
#[path = "shortcuts_mouse.rs"]
mod mouse;
#[path = "shortcuts_render.rs"]
mod render;
