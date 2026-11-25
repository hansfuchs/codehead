//! Repo picker of the task form (enter on the repo field), 2 panes like yazi. Left: the dirs of
//! the parent dir, the cursor on the current dir. Right: the dirs of the current dir. ↑↓: the
//! current dir moves in the parent; →: into the current dir; ←: up 1 dir; type: filter of the
//! left pane; enter: pick the current dir. Home repos without a clone show in their parent dir
//! (a pick clones them first).

use std::path::{Path, PathBuf};

use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use unicode_width::UnicodeWidthStr;

use crate::ui::dropdown;
use crate::ui::theme;

/// Rows of each pane.
pub(crate) const ROWS: usize = 12;

/// A dir of a pane: a git repo (`.git` in it), a home repo not on disk (its clone URL).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Entry {
    pub name: String,
    pub path: PathBuf,
    pub git: bool,
    pub clone: Option<String>,
}

/// What a key did.
#[derive(Debug, PartialEq)]
pub(crate) enum Act {
    None,
    Close,
    Pick(Entry),
}

pub(crate) struct Picker {
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
    home: Vec<crate::infra::home::Repo>,
    /// Rows drawn last: left (area, index in the shown rows), right (area, index in kids).
    pub left_rows: Vec<(Rect, usize)>,
    pub right_rows: Vec<(Rect, usize)>,
}

/// Dirs of DIR, hidden ones out, sorted by name (case off); HOME repos not on disk with DIR as
/// their parent too.
fn list(dir: &Path, home: &[crate::infra::home::Repo]) -> Vec<Entry> {
    let mut entries: Vec<Entry> = std::fs::read_dir(dir)
        .map(|listing| {
            listing
                .flatten()
                .filter(|entry| !entry.file_name().to_string_lossy().starts_with('.'))
                .map(|entry| entry.path())
                .filter(|path| path.is_dir())
                .map(|path| Entry {
                    name: path
                        .file_name()
                        .map(|name| name.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                    git: path.join(".git").exists(),
                    path,
                    clone: None,
                })
                .collect()
        })
        .unwrap_or_default();

    for repo in home
        .iter()
        .filter(|repo| repo.path.parent() == Some(dir) && !repo.path.exists())
    {
        entries.push(Entry {
            name: repo
                .path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default(),
            path: repo.path.clone(),
            git: false,
            clone: repo.url.clone(),
        });
    }

    entries.sort_by_key(|entry| entry.name.to_lowercase());
    entries
}

impl Picker {
    /// The picker on DIR (the form's repo, else the dir codehead started in).
    pub(crate) fn new(dir: &Path, home: Vec<crate::infra::home::Repo>) -> Self {
        let mut picker = Picker {
            parent: Vec::new(),
            cursor: 0,
            offset: 0,
            kids: Vec::new(),
            kids_offset: 0,
            query: String::new(),
            home,
            left_rows: Vec::new(),
            right_rows: Vec::new(),
        };

        picker.go(dir);
        picker
    }

    /// DIR becomes the current dir: its parent's dirs left (the cursor on DIR), its dirs right.
    fn go(&mut self, dir: &Path) {
        self.query.clear();
        self.offset = 0;

        if let Some(up) = dir.parent() {
            self.list_parent(dir, up);
        } else {
            self.parent = vec![Entry {
                name: "/".into(),
                path: dir.to_path_buf(),
                git: false,
                clone: None,
            }];
            self.cursor = 0;
        }

        self.load_kids();
    }

    fn list_parent(&mut self, dir: &Path, up: &Path) {
        self.parent = list(up, &self.home);

        // a dir the listing leaves out (hidden): in it all the same
        if !self.parent.iter().any(|entry| entry.path == dir) {
            self.parent.push(Entry {
                name: dir
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                git: dir.join(".git").exists(),
                path: dir.to_path_buf(),
                clone: None,
            });
        }

        self.cursor = self.parent.iter().position(|entry| entry.path == dir).unwrap_or(0);
    }

    /// The left pane's rows (indexes of the parent's dirs): the filter's matches, in order.
    pub(crate) fn shown(&self) -> Vec<usize> {
        let query = self.query.trim().to_lowercase();

        (0..self.parent.len())
            .filter(|&i| query.is_empty() || crate::domain::fuzzy::fuzzy(&query, &self.parent[i].name).is_some())
            .collect()
    }

    /// The current dir (under the cursor of the left pane).
    pub(crate) fn current(&self) -> Option<&Entry> {
        self.shown().get(self.cursor).map(|&i| &self.parent[i])
    }

    fn load_kids(&mut self) {
        self.kids_offset = 0;

        self.kids = match self.current() {
            Some(entry) if entry.clone.is_none() => list(&entry.path.clone(), &self.home),
            _ => Vec::new(),
        };
    }

    /// The parent dir (the left pane's title).
    fn parent_dir(&self) -> Option<PathBuf> {
        self.parent
            .first()
            .and_then(|entry| entry.path.parent())
            .map(Path::to_path_buf)
    }

    pub(crate) fn key(&mut self, key: KeyEvent) -> Act {
        let typed = !key
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER);
        let count = self.shown().len();

        match key.code {
            KeyCode::Esc if !self.query.is_empty() => self.set_query(String::new()),
            KeyCode::Esc => return Act::Close,
            KeyCode::Enter => return self.current().cloned().map_or(Act::None, Act::Pick),
            KeyCode::Up if count > 0 => self.step((self.cursor + count - 1) % count),
            KeyCode::Down if count > 0 => self.step((self.cursor + 1) % count),
            KeyCode::PageUp => self.step(self.cursor.saturating_sub(ROWS)),
            KeyCode::PageDown if count > 0 => self.step((self.cursor + ROWS).min(count - 1)),
            KeyCode::Home => self.step(0),
            KeyCode::End if count > 0 => self.step(count - 1),
            KeyCode::Right | KeyCode::Tab => {
                self.enter_first_kid();
            }
            KeyCode::Left | KeyCode::BackTab => self.up(),
            KeyCode::Backspace if typed && self.query.is_empty() => self.up(),
            KeyCode::Backspace if typed => {
                let query: String = self.query.chars().take(self.query.chars().count() - 1).collect();

                self.set_query(query);
            }
            // ~ (0 filter): the home dir
            KeyCode::Char('~') if typed && self.query.is_empty() => self.go(&crate::infra::config::home()),
            KeyCode::Char(character) if typed => {
                let query = format!("{}{character}", self.query);

                self.set_query(query);
            }
            _ => {}
        }

        Act::None
    }

    fn enter_first_kid(&mut self) {
        if let Some(first) = self.kids.first().map(|kid| kid.path.clone()) {
            self.go(&first);
        }
    }

    fn step(&mut self, i: usize) {
        self.cursor = i;
        self.load_kids();
    }

    /// ←: the parent becomes the current dir.
    fn up(&mut self) {
        if let Some(path) = self.parent_dir() {
            self.go(&path);
        }
    }

    /// A new filter: the cursor on the 1st match.
    fn set_query(&mut self, query: String) {
        let at = self.current().map(|entry| entry.path.clone());

        self.query = query;
        let shown = self.shown();

        self.cursor = if self.query.is_empty() {
            at.and_then(|path| shown.iter().position(|&i| self.parent[i].path == path))
                .unwrap_or(0)
        } else {
            0
        };

        self.offset = 0;
        self.load_kids();
    }

    /// A click on a row: left, the current dir; right, into that dir. True: a row took it.
    pub(crate) fn click(&mut self, x: u16, y: u16) -> bool {
        let at = ratatui::layout::Position { x, y };

        if let Some(&(_, i)) = self.left_rows.iter().find(|(rect, _)| rect.contains(at)) {
            self.step(i);
            return true;
        }

        if let Some(&(_, i)) = self.right_rows.iter().find(|(rect, _)| rect.contains(at)) {
            let path = self.kids[i].path.clone();

            self.go(&path);
            return true;
        }

        false
    }

    /// The picker below ANCHOR (the repo chip), else above it, inside BOUNDS. Returns its area.
    pub(crate) fn draw(&mut self, frame: &mut Frame, anchor: Rect, bounds: Rect) -> Option<Rect> {
        let width = bounds.width.min(110);
        let count = self.shown().len().max(self.kids.len());
        let area = dropdown::place_max(Rect { x: bounds.x, ..anchor }, width, count, ROWS, bounds, false)?;
        let parent = self.parent_dir().map_or_else(|| "/".into(), |path| tilde(&path));

        let label = if self.query.is_empty() {
            format!("repo · {parent}")
        } else {
            format!("repo · {parent} › {}", self.query)
        };

        let inner = dropdown::frame(
            frame,
            area,
            &label,
            "↑↓ · ←→ dirs · type: filter · ~ home · enter: pick",
            dropdown::FrameState {
                up: false,
                down: false,
                active: true,
            },
        );
        let half = inner.width / 2;
        let left = Rect { width: half, ..inner };
        let line = Rect {
            x: inner.x + half,
            width: 1,
            ..inner
        };
        let right = Rect {
            x: inner.x + half + 1,
            width: inner.width.saturating_sub(half + 1),
            ..inner
        };

        for y in line.y..line.bottom() {
            frame.buffer_mut()[(line.x, y)]
                .set_symbol("│")
                .set_style(theme::line().bg(theme::pal().on_accent));
        }

        self.draw_parent_rows(frame, left);
        self.draw_child_rows(frame, right);

        Some(area)
    }

    fn draw_parent_rows(&mut self, frame: &mut Frame, left: Rect) {
        let shown = self.shown();
        let rows = usize::from(left.height);

        self.offset = dropdown::fit(self.offset, self.cursor, shown.len(), rows);
        self.left_rows.clear();

        if shown.is_empty() {
            frame.render_widget(
                Paragraph::new(Line::styled(" 0 matches", theme::muted())),
                Rect {
                    height: 1.min(left.height),
                    ..left
                },
            );
        }

        for (j, &i) in shown.iter().enumerate().skip(self.offset).take(rows) {
            let row = Rect {
                y: left.y + (j - self.offset) as u16,
                height: 1,
                ..left
            };

            Self::row(frame, row, &self.parent[i], j == self.cursor);
            self.left_rows.push((row, j));
        }
    }
    fn draw_child_rows(&mut self, frame: &mut Frame, right: Rect) {
        let rows = usize::from(right.height);

        self.right_rows.clear();
        let cur = self.current().cloned();

        let empty = match &cur {
            Some(entry) if entry.clone.is_some() => " 0 clone yet · enter: clone",
            _ => " 0 dirs",
        };

        if self.kids.is_empty() {
            frame.render_widget(
                Paragraph::new(Line::styled(empty, theme::muted())),
                Rect {
                    height: 1.min(right.height),
                    ..right
                },
            );
        }

        for (j, kid) in self.kids.iter().enumerate().skip(self.kids_offset).take(rows) {
            let row = Rect {
                y: right.y + (j - self.kids_offset) as u16,
                height: 1,
                ..right
            };

            Self::row(frame, row, kid, false);
            self.right_rows.push((row, j));
        }
    }

    /// A dir row: its name, `git` / `clone` at the right; CUR: the cursor row.
    fn row(frame: &mut Frame, row: Rect, entry: &Entry, cur: bool) {
        let st = if cur {
            theme::pill().add_modifier(Modifier::BOLD)
        } else if entry.git {
            Style::default().fg(theme::pal().text)
        } else {
            theme::muted()
        };

        let dim = if cur { theme::pill() } else { theme::muted() };

        let detail = if entry.clone.is_some() {
            "clone"
        } else if entry.git {
            "git"
        } else {
            ""
        };

        let room = (row.width as usize).saturating_sub(detail.width() + 3);
        let name = format!("{}/", entry.name);

        let name: String = if name.width() > room {
            name.chars().take(room.saturating_sub(1)).chain(['…']).collect()
        } else {
            name
        };

        let pad = room.saturating_sub(name.width());
        let line = Line::from(vec![
            Span::styled(format!(" {name}"), st),
            Span::styled(" ".repeat(pad + 1), st),
            Span::styled(format!("{detail} "), dim),
        ]);

        frame.render_widget(Paragraph::new(line).style(st), row);
    }
}

#[cfg(test)]
#[path = "start_screen_repo_pick_tests.rs"]
mod tests;

fn tilde(path: &Path) -> String {
    let text = path.display().to_string();
    let home = crate::infra::config::home().display().to_string();

    match text.strip_prefix(&home) {
        Some(rest) if !home.is_empty() => format!("~{rest}"),
        _ => text,
    }
}
