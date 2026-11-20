//! Repo picker of the task form (enter on the repo field), 2 panes like yazi. Left: the dirs of
//! the parent dir, the cursor on the current dir. Right: the dirs of the current dir. ↑↓: the
//! current dir moves in the parent; →: into the current dir; ←: up 1 dir; type: filter of the
//! left pane; enter: pick the current dir. Home repos without a clone show in their parent dir
//! (a pick clones them first).

use std::path::{Path, PathBuf};

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use unicode_width::UnicodeWidthStr;

use crate::dropdown;
use crate::theme;

/// Rows of each pane.
pub const ROWS: usize = 12;

/// A dir of a pane: a git repo (`.git` in it), a home repo not on disk (its clone URL).
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub name: String,
    pub path: PathBuf,
    pub git: bool,
    pub clone: Option<String>,
}

/// What a key did.
#[derive(Debug, PartialEq)]
pub enum Act {
    None,
    Close,
    Pick(Entry),
}

pub struct Picker {
    /// The dirs of the parent (left pane), the current dir's row, the 1st row in view.
    parent: Vec<Entry>,
    pub cursor: usize,
    offset: usize,
    /// The dirs of the current dir (right pane), the 1st row in view.
    kids: Vec<Entry>,
    kids_offset: usize,
    /// Filter of the left pane (typed).
    pub query: String,
    /// Home repos (clones not on disk show as entries).
    home: Vec<crate::home::Repo>,
    /// Rows drawn last: left (area, index in the shown rows), right (area, index in kids).
    pub left_rows: Vec<(Rect, usize)>,
    pub right_rows: Vec<(Rect, usize)>,
}

/// Dirs of DIR, hidden ones out, sorted by name (case off); HOME repos not on disk with DIR as
/// their parent too.
fn list(dir: &Path, home: &[crate::home::Repo]) -> Vec<Entry> {
    let mut v: Vec<Entry> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .filter(|e| !e.file_name().to_string_lossy().starts_with('.'))
                .map(|e| e.path())
                .filter(|p| p.is_dir())
                .map(|p| Entry { name: p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), git: p.join(".git").exists(), path: p, clone: None })
                .collect()
        })
        .unwrap_or_default();
    for r in home.iter().filter(|r| r.path.parent() == Some(dir) && !r.path.exists()) {
        v.push(Entry { name: r.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), path: r.path.clone(), git: false, clone: r.url.clone() });
    }
    v.sort_by_key(|e| e.name.to_lowercase());
    v
}

impl Picker {
    /// The picker on DIR (the form's repo, else the dir codehead started in).
    pub fn new(dir: &Path, home: Vec<crate::home::Repo>) -> Self {
        let mut p = Picker { parent: Vec::new(), cursor: 0, offset: 0, kids: Vec::new(), kids_offset: 0, query: String::new(), home, left_rows: Vec::new(), right_rows: Vec::new() };
        p.go(dir);
        p
    }

    /// DIR becomes the current dir: its parent's dirs left (the cursor on DIR), its dirs right.
    fn go(&mut self, dir: &Path) {
        self.query.clear();
        self.offset = 0;
        match dir.parent() {
            Some(up) => {
                self.parent = list(up, &self.home);
                // a dir the listing leaves out (hidden): in it all the same
                if !self.parent.iter().any(|e| e.path == dir) {
                    self.parent.push(Entry { name: dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), git: dir.join(".git").exists(), path: dir.to_path_buf(), clone: None });
                }
                self.cursor = self.parent.iter().position(|e| e.path == dir).unwrap_or(0);
            }
            None => {
                self.parent = vec![Entry { name: "/".into(), path: dir.to_path_buf(), git: false, clone: None }];
                self.cursor = 0;
            }
        }
        self.load_kids();
    }

    /// The left pane's rows (indexes of the parent's dirs): the filter's matches, in order.
    pub fn shown(&self) -> Vec<usize> {
        let q = self.query.trim().to_lowercase();
        (0..self.parent.len()).filter(|&i| q.is_empty() || crate::app::fuzzy(&q, &self.parent[i].name).is_some()).collect()
    }

    /// The current dir (under the cursor of the left pane).
    pub fn current(&self) -> Option<&Entry> {
        self.shown().get(self.cursor).map(|&i| &self.parent[i])
    }

    fn load_kids(&mut self) {
        self.kids_offset = 0;
        self.kids = match self.current() {
            Some(e) if e.clone.is_none() => list(&e.path.clone(), &self.home),
            _ => Vec::new(),
        };
    }

    /// The parent dir (the left pane's title).
    fn parent_dir(&self) -> Option<PathBuf> {
        self.parent.first().and_then(|e| e.path.parent()).map(Path::to_path_buf)
    }

    pub fn key(&mut self, k: KeyEvent) -> Act {
        let typed = !k.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER);
        let n = self.shown().len();
        match k.code {
            KeyCode::Esc if !self.query.is_empty() => self.set_query(String::new()),
            KeyCode::Esc => return Act::Close,
            KeyCode::Enter => {
                if let Some(e) = self.current() {
                    return Act::Pick(e.clone());
                }
            }
            KeyCode::Up if n > 0 => self.step((self.cursor + n - 1) % n),
            KeyCode::Down if n > 0 => self.step((self.cursor + 1) % n),
            KeyCode::PageUp => self.step(self.cursor.saturating_sub(ROWS)),
            KeyCode::PageDown if n > 0 => self.step((self.cursor + ROWS).min(n - 1)),
            KeyCode::Home => self.step(0),
            KeyCode::End if n > 0 => self.step(n - 1),
            KeyCode::Right | KeyCode::Tab => {
                if let Some(first) = self.kids.first().map(|e| e.path.clone()) {
                    self.go(&first);
                }
            }
            KeyCode::Left | KeyCode::BackTab => self.up(),
            KeyCode::Backspace if typed && self.query.is_empty() => self.up(),
            KeyCode::Backspace if typed => {
                let q: String = self.query.chars().take(self.query.chars().count() - 1).collect();
                self.set_query(q);
            }
            // ~ (0 filter): the home dir
            KeyCode::Char('~') if typed && self.query.is_empty() => self.go(&crate::config::home()),
            KeyCode::Char(c) if typed => {
                let q = format!("{}{c}", self.query);
                self.set_query(q);
            }
            _ => {}
        }
        Act::None
    }

    fn step(&mut self, i: usize) {
        self.cursor = i;
        self.load_kids();
    }

    /// ←: the parent becomes the current dir.
    fn up(&mut self) {
        if let Some(p) = self.parent_dir() {
            self.go(&p);
        }
    }

    /// A new filter: the cursor on the 1st match.
    fn set_query(&mut self, q: String) {
        let at = self.current().map(|e| e.path.clone());
        self.query = q;
        let shown = self.shown();
        self.cursor = if self.query.is_empty() { at.and_then(|p| shown.iter().position(|&i| self.parent[i].path == p)).unwrap_or(0) } else { 0 };
        self.offset = 0;
        self.load_kids();
    }

    /// A click on a row: left, the current dir; right, into that dir. True: a row took it.
    pub fn click(&mut self, x: u16, y: u16) -> bool {
        let at = ratatui::layout::Position { x, y };
        if let Some(&(_, i)) = self.left_rows.iter().find(|(r, _)| r.contains(at)) {
            self.step(i);
            return true;
        }
        if let Some(&(_, i)) = self.right_rows.iter().find(|(r, _)| r.contains(at)) {
            let p = self.kids[i].path.clone();
            self.go(&p);
            return true;
        }
        false
    }

    /// The picker below ANCHOR (the repo chip), else above it, inside BOUNDS. Returns its area.
    pub fn draw(&mut self, f: &mut Frame, anchor: Rect, bounds: Rect) -> Option<Rect> {
        let w = bounds.width.min(110);
        let n = self.shown().len().max(self.kids.len());
        let r = dropdown::place_max(Rect { x: bounds.x, ..anchor }, w, n, ROWS, bounds, false)?;
        let parent = self.parent_dir().map(|p| tilde(&p)).unwrap_or_else(|| "/".into());
        let label = if self.query.is_empty() { format!("repo · {parent}") } else { format!("repo · {parent} › {}", self.query) };
        let inner = dropdown::frame(f, r, &label, "↑↓ · ←→ dirs · type: filter · ~ home · enter: pick", false, false, true);
        let half = inner.width / 2;
        let left = Rect { width: half, ..inner };
        let line = Rect { x: inner.x + half, width: 1, ..inner };
        let right = Rect { x: inner.x + half + 1, width: inner.width.saturating_sub(half + 1), ..inner };
        for y in line.y..line.bottom() {
            f.buffer_mut()[(line.x, y)].set_symbol("│").set_style(theme::line().bg(theme::pal().on_accent));
        }
        let shown = self.shown();
        let rows = inner.height as usize;
        self.offset = dropdown::fit(self.offset, self.cursor, shown.len(), rows);
        self.left_rows.clear();
        if shown.is_empty() {
            f.render_widget(Paragraph::new(Line::styled(" 0 matches", theme::muted())), Rect { height: 1.min(left.height), ..left });
        }
        for (j, &i) in shown.iter().enumerate().skip(self.offset).take(rows) {
            let row = Rect { y: left.y + (j - self.offset) as u16, height: 1, ..left };
            Self::row(f, row, &self.parent[i], j == self.cursor);
            self.left_rows.push((row, j));
        }
        self.right_rows.clear();
        let cur = self.current().cloned();
        let empty = match &cur {
            Some(e) if e.clone.is_some() => " 0 clone yet · enter: clone",
            _ => " 0 dirs",
        };
        if self.kids.is_empty() {
            f.render_widget(Paragraph::new(Line::styled(empty, theme::muted())), Rect { height: 1.min(right.height), ..right });
        }
        for (j, e) in self.kids.iter().enumerate().skip(self.kids_offset).take(rows) {
            let row = Rect { y: right.y + (j - self.kids_offset) as u16, height: 1, ..right };
            Self::row(f, row, e, false);
            self.right_rows.push((row, j));
        }
        Some(r)
    }

    /// A dir row: its name, `git` / `clone` at the right; CUR: the cursor row.
    fn row(f: &mut Frame, row: Rect, e: &Entry, cur: bool) {
        let st = if cur { theme::pill().add_modifier(Modifier::BOLD) } else if e.git { Style::default().fg(theme::pal().text) } else { theme::muted() };
        let dim = if cur { theme::pill() } else { theme::muted() };
        let detail = if e.clone.is_some() { "clone" } else if e.git { "git" } else { "" };
        let room = (row.width as usize).saturating_sub(detail.width() + 3);
        let name = format!("{}/", e.name);
        let name: String = if name.width() > room { name.chars().take(room.saturating_sub(1)).chain(['…']).collect() } else { name };
        let pad = room.saturating_sub(name.width());
        let line = Line::from(vec![Span::styled(format!(" {name}"), st), Span::styled(" ".repeat(pad + 1), st), Span::styled(format!("{detail} "), dim)]);
        f.render_widget(Paragraph::new(line).style(st), row);
    }
}

fn tilde(p: &Path) -> String {
    let s = p.display().to_string();
    let h = crate::config::home().display().to_string();
    match s.strip_prefix(&h) {
        Some(rest) if !h.is_empty() => format!("~{rest}"),
        _ => s,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(p: &mut Picker, c: KeyCode) -> Act {
        p.key(KeyEvent::new(c, KeyModifiers::NONE))
    }

    #[test]
    fn two_panes_move_in_out_filter_and_pick() {
        let base = std::env::temp_dir().join(format!("ch-repopick-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        for d in ["alpha/.git", "alpha/src", "beta", "gamma/.git", ".hidden"] {
            std::fs::create_dir_all(base.join(d)).unwrap();
        }
        let home = vec![crate::home::Repo { name: "zeta".into(), path: base.join("zeta"), url: Some("git@x:y/zeta.git".into()) }];
        let mut p = Picker::new(&base.join("beta"), home);
        let names = |v: &[Entry]| v.iter().map(|e| e.name.clone()).collect::<Vec<_>>();
        // left: the parent's dirs (0 hidden, the home clone too), the cursor on beta; right: beta's
        assert_eq!(names(&p.parent), ["alpha", "beta", "gamma", "zeta"]);
        assert_eq!(p.current().map(|e| e.name.as_str()), Some("beta"));
        assert!(p.kids.is_empty());
        key(&mut p, KeyCode::Up);
        assert_eq!(names(&p.kids), ["src"]);
        assert!(p.current().unwrap().git);
        // →: into alpha (src current), ←: back up (alpha current)
        key(&mut p, KeyCode::Right);
        assert_eq!(p.current().map(|e| e.name.as_str()), Some("src"));
        key(&mut p, KeyCode::Left);
        assert_eq!(p.current().map(|e| e.name.as_str()), Some("alpha"));
        // type: filter, the cursor on the 1st match; esc clears it, the 2nd esc closes
        for c in "gam".chars() {
            key(&mut p, KeyCode::Char(c));
        }
        assert_eq!(p.current().map(|e| e.name.as_str()), Some("gamma"));
        assert_eq!(key(&mut p, KeyCode::Esc), Act::None);
        assert_eq!(p.current().map(|e| e.name.as_str()), Some("gamma"));
        // the home repo not on disk: enter picks it with its URL
        key(&mut p, KeyCode::Down);
        let Act::Pick(e) = key(&mut p, KeyCode::Enter) else { panic!() };
        assert_eq!((e.name.as_str(), e.clone.as_deref()), ("zeta", Some("git@x:y/zeta.git")));
        assert_eq!(key(&mut p, KeyCode::Esc), Act::Close);
        // drawn: both panes side by side
        let mut t = ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 20)).unwrap();
        key(&mut p, KeyCode::Up);
        t.draw(|f| {
            p.draw(f, Rect { x: 0, y: 0, width: 20, height: 1 }, f.area());
        })
        .unwrap();
        let rows: Vec<String> = (0..20u16).map(|y| (0..80u16).map(|x| t.backend().buffer()[(x, y)].symbol().to_string()).collect()).collect();
        assert!(rows.iter().any(|l| l.contains("gamma/") && l.contains('│')), "{rows:#?}");
        assert!(rows.iter().any(|l| l.contains("alpha/") && l.contains("git")), "{rows:#?}");
        let _ = std::fs::remove_dir_all(base);
    }
}
