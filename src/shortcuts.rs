//! Shortcuts: `#name` in a prompt stands for a text the owner wrote (settings → shortcuts),
//! e.g. `#review` → "bring in another agent to review the code you wrote …". The texts live in
//! `<config>/shortcuts.toml` (`name = '''text'''`), read again when the file changes.
//!
//! `#` in a prompt: a dropdown of the shortcuts, filtered as the name is typed; a 2nd box beside
//! it shows the current shortcut's text. ↑↓: `#name` into the text at once (live preview), tab /
//! enter: the text in place of `#name`. A `#name` left in the prompt: expanded on send. The mouse
//! over a `#name` of the prompt (or the cursor in it): a popup with its text.

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::SystemTime;

use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use unicode_width::UnicodeWidthStr;

use crate::dropdown;
use crate::textarea::{Pos, TextArea};
use crate::theme;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Shortcut {
    pub name: String,
    pub text: String,
}

/// The shortcuts of a new install (0 file yet).
fn defaults() -> Vec<Shortcut> {
    vec![Shortcut { name: "review".into(), text: "bring in another agent to review the code you wrote and have it write comments for each finding".into() }]
}

pub fn path() -> PathBuf {
    // tests: the test data dir, never the owner's config
    if cfg!(test) {
        return crate::engine::data_dir().join("shortcuts.toml");
    }
    crate::engine::shared_dir().join("shortcuts.toml")
}

/// The last read: the file's time, the shortcuts.
static CACHE: Mutex<Option<(Option<SystemTime>, Vec<Shortcut>)>> = Mutex::new(None);

/// A valid name: letters, digits, `_`, `-`.
pub fn valid_name(n: &str) -> bool {
    !n.is_empty() && n.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '-')
}

fn parse(text: &str) -> Result<Vec<Shortcut>, String> {
    let t: toml::Table = text.parse().map_err(|e: toml::de::Error| e.to_string())?;
    let mut v: Vec<Shortcut> = t.into_iter().filter_map(|(name, val)| Some(Shortcut { name, text: val.as_str()?.to_string() })).collect();
    v.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(v)
}

/// Every shortcut, A→Z (1 stat per call; the file again when it changed).
pub fn all() -> Vec<Shortcut> {
    let p = path();
    let at = std::fs::metadata(&p).and_then(|m| m.modified()).ok();
    let mut g = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((t, v)) = g.as_ref()
        && *t == at
    {
        return v.clone();
    }
    let v = match std::fs::read_to_string(&p) {
        Ok(text) => parse(&text).unwrap_or_default(),
        Err(_) => defaults(),
    };
    *g = Some((at, v.clone()));
    v
}

/// The text of shortcut NAME.
pub fn get(name: &str) -> Option<String> {
    all().into_iter().find(|s| s.name == name).map(|s| s.text)
}

/// TOML of LIST: `name = '''text'''` (a text with `'''` in it: a basic string).
fn render(list: &[Shortcut]) -> String {
    let mut out = String::from("# codehead shortcuts: #name in a prompt → this text\n\n");
    for s in list {
        let key = if s.name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') { s.name.clone() } else { toml::Value::String(s.name.clone()).to_string() };
        if s.text.contains("'''") {
            out.push_str(&format!("{key} = {}\n\n", toml::Value::String(s.text.clone())));
        } else {
            out.push_str(&format!("{key} = '''\n{}'''\n\n", s.text));
        }
    }
    out
}

/// LIST into the file.
pub fn save(list: &[Shortcut]) -> Result<(), String> {
    let p = path();
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    let mut list = list.to_vec();
    list.sort_by(|a, b| a.name.cmp(&b.name));
    crate::engine::write_atomic(&p, render(&list).as_bytes()).map_err(|e| e.to_string())?;
    let at = std::fs::metadata(&p).and_then(|m| m.modified()).ok();
    *CACHE.lock().unwrap_or_else(|e| e.into_inner()) = Some((at, list));
    Ok(())
}

/// TEXT with each `#name` of a shortcut (at a word start, the name whole) as the shortcut's text.
pub fn expand(text: &str) -> String {
    if !text.contains('#') {
        return text.to_string();
    }
    expand_with(&all(), text)
}

fn expand_with(list: &[Shortcut], text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let name = |c: char| c.is_alphanumeric() || c == '_' || c == '-';
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let start = i == 0 || chars[i - 1].is_whitespace() || matches!(chars[i - 1], '(' | '[' | '"' | '\'');
        if c == '#' && start {
            let mut j = i + 1;
            while j < chars.len() && name(chars[j]) {
                j += 1;
            }
            let word: String = chars[i + 1..j].iter().collect();
            if let Some(s) = list.iter().find(|s| s.name == word) {
                out.push_str(&s.text);
                i = j;
                continue;
            }
        }
        out.push(c);
        i += 1;
    }
    out
}

/// The shortcuts for query Q: names that start with it, then names that hold it.
fn matches(list: &[Shortcut], q: &str) -> Vec<usize> {
    let q = q.to_lowercase();
    let mut v: Vec<usize> = (0..list.len()).filter(|&i| list[i].name.to_lowercase().starts_with(&q)).collect();
    v.extend((0..list.len()).filter(|&i| !list[i].name.to_lowercase().starts_with(&q) && list[i].name.to_lowercase().contains(&q)));
    v
}

/// Rows of TEXT wrapped to W columns (max. MAX rows; more: `…` on the last).
fn wrap(text: &str, w: usize, max: usize) -> Vec<String> {
    let mut rows = Vec::new();
    for line in text.lines() {
        if line.is_empty() {
            rows.push(String::new());
            continue;
        }
        let mut cur = String::new();
        for word in line.split(' ') {
            if !cur.is_empty() && cur.width() + 1 + word.width() > w {
                rows.push(std::mem::take(&mut cur));
            }
            if !cur.is_empty() {
                cur.push(' ');
            }
            cur.push_str(word);
            while cur.width() > w {
                let head: String = cur.chars().take(w).collect();
                cur = cur.chars().skip(w).collect();
                rows.push(head);
            }
        }
        rows.push(cur);
    }
    if rows.len() > max {
        rows.truncate(max);
        if let Some(l) = rows.last_mut() {
            l.push('…');
        }
    }
    rows
}

/// A box with TEXT (a shortcut's text) in R: frame with LABEL, the text wrapped.
fn text_box(f: &mut Frame, r: Rect, label: &str, text: &str) {
    let inner = dropdown::frame(f, r, label, "", false, false, false);
    let rows = wrap(text, usize::from(inner.width.saturating_sub(2)), usize::from(inner.height));
    let lines: Vec<Line> = rows.into_iter().map(|l| Line::styled(format!(" {l}"), theme::text())).collect();
    f.render_widget(Paragraph::new(lines), inner);
}

/// Width and rows of the text box for TEXT: max. 64 columns, 12 rows.
fn box_size(text: &str, bounds: Rect) -> (u16, usize) {
    let w = text.lines().map(UnicodeWidthStr::width).max().unwrap_or(0).clamp(24, 62) as u16 + 4;
    let w = w.min(bounds.width);
    let rows = wrap(text, usize::from(w.saturating_sub(4)), 12).len().max(1);
    (w, rows)
}

/// The `#` dropdown of a prompt.
#[derive(Default)]
pub struct Pick {
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
    q: String,
    list: Vec<Shortcut>,
    m: Vec<usize>,
}

impl Pick {
    /// The dropdown shows for TA.
    pub fn is_open(&mut self, ta: &TextArea) -> bool {
        self.open(ta).is_some()
    }

    fn open(&mut self, ta: &TextArea) -> Option<Open> {
        let found = ta.shortcut();
        let (at, q) = match self.preview.typed(ta) {
            Some((p, t)) if found.as_ref().is_some_and(|f| f.0 == p) => (p, t.chars().skip(1).collect()),
            _ => found?,
        };
        if self.closed == Some(at) {
            return None;
        }
        self.closed = None;
        let list = all();
        let m = matches(&list, &q);
        if m.is_empty() {
            return None;
        }
        if self.seen != q {
            self.seen = q.clone();
            self.cursor = 0;
            self.offset = 0;
        }
        self.cursor = self.cursor.min(m.len() - 1);
        Some(Open { at, q, list, m })
    }

    /// The shortcut's text in place of `#query`.
    fn pick(&mut self, ta: &mut TextArea, o: &Open, i: usize) {
        let Some(s) = o.m.get(i).map(|&k| &o.list[k]) else { return };
        ta.replace_to_cursor(o.at, &s.text);
        self.preview.clear();
        self.cursor = 0;
        self.hover = None;
    }

    /// Key for the dropdown. true: used (the input must not see it).
    pub fn key(&mut self, k: KeyEvent, ta: &mut TextArea) -> bool {
        let Some(o) = self.open(ta) else { return false };
        let plain = k.modifiers.is_empty() || k.modifiers == KeyModifiers::SHIFT;
        let n = o.m.len();
        match k.code {
            KeyCode::Up | KeyCode::Down if plain => {
                self.cursor = if k.code == KeyCode::Up { (self.cursor + n - 1) % n } else { (self.cursor + 1) % n };
                self.hover = None;
                let name = o.list[o.m[self.cursor]].name.clone();
                self.preview.show(ta, o.at, &format!("#{}", o.q), &format!("#{name}"));
            }
            KeyCode::Tab | KeyCode::Enter if k.modifiers.is_empty() => self.pick(ta, &o, self.cursor),
            KeyCode::Esc if k.modifiers.is_empty() => self.closed = Some(o.at),
            KeyCode::Char(c) if plain && (c.is_alphanumeric() || c == '_' || c == '-') && self.preview.revert(ta) => return false,
            _ => return false,
        }
        true
    }

    pub fn hide(&mut self) {
        self.rows.clear();
        self.area = Rect::default();
    }

    /// Row of the last draw at P (hover).
    pub fn row_at(&self, p: Position) -> Option<Rect> {
        self.rows.iter().map(|(r, _)| *r).find(|r| r.contains(p))
    }

    /// Click on a row: its text in place of `#query`. Hover: the row's text in the box. Wheel:
    /// the cursor. true: used.
    pub fn mouse(&mut self, m: MouseEvent, ta: &mut TextArea) -> bool {
        let p = Position { x: m.column, y: m.row };
        if !self.area.contains(p) {
            if matches!(m.kind, MouseEventKind::Moved) {
                self.hover = None;
            }
            return false;
        }
        let Some(o) = self.open(ta) else { return false };
        let n = o.m.len();
        let hit = self.rows.iter().find(|(r, _)| r.contains(p)).map(|&(_, i)| i);
        match m.kind {
            MouseEventKind::ScrollUp => self.cursor = self.cursor.saturating_sub(1),
            MouseEventKind::ScrollDown => self.cursor = (self.cursor + 1).min(n - 1),
            MouseEventKind::Moved => self.hover = hit,
            MouseEventKind::Down(MouseButton::Left) => {
                if let Some(i) = hit {
                    self.pick(ta, &o, i);
                }
            }
            _ => {}
        }
        true
    }

    /// The dropdown for the `#` of TA (TA rendered this frame) inside BOUNDS, never over AVOID
    /// (the prompt box), and the box with the current (hovered) shortcut's text beside it.
    pub fn render(&mut self, f: &mut Frame, ta: &TextArea, bounds: Rect, avoid: Rect) {
        self.hide();
        let Some(o) = self.open(ta) else { return };
        let Some(p) = ta.screen_pos(o.at) else { return };
        let above = avoid.y.saturating_sub(bounds.y) >= 5 || avoid.y.saturating_sub(bounds.y) >= bounds.bottom().saturating_sub(avoid.bottom());
        let n = o.m.len();
        let w = (o.m.iter().map(|&i| o.list[i].name.width()).max().unwrap_or(0) + 6).clamp(20, 40) as u16;
        let (anchor, room) = if above {
            (Rect { x: p.x, y: avoid.y, width: 1, height: 1 }, Rect { height: avoid.y.saturating_sub(bounds.y), ..bounds })
        } else {
            let y = avoid.bottom().clamp(bounds.y, bounds.bottom());
            (Rect { x: p.x, y: avoid.bottom().saturating_sub(1), width: 1, height: 1 }, Rect { y, height: bounds.bottom() - y, ..bounds })
        };
        let Some(area) = dropdown::place(anchor, w, n, room, above) else { return };
        let rows: Vec<dropdown::Row> = o.m.iter().map(|&i| dropdown::Row { text: format!("#{}", o.list[i].name), detail: String::new(), now: false, marks: Vec::new() }).collect();
        let (off, hits) = dropdown::list(f, area, "shortcuts", "tab / enter", &rows, self.cursor, self.offset, "0 shortcuts");
        self.offset = off;
        self.rows = hits;
        self.area = area;
        // the 2nd box: the text of the hovered row, else of the current one
        let cur = self.hover.filter(|&h| h < n).unwrap_or(self.cursor);
        let s = &o.list[o.m[cur]];
        let (bw, br) = box_size(&s.text, bounds);
        let row_y = self.rows.iter().find(|(_, i)| *i == cur).map_or(area.y + 1, |(r, _)| r.y);
        if let Some(r) = dropdown::place_beside(area, row_y, bw, br, room, area) {
            text_box(f, r, &format!("#{}", s.name), &s.text);
        }
    }
}

/// The popup of the `#name` (a shortcut) under the mouse AT, else at TA's cursor: its text in a
/// box above it, inside BOUNDS. 0 popup while the `#` dropdown is open.
/// CURSOR: TA has the keys (its cursor counts).
pub fn render_popup(f: &mut Frame, ta: &TextArea, at: Option<Position>, cursor: bool, bounds: Rect, pick_open: bool) {
    if pick_open {
        return;
    }
    let hit = at.and_then(|a| ta.shortcut_at(Some(a))).or_else(|| if cursor { ta.shortcut_at(None) } else { None });
    let Some((pos, name)) = hit else { return };
    let Some(text) = get(&name) else { return };
    let Some(p) = ta.screen_pos(pos) else { return };
    let (w, rows) = box_size(&text, bounds);
    let anchor = Rect { x: p.x, y: p.y, width: 1, height: 1 };
    let Some(r) = dropdown::place_max(anchor, w, rows, 12, bounds, true) else { return };
    text_box(f, r, &format!("#{name}"), &text);
}

/// Settings → shortcuts: the list left, the name + text of the picked one right.
pub struct Tab {
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
    pub fn new() -> Tab {
        let mut name = TextArea::new(true, "name: letters, digits, _ -");
        name.style = Some(theme::panel());
        let mut text = TextArea::new(false, "the text #name stands for");
        text.style = Some(theme::panel());
        let mut t = Tab { list: Vec::new(), at: 0, focus: Focus::List, name, text, error: None, armed: false, esc_at: None, list_rect: Rect::default(), name_rect: Rect::default(), text_rect: Rect::default(), rows: Vec::new() };
        t.enter();
        t
    }

    /// The tab shows: the file read again.
    pub fn enter(&mut self) {
        self.list = all();
        self.at = self.at.min(self.list.len());
        self.focus = Focus::List;
        self.load();
    }

    /// The inputs show the picked shortcut.
    fn load(&mut self) {
        let (n, t) = self.list.get(self.at).map(|s| (s.name.clone(), s.text.clone())).unwrap_or_default();
        self.name.set_text(&n);
        self.text.set_text(&t);
        self.error = None;
    }

    /// The inputs into the list and the file. False: an invalid name (the error shows).
    fn store(&mut self) -> bool {
        let Some(cur) = self.list.get(self.at).cloned() else { return true };
        let name = self.name.text().trim().to_string();
        let text = self.text.text().trim_end().to_string();
        if !valid_name(&name) {
            self.error = Some("name: letters, digits, _ or - only".into());
            return false;
        }
        if name != cur.name && self.list.iter().any(|s| s.name == name) {
            self.error = Some(format!("#{name} exists already"));
            return false;
        }
        if name == cur.name && text == cur.text {
            return true;
        }
        self.list[self.at] = Shortcut { name: name.clone(), text };
        if let Err(e) = save(&self.list) {
            self.error = Some(format!("save: {e}"));
            return false;
        }
        self.list = all();
        self.at = self.list.iter().position(|s| s.name == name).unwrap_or(0);
        self.error = None;
        true
    }

    fn add(&mut self) {
        let mut k = 1;
        while self.list.iter().any(|s| s.name == format!("new-{k}")) {
            k += 1;
        }
        self.list.push(Shortcut { name: format!("new-{k}"), text: String::new() });
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
        if let Err(e) = save(&self.list) {
            self.error = Some(format!("save: {e}"));
        }
        self.at = self.at.min(self.list.len().saturating_sub(1));
        self.load();
    }

    /// A key. False: ↑ on the 1st row (the sub-tab bar takes the keys).
    pub fn key(&mut self, k: KeyEvent) -> bool {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        match self.focus {
            Focus::List => {
                let n = self.list.len() + 1;
                if !matches!(k.code, KeyCode::Char('d') | KeyCode::Delete) {
                    self.armed = false;
                }
                match k.code {
                    KeyCode::Up | KeyCode::Char('k') if self.at == 0 => return false,
                    KeyCode::Up | KeyCode::Char('k') => {
                        self.at -= 1;
                        self.load();
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        self.at = (self.at + 1).min(n - 1);
                        self.load();
                    }
                    KeyCode::Enter | KeyCode::Right | KeyCode::Char('l') if self.at == self.list.len() => self.add(),
                    KeyCode::Enter | KeyCode::Right | KeyCode::Char('l') => {
                        self.text.move_end();
                        self.focus = Focus::Text;
                    }
                    KeyCode::Char('n') if ctrl || k.modifiers.is_empty() => self.add(),
                    KeyCode::Char('r') if self.at < self.list.len() => {
                        self.name.move_end();
                        self.focus = Focus::Name;
                    }
                    KeyCode::Char('d') | KeyCode::Delete if self.at < self.list.len() => {
                        if self.armed {
                            self.armed = false;
                            self.delete();
                        } else {
                            self.armed = true;
                        }
                    }
                    _ => {}
                }
            }
            // esc in the name or the text: back to the list, 0 save; edits only on a 2nd esc
            Focus::Name | Focus::Text if k.code == KeyCode::Esc => {
                let dirty = self.name.edited() || self.text.edited();
                if !crate::textarea::esc_drops(&mut self.esc_at, dirty) {
                    self.error = Some("esc again: drop the edits · ctrl+s: save".into());
                    return true;
                }

                self.error = None;
                // a new shortcut never saved: it goes
                if self.list.get(self.at).is_some_and(|s| s.text.is_empty() && !all().iter().any(|a| a.name == s.name)) {
                    self.list.remove(self.at);
                    self.at = self.at.min(self.list.len());
                }
                self.load();
                self.focus = Focus::List;
            }
            Focus::Name => match k.code {
                KeyCode::Enter | KeyCode::Tab | KeyCode::Down => {
                    let name = self.name.text().trim().to_string();
                    if !valid_name(&name) {
                        self.error = Some("name: letters, digits, _ or - only".into());
                    } else if self.list.iter().enumerate().any(|(i, s)| i != self.at && s.name == name) {
                        self.error = Some(format!("#{name} exists already"));
                    } else {
                        self.error = None;
                        self.text.move_end();
                        self.focus = Focus::Text;
                    }
                }
                _ => {
                    self.name.key(k);
                }
            },
            Focus::Text => match k.code {
                // ctrl+s / ctrl+enter: saved, back to the list
                KeyCode::Char('s') if ctrl => {
                    if self.store() {
                        self.focus = Focus::List;
                    }
                }
                KeyCode::Enter if k.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER) => {
                    if self.store() {
                        self.focus = Focus::List;
                    }
                }
                KeyCode::BackTab => self.focus = Focus::Name,
                KeyCode::Up if self.text.on_first_row() => self.focus = Focus::Name,
                _ => {
                    if !matches!(self.text.key(k), crate::textarea::Outcome::Handled) && k.code == KeyCode::Enter {
                        self.text.newline();
                    }
                }
            },
        }
        true
    }

    pub fn paste(&mut self, text: &str) {
        match self.focus {
            Focus::Name => self.name.insert(text),
            Focus::Text => self.text.insert(text),
            Focus::List => {}
        }
    }

    pub fn mouse(&mut self, m: MouseEvent) {
        let p = Position { x: m.column, y: m.row };
        let click = matches!(m.kind, MouseEventKind::Down(MouseButton::Left));
        if self.text_rect.contains(p) {
            if click && self.focus == Focus::List {
                self.focus = Focus::Text;
            }
            self.text.mouse(m);
            return;
        }
        if self.name_rect.contains(p) {
            if click {
                self.focus = Focus::Name;
            }
            self.name.mouse(m);
            return;
        }
        if !click || !self.list_rect.contains(p) {
            return;
        }
        if self.focus != Focus::List && !self.store() {
            return;
        }
        self.focus = Focus::List;
        if let Some(&(_, i)) = self.rows.iter().find(|(r, _)| r.contains(p)) {
            if i == self.list.len() {
                self.add();
            } else {
                self.at = i;
                self.load();
            }
        }
    }

    pub fn row_at(&self, p: Position) -> Option<Rect> {
        self.rows.iter().map(|(r, _)| *r).find(|r| r.contains(p))
    }

    pub fn help(&self) -> &'static [(&'static str, &'static str)] {
        match self.focus {
            Focus::List if self.armed => &[("d", "again: delete"), ("↑↓", "keep")],
            Focus::List => &[("↑↓", "move"), ("enter →", "edit text"), ("r", "rename"), ("n", "new"), ("d d", "delete"), ("↑ top", "sub-tabs")],
            Focus::Name => &[("enter ↓", "the text"), ("esc", "back")],
            Focus::Text => &[("ctrl+s", "save"), ("esc", "back"), ("↑ 1st row", "the name"), ("enter", "new line")],
        }
    }

    pub fn draw(&mut self, f: &mut Frame, area: Rect, active: bool) {
        self.rows.clear();
        if area.height < 4 || area.width < 30 {
            return;
        }
        let lw = (area.width / 3).clamp(18, 32);
        let list = Rect { width: lw, ..area };
        self.list_rect = list;
        f.render_widget(Paragraph::new(Line::styled("#name → text", theme::muted())), Rect { height: 1, ..list });
        let n = self.list.len() + 1;
        let shown = usize::from(list.height.saturating_sub(2));
        let off = dropdown::fit(0, self.at, n, shown);
        for k in off..n.min(off + shown) {
            let r = Rect { y: list.y + 2 + (k - off) as u16, height: 1, ..list };
            let on = k == self.at;
            let (label, st) = match self.list.get(k) {
                Some(s) => (format!(" #{}", s.name), theme::text()),
                None => (" + add shortcut".to_string(), theme::accent()),
            };
            let st = match (on, active && self.focus == Focus::List) {
                (true, true) => theme::pill().add_modifier(Modifier::BOLD),
                (true, false) => theme::panel().add_modifier(Modifier::BOLD),
                _ => st,
            };
            let label = crate::nav::fit(&label, usize::from(r.width));
            let pad = usize::from(r.width).saturating_sub(label.width());
            f.render_widget(Paragraph::new(Line::styled(format!("{label}{}", " ".repeat(pad)), st)), r);
            self.rows.push((r, k));
        }
        // right: the name, the text
        let right = Rect { x: list.right() + 2, width: area.width.saturating_sub(lw + 2), ..area };
        if self.at >= self.list.len() {
            self.name_rect = Rect::default();
            self.text_rect = Rect::default();
            f.render_widget(Paragraph::new(Line::styled("enter: a new shortcut. in a prompt: # opens the shortcuts, tab / enter puts the text in", theme::muted())), Rect { height: 1, ..right });
            return;
        }
        let label = |t: &str, on: bool| Span::styled(t.to_string(), if on { theme::accent_bold() } else { theme::muted() });
        f.render_widget(Paragraph::new(Line::from(label("name", self.focus == Focus::Name))), Rect { height: 1, ..right });
        self.name_rect = Rect { y: right.y + 1, height: 1, ..right };
        self.name.render(f, self.name_rect, active && self.focus == Focus::Name);
        let err = self.error.clone();
        let ty = right.y + 3;
        f.render_widget(Paragraph::new(Line::from(label("text", self.focus == Focus::Text))), Rect { y: ty, height: 1, ..right });
        let th = right.bottom().saturating_sub(ty + 2);
        self.text_rect = Rect { y: ty + 1, height: th, ..right };
        self.text.render(f, self.text_rect, active && self.focus == Focus::Text);
        if let Some(e) = err {
            f.render_widget(Paragraph::new(Line::styled(e, Style::default().fg(theme::pal().danger))), Rect { y: right.bottom().saturating_sub(1), height: 1, ..right });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(n: &str, t: &str) -> Shortcut {
        Shortcut { name: n.into(), text: t.into() }
    }

    #[test]
    fn names_expand_at_word_starts_only() {
        let l = vec![s("review", "line 1\nline 2"), s("fix", "fix it")];
        assert_eq!(expand_with(&l, "please #review now"), "please line 1\nline 2 now");
        assert_eq!(expand_with(&l, "#fix"), "fix it");
        // 0 shortcut, a # in a word, a longer name: as typed
        assert_eq!(expand_with(&l, "issue #12 a#fix #fixes"), "issue #12 a#fix #fixes");
        assert_eq!(matches(&l, "fi"), [1]);
        assert_eq!(matches(&l, "e"), [0]);
    }

    #[test]
    fn the_file_round_trips_multiline_texts() {
        let l = vec![s("a", "x\ny 'z'"), s("b-2", "has ''' in it")];
        assert_eq!(parse(&render(&l)).unwrap(), l);
    }

    #[test]
    fn the_dropdown_previews_the_name_and_tab_puts_the_text_in() {
        let _g = crate::engine::ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        save(&[s("review", "review it\nall"), s("rebase", "rebase on main")]).unwrap();
        let mut p = Pick::default();
        let mut ta = TextArea::new(false, "");
        ta.set_text("go #re");
        ta.move_end();
        let k = |c| KeyEvent::new(c, KeyModifiers::NONE);
        // ↓: the 2nd row's name in the text at once; the typed query keeps both rows
        assert!(p.key(k(KeyCode::Down), &mut ta));
        assert_eq!(ta.text(), "go #review");
        assert!(p.key(k(KeyCode::Down), &mut ta));
        assert_eq!(ta.text(), "go #rebase");
        // a name char: the typed query again, the char after it
        assert!(!p.key(k(KeyCode::Char('v')), &mut ta));
        assert_eq!(ta.text(), "go #re");
        ta.insert("v");
        assert!(p.key(k(KeyCode::Tab), &mut ta));
        assert_eq!(ta.text(), "go review it\nall");
        let _ = std::fs::remove_file(path());
    }
}
