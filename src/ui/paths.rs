//! A path typed in a prompt input (`src/`, `./docs/a`, `~/notes/`, `/usr/lo`): a dropdown above the
//! prompt box with the entries of that directory, filtered as the name is typed. Like tab in a
//! shell, without the tab. Relative paths: from the repo / worktree top, else from the thread's dir.
//! ↑↓ move, tab inserts the entry (a dir: with `/`, the dropdown goes on with its entries). enter
//! inserts only after ↑↓ picked a row (else: enter is the input's). esc: closed for this word.
//! ↑↓ put the row's name into the text at once (live preview; the typed name stays the filter).
//! Opens only when the directory exists (`and/or`, `https://…`: 0 dropdown).

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use unicode_width::UnicodeWidthStr;

use crate::ui::dropdown;
use crate::ui::textarea::{Pos, TextArea};
use crate::ui::theme;

/// A directory's entries are read again after this time.
const FRESH: Duration = Duration::from_secs(2);

/// Entries of a directory: (name, is a dir).
type Entries = Vec<(String, bool)>;

#[derive(Default)]
pub(crate) struct Paths {
    /// Relative paths start here: the repo / worktree top (else the dir itself), and the dir.
    roots: Vec<PathBuf>,
    /// esc on the dropdown of the word at this position: closed until another word.
    closed: Option<Pos>,
    cursor: usize,
    offset: usize,
    /// The word the cursor belongs to (new text: the 1st row again), and whether ↑↓ picked a row.
    seen: String,
    moved: bool,
    /// Entries of the last directory read: (name, is a dir), dirs first, by name.
    cache: Option<(PathBuf, Instant, Entries)>,
    /// Rows of the last draw (mouse): area, row.
    rows: Vec<(Rect, usize)>,
    area: Rect,
    /// Dirs only (a dir input: shell mode's dir).
    pub dirs_only: bool,
    /// ↑↓: the row's name in the text (the typed name: the filter).
    preview: dropdown::Preview,
}

/// The open dropdown: where the name starts, the directory (as typed), the matching entries.
struct Open {
    /// Start of the last path part (what a pick replaces).
    at: Pos,
    word_at: Pos,
    dir: String,
    /// The name as typed (the filter; a live preview may sit in the text instead).
    name: String,
    entries: Vec<(String, bool)>,
    /// The word sits after an open `"` (spaces allowed).
    quoted: bool,
}

/// WORD → (directory part with its last `/`, name part): `src/ap` → (`src/`, `ap`).
fn split(word: &str) -> (&str, &str) {
    let i = word.rfind('/').map_or(0, |i| i + 1);

    (&word[..i], &word[i..])
}

/// Entries of DIR: dirs first, then files, by name (case ignored). `.git`: left out.
fn read(dir: &Path) -> Vec<(String, bool)> {
    let mut value: Vec<(String, bool)> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|error| (error.file_name().to_string_lossy().into_owned(), error.path().is_dir()))
        .filter(|(count, _)| count != ".git" && count != ".DS_Store")
        .collect();

    value.sort_by_key(|agent| (!agent.1, agent.0.to_lowercase()));
    value
}

/// Entries that match NAME: those that start with it (case ignored), then those that hold it.
/// Hidden entries: only for a NAME that starts with `.`.
fn filter(entries: &[(String, bool)], name: &str) -> Vec<(String, bool)> {
    let query = name.to_lowercase();
    let shown = |count: &str| query.starts_with('.') || !count.starts_with('.');
    let mut value: Vec<(String, bool)> = entries
        .iter()
        .filter(|(count_value, _)| shown(count_value) && count_value.to_lowercase().starts_with(&query))
        .cloned()
        .collect();

    // `.` + text: hidden entries by prefix only (`.` is in every file name with an extension)
    if !query.starts_with('.') {
        value.extend(
            entries
                .iter()
                .filter(|(count_2, _)| {
                    shown(count_2)
                        && !count_2.to_lowercase().starts_with(&query)
                        && count_2.to_lowercase().contains(&query)
                })
                .cloned(),
        );
    }

    value
}

impl Paths {
    /// A single directory root, without a Git read (config paths are home-relative).
    pub(crate) fn directory(root: &Path) -> Paths {
        Paths {
            roots: vec![root.to_path_buf()],
            ..Default::default()
        }
    }

    /// Another input has focus. Keep directory data, reset input-specific state.
    pub(crate) fn reset_input(&mut self) {
        self.closed = None;
        self.seen.clear();
        self.cursor = 0;
        self.offset = 0;
        self.moved = false;
        self.hide();
    }

    /// CWD: the dir of the task / thread.
    pub(crate) fn new(cwd: &Path) -> Paths {
        let mut roots: Vec<PathBuf> = crate::domain::threads::toplevel(cwd).into_iter().collect();

        if !roots.iter().any(|result| result == cwd) {
            roots.push(cwd.to_path_buf());
        }

        Paths {
            roots,
            ..Default::default()
        }
    }

    /// The directory DIR (as typed) names: absolute, `~/…`, else below a root. None: 0 such dir.
    fn resolve(&self, dir: &str) -> Option<PathBuf> {
        let found = if let Some(path) = crate::infra::config::resolve_alias(dir) {
            Some(path)
        } else if let Some(rest) = dir.strip_prefix("~/") {
            Some(crate::infra::config::home().join(rest))
        } else if dir.starts_with('/') {
            Some(PathBuf::from(dir))
        } else {
            self.roots
                .iter()
                .map(|result| result.join(dir))
                .find(|path_value| path_value.is_dir())
        };

        found.filter(|path_2| path_2.is_dir())
    }

    fn entries(&mut self, dir: &Path) -> Vec<(String, bool)> {
        match &self.cache {
            Some((directory, at, value)) if directory == dir && at.elapsed() < FRESH => value.clone(),
            _ => {
                let value_value = read(dir);

                self.cache = Some((dir.to_path_buf(), Instant::now(), value_value.clone()));
                value_value
            }
        }
    }

    fn open(&mut self, ta: &TextArea) -> Option<Open> {
        let found = ta.path_word_quoted();

        if found.is_none() {
            self.closed = None;
        }

        let (word_at, word, quoted) = found.filter(|(at, _, _)| self.closed != Some(*at))?;
        let (dir, name) = split(&word);

        // a live preview in the text: the name typed before it filters
        let name_at = Pos {
            row: word_at.row,
            col: word_at.col + dir.chars().count(),
        };
        let typed = self
            .preview
            .typed(ta)
            .filter(|(path_value, _)| *path_value == name_at)
            .map(|(_, thread)| thread);
        let name = typed.as_deref().unwrap_or(name);

        // `/` alone (a slash in a sentence), `//`: not a path yet
        if dir == "/" && name.is_empty() || dir.contains("//") {
            return None;
        }

        let path = self.resolve(dir)?;
        let mut all = self.entries(&path);

        if self.dirs_only {
            all.retain(|error| error.1);
        }

        // `/name` can be a slash command: only names that start like it
        let entries: Vec<(String, bool)> = if dir == "/" {
            filter(&all, name)
                .into_iter()
                .filter(|(count_value, _)| count_value.to_lowercase().starts_with(&name.to_lowercase()))
                .collect()
        } else {
            filter(&all, name)
        };

        // 0 matches, or the word is 1 file's full name already: nothing to offer
        if entries.is_empty() || (entries.len() == 1 && entries[0].0 == name && !entries[0].1) {
            return None;
        }

        // the word as typed (a live preview: the typed name), new text: the 1st row again
        let typed_word = format!("{dir}{name}");

        if self.seen != typed_word {
            self.seen = typed_word;
            self.cursor = 0;
            self.offset = 0;
            self.moved = false;
        }

        self.cursor = self.cursor.min(entries.len() - 1);

        Some(Open {
            at: name_at,
            word_at,
            dir: dir.to_string(),
            name: name.to_string(),
            entries,
            quoted,
        })
    }

    #[cfg(test)]
    fn names(&mut self, ta: &TextArea) -> Vec<String> {
        matched_entries(self, ta)
    }

    /// The entry into the text: a dir with `/` (its entries show next), a file with a space. A
    /// name with a space (or a quote char): the path gets a `"` in front (a file: one after it).
    fn pick(&mut self, ta: &mut TextArea, output: &Open, i: usize) {
        insert_entry(self, ta, output, i);
    }

    /// The dropdown shows (a path at the cursor, 1+ entries).
    pub(crate) fn is_open(&mut self, ta: &TextArea) -> bool {
        self.open(ta).is_some()
    }

    /// Key for the dropdown. true: used (the input must not see it).
    pub(crate) fn key(&mut self, key_value: KeyEvent, ta: &mut TextArea) -> bool {
        handle_key(self, key_value, ta)
    }

    /// Not drawn this frame: 0 rows for hover / clicks.
    pub(crate) fn hide(&mut self) {
        self.rows.clear();
        self.area = Rect::default();
    }

    /// Row of the last draw at P (hover).
    pub(crate) fn row_at(&self, path: Position) -> Option<Rect> {
        self.rows
            .iter()
            .map(|(result, _)| *result)
            .find(|result_value| result_value.contains(path))
    }

    /// Click on a row: inserted. Wheel: the cursor. true: used.
    pub(crate) fn mouse(&mut self, event: MouseEvent, ta: &mut TextArea) -> bool {
        handle_mouse(self, event, ta)
    }

    /// The dropdown for the path word of TA (TA rendered this frame), inside BOUNDS and never over
    /// AVOID (the prompt box): above it, at the column of the name (0 room above: below).
    pub(crate) fn render(&mut self, frame_value: &mut Frame, ta: &TextArea, bounds: Rect, avoid: Rect) {
        self.rows.clear();
        self.area = Rect::default();
        let Some(output) = self.open(ta) else { return };
        let Some(path) = ta.screen_pos(output.at) else { return };
        let above = Rect {
            height: avoid.y.saturating_sub(bounds.y).min(bounds.height),
            ..bounds
        };
        let below_y = avoid.bottom().clamp(bounds.y, bounds.bottom());
        let below = Rect {
            y: below_y,
            height: bounds.bottom() - below_y,
            ..bounds
        };
        let up = above.height >= 5 || above.height >= below.height;
        let count = output.entries.len();

        let label: String = { render_directory(&output) };

        let width_value = (output.entries.iter().map(|(name, _)| name.width()).max().unwrap_or(0) + 5)
            .max(label.width() + 6)
            .clamp(24, 72) as u16;

        let area = if up {
            dropdown::place(
                Rect {
                    x: path.x,
                    y: avoid.y,
                    width: 1,
                    height: 1,
                },
                width_value,
                count,
                above,
                true,
            )
        } else {
            dropdown::place(
                Rect {
                    x: path.x,
                    y: avoid.bottom().saturating_sub(1),
                    width: 1,
                    height: 1,
                },
                width_value,
                count,
                below,
                false,
            )
        };

        let Some(area) = area else { return };
        let shown = (area.height as usize).saturating_sub(2);

        draw_entries(frame_value, &(output), count, &(label), area, shown, self);
    }
}

fn render_directory(output: &Open) -> String {
    let directory: Vec<char> = output.dir.chars().collect();

    if directory.len() > 40 {
        format!("…{}", directory[directory.len() - 39..].iter().collect::<String>())
    } else {
        output.dir.clone()
    }
}

#[cfg(test)]
#[path = "paths_tests.rs"]
mod tests;

#[cfg(test)]
fn matched_entries(widget_state: &mut Paths, ta: &TextArea) -> Vec<String> {
    widget_state
        .open(ta)
        .map(|output| {
            output
                .entries
                .into_iter()
                .map(|(count, directory)| if directory { format!("{count}/") } else { count })
                .collect()
        })
        .unwrap_or_default()
}

fn insert_entry(_widget_state: &mut Paths, ta: &mut TextArea, output: &Open, i: usize) {
    let Some((name, dir)) = output.entries.get(i) else {
        return;
    };

    let end = if *dir {
        "/"
    } else if output.quoted {
        "\" "
    } else {
        " "
    };

    if output.quoted
        || !name.chars().any(|character| {
            character.is_whitespace() || matches!(character, '(' | ')' | '[' | ']' | '\'' | '`' | '<' | '>' | ',')
        })
    {
        ta.replace_to_cursor(output.at, &format!("{name}{end}"));
    } else {
        let end = if *dir { "/" } else { "\" " };

        ta.replace_to_cursor(output.word_at, &format!("\"{}{name}{end}", output.dir));
    }
}

fn handle_key(widget_state: &mut Paths, key_value: KeyEvent, ta: &mut TextArea) -> bool {
    let Some(output) = widget_state.open(ta) else {
        return false;
    };

    if !key_value.modifiers.is_empty() {
        return false;
    }

    let count = output.entries.len();

    match key_value.code {
        KeyCode::Up | KeyCode::Down => {
            widget_state.cursor = if key_value.code == KeyCode::Up {
                (widget_state.cursor + count - 1) % count
            } else {
                (widget_state.cursor + 1) % count
            };

            widget_state.moved = true;

            // the row's name in the text at once (a name with a space: on enter / tab only)
            let name = &output.entries[widget_state.cursor].0;

            if output.quoted
                || !name
                    .chars()
                    .any(|character| character.is_whitespace() || character == '"')
            {
                widget_state.preview.show(ta, output.at, &output.name, name);
            }
        }
        KeyCode::Tab => widget_state.pick(ta, &output, widget_state.cursor),
        // enter stays the input's key, unless ↑↓ picked a row
        KeyCode::Enter if widget_state.moved => widget_state.pick(ta, &output, widget_state.cursor),
        KeyCode::Esc => widget_state.closed = Some(output.word_at),
        // a name char after a preview (not `/`: it goes into the previewed dir): the typed
        // name again, the char after it
        KeyCode::Char(character_value)
            if !character_value.is_whitespace() && character_value != '/' && widget_state.preview.revert(ta) =>
        {
            return false;
        }
        _ => return false,
    }

    true
}

fn handle_mouse(widget_state: &mut Paths, event: MouseEvent, ta: &mut TextArea) -> bool {
    let path = Position {
        x: event.column,
        y: event.row,
    };

    if !widget_state.area.contains(path) {
        return false;
    }

    let Some(output) = widget_state.open(ta) else {
        return false;
    };

    let count = output.entries.len();

    match event.kind {
        MouseEventKind::ScrollUp => widget_state.cursor = widget_state.cursor.saturating_sub(3),
        MouseEventKind::ScrollDown => widget_state.cursor = (widget_state.cursor + 3).min(count - 1),
        MouseEventKind::Down(MouseButton::Left) => {
            if let Some(&(_, i)) = widget_state.rows.iter().find(|(result, _)| result.contains(path)) {
                widget_state.pick(ta, &output, i);
            }
        }
        _ => {}
    }

    true
}

fn draw_entries(
    frame_value: &mut Frame<'_>,
    output: &Open,
    count: usize,
    label: &str,
    area: Rect,
    shown: usize,
    widget_state: &mut Paths,
) {
    widget_state.offset = dropdown::fit(widget_state.offset, widget_state.cursor, count, shown);
    let off = widget_state.offset;
    let note = if widget_state.moved { "tab / enter" } else { "tab" };
    let inner = dropdown::frame(
        frame_value,
        area,
        label,
        note,
        dropdown::FrameState {
            up: off > 0,
            down: off + shown < count,
            active: true,
        },
    );

    widget_state.area = area;

    for (i, (name, dir)) in output.entries.iter().enumerate().skip(off).take(shown) {
        draw_entry(frame_value, off, inner, i, name, *dir, widget_state);
    }
}

fn draw_entry(
    frame_value: &mut Frame<'_>,
    off: usize,
    inner: Rect,
    i: usize,
    name: &str,
    dir: bool,
    widget_state: &mut Paths,
) {
    let row = Rect {
        y: inner.y + (i - off) as u16,
        height: 1,
        ..inner
    };
    let hl = i == widget_state.cursor;

    let st = if hl {
        theme::pill().add_modifier(Modifier::BOLD)
    } else if dir {
        Style::default().fg(theme::pal().text).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme::pal().text)
    };

    let dim = if hl { theme::pill() } else { theme::muted() };
    let room = (row.width as usize).saturating_sub(3);

    let text: String = if name.width() > room {
        name.chars().take(room.saturating_sub(1)).chain(['…']).collect()
    } else {
        name.to_owned()
    };

    let line = Line::from(vec![
        Span::styled(format!(" {text}"), st),
        Span::styled(if dir { "/" } else { "" }, dim),
    ]);

    frame_value.render_widget(Paragraph::new(line).style(st), row);
    widget_state.rows.push((row, i));
}
