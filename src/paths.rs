//! A path typed in a prompt input (`src/`, `./docs/a`, `~/notes/`, `/usr/lo`): a dropdown above the
//! prompt box with the entries of that directory, filtered as the name is typed. Like tab in a
//! shell, without the tab. Relative paths: from the repo / worktree top, else from the thread's dir.
//! ↑↓ move, tab inserts the entry (a dir: with `/`, the dropdown goes on with its entries). enter
//! inserts only after ↑↓ picked a row (else: enter is the input's). esc: closed for this word.
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

use crate::dropdown;
use crate::textarea::{Pos, TextArea};
use crate::theme;

/// A directory's entries are read again after this time.
const FRESH: Duration = Duration::from_secs(2);

/// Entries of a directory: (name, is a dir).
type Entries = Vec<(String, bool)>;

#[derive(Default)]
pub struct Paths {
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
}

/// The open dropdown: where the name starts, the directory (as typed), the matching entries.
struct Open {
    /// Start of the last path part (what a pick replaces).
    at: Pos,
    word_at: Pos,
    dir: String,
    entries: Vec<(String, bool)>,
}

/// WORD → (directory part with its last `/`, name part): `src/ap` → (`src/`, `ap`).
fn split(word: &str) -> (&str, &str) {
    let i = word.rfind('/').map_or(0, |i| i + 1);
    (&word[..i], &word[i..])
}

/// Entries of DIR: dirs first, then files, by name (case ignored). `.git`: left out.
fn read(dir: &Path) -> Vec<(String, bool)> {
    let mut v: Vec<(String, bool)> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| (e.file_name().to_string_lossy().into_owned(), e.path().is_dir()))
        .filter(|(n, _)| n != ".git" && n != ".DS_Store")
        .collect();
    v.sort_by_key(|a| (!a.1, a.0.to_lowercase()));
    v
}

/// Entries that match NAME: those that start with it (case ignored), then those that hold it.
/// Hidden entries: only for a NAME that starts with `.`.
fn filter(entries: &[(String, bool)], name: &str) -> Vec<(String, bool)> {
    let q = name.to_lowercase();
    let shown = |n: &str| q.starts_with('.') || !n.starts_with('.');
    let mut v: Vec<(String, bool)> = entries.iter().filter(|(n, _)| shown(n) && n.to_lowercase().starts_with(&q)).cloned().collect();
    // `.` + text: hidden entries by prefix only (`.` is in every file name with an extension)
    if !q.starts_with('.') {
        v.extend(entries.iter().filter(|(n, _)| shown(n) && !n.to_lowercase().starts_with(&q) && n.to_lowercase().contains(&q)).cloned());
    }
    v
}

impl Paths {
    /// A single directory root, without a Git read (config paths are home-relative).
    pub fn directory(root: &Path) -> Paths {
        Paths { roots: vec![root.to_path_buf()], ..Default::default() }
    }

    /// Another input has focus. Keep directory data, reset input-specific state.
    pub fn reset_input(&mut self) {
        self.closed = None;
        self.seen.clear();
        self.cursor = 0;
        self.offset = 0;
        self.moved = false;
        self.hide();
    }

    /// CWD: the dir of the task / thread.
    pub fn new(cwd: &Path) -> Paths {
        let (ok, top, _) = crate::threads::git(cwd, &["rev-parse", "--show-toplevel"]);
        let mut roots = Vec::new();
        if ok && !top.trim().is_empty() {
            roots.push(PathBuf::from(top.trim()));
        }
        if !roots.iter().any(|r| r == cwd) {
            roots.push(cwd.to_path_buf());
        }
        Paths { roots, ..Default::default() }
    }

    /// The directory DIR (as typed) names: absolute, `~/…`, else below a root. None: 0 such dir.
    fn resolve(&self, dir: &str) -> Option<PathBuf> {
        let found = if let Some(path) = crate::config::resolve_alias(dir) {
            Some(path)
        } else if let Some(rest) = dir.strip_prefix("~/") {
            Some(crate::config::home().join(rest))
        } else if dir.starts_with('/') {
            Some(PathBuf::from(dir))
        } else {
            self.roots.iter().map(|r| r.join(dir)).find(|p| p.is_dir())
        };
        found.filter(|p| p.is_dir())
    }

    fn entries(&mut self, dir: &Path) -> Vec<(String, bool)> {
        match &self.cache {
            Some((d, at, v)) if d == dir && at.elapsed() < FRESH => v.clone(),
            _ => {
                let v = read(dir);
                self.cache = Some((dir.to_path_buf(), Instant::now(), v.clone()));
                v
            }
        }
    }

    fn open(&mut self, ta: &TextArea) -> Option<Open> {
        let found = ta.path_word();
        if found.is_none() {
            self.closed = None;
        }
        let (word_at, word) = found.filter(|(at, _)| self.closed != Some(*at))?;
        let (dir, name) = split(&word);
        // `/` alone (a slash in a sentence), `//`: not a path yet
        if dir == "/" && name.is_empty() || dir.contains("//") {
            return None;
        }
        let path = self.resolve(dir)?;
        let mut all = self.entries(&path);
        if self.dirs_only {
            all.retain(|e| e.1);
        }
        // `/name` can be a slash command: only names that start like it
        let entries: Vec<(String, bool)> = if dir == "/" { filter(&all, name).into_iter().filter(|(n, _)| n.to_lowercase().starts_with(&name.to_lowercase())).collect() } else { filter(&all, name) };
        // 0 matches, or the word is 1 file's full name already: nothing to offer
        if entries.is_empty() || (entries.len() == 1 && entries[0].0 == name && !entries[0].1) {
            return None;
        }
        if self.seen != word {
            self.seen = word.clone();
            self.cursor = 0;
            self.offset = 0;
            self.moved = false;
        }
        self.cursor = self.cursor.min(entries.len() - 1);
        let at = Pos { row: word_at.row, col: word_at.col + dir.chars().count() };
        Some(Open { at, word_at, dir: dir.to_string(), entries })
    }

    #[cfg(test)]
    fn names(&mut self, ta: &TextArea) -> Vec<String> {
        self.open(ta).map(|o| o.entries.into_iter().map(|(n, d)| if d { format!("{n}/") } else { n }).collect()).unwrap_or_default()
    }

    /// The entry into the text: a dir with `/` (its entries show next), a file with a space.
    fn pick(&mut self, ta: &mut TextArea, o: &Open, i: usize) {
        let Some((name, dir)) = o.entries.get(i) else { return };
        ta.replace_to_cursor(o.at, &format!("{name}{}", if *dir { "/" } else { " " }));
    }

    /// Key for the dropdown. true: used (the input must not see it).
    pub fn key(&mut self, k: KeyEvent, ta: &mut TextArea) -> bool {
        let Some(o) = self.open(ta) else { return false };
        if !k.modifiers.is_empty() {
            return false;
        }
        let n = o.entries.len();
        match k.code {
            KeyCode::Up => {
                self.cursor = (self.cursor + n - 1) % n;
                self.moved = true;
            }
            KeyCode::Down => {
                self.cursor = (self.cursor + 1) % n;
                self.moved = true;
            }
            KeyCode::Tab => self.pick(ta, &o, self.cursor),
            // enter stays the input's key, unless ↑↓ picked a row
            KeyCode::Enter if self.moved => self.pick(ta, &o, self.cursor),
            KeyCode::Esc => self.closed = Some(o.word_at),
            _ => return false,
        }
        true
    }

    /// Not drawn this frame: 0 rows for hover / clicks.
    pub fn hide(&mut self) {
        self.rows.clear();
        self.area = Rect::default();
    }

    /// Row of the last draw at P (hover).
    pub fn row_at(&self, p: Position) -> Option<Rect> {
        self.rows.iter().map(|(r, _)| *r).find(|r| r.contains(p))
    }

    /// Click on a row: inserted. Wheel: the cursor. true: used.
    pub fn mouse(&mut self, m: MouseEvent, ta: &mut TextArea) -> bool {
        let p = Position { x: m.column, y: m.row };
        if !self.area.contains(p) {
            return false;
        }
        let Some(o) = self.open(ta) else { return false };
        let n = o.entries.len();
        match m.kind {
            MouseEventKind::ScrollUp => self.cursor = self.cursor.saturating_sub(3),
            MouseEventKind::ScrollDown => self.cursor = (self.cursor + 3).min(n - 1),
            MouseEventKind::Down(MouseButton::Left) => {
                if let Some(&(_, i)) = self.rows.iter().find(|(r, _)| r.contains(p)) {
                    self.pick(ta, &o, i);
                }
            }
            _ => {}
        }
        true
    }

    /// The dropdown for the path word of TA (TA rendered this frame), inside BOUNDS and never over
    /// AVOID (the prompt box): above it, at the column of the name (0 room above: below).
    pub fn render(&mut self, f: &mut Frame, ta: &TextArea, bounds: Rect, avoid: Rect) {
        self.rows.clear();
        self.area = Rect::default();
        let Some(o) = self.open(ta) else { return };
        let Some(p) = ta.screen_pos(o.at) else { return };
        let above = Rect { height: avoid.y.saturating_sub(bounds.y).min(bounds.height), ..bounds };
        let below_y = avoid.bottom().clamp(bounds.y, bounds.bottom());
        let below = Rect { y: below_y, height: bounds.bottom() - below_y, ..bounds };
        let up = above.height >= 5 || above.height >= below.height;
        let n = o.entries.len();
        let label: String = {
            let d: Vec<char> = o.dir.chars().collect();
            if d.len() > 40 { format!("…{}", d[d.len() - 39..].iter().collect::<String>()) } else { o.dir.clone() }
        };
        let w = (o.entries.iter().map(|(name, _)| name.width()).max().unwrap_or(0) + 5).max(label.width() + 6).clamp(24, 72) as u16;
        let area = if up {
            dropdown::place(Rect { x: p.x, y: avoid.y, width: 1, height: 1 }, w, n, above, true)
        } else {
            dropdown::place(Rect { x: p.x, y: avoid.bottom().saturating_sub(1), width: 1, height: 1 }, w, n, below, false)
        };
        let Some(area) = area else { return };
        let shown = (area.height as usize).saturating_sub(2);
        self.offset = dropdown::fit(self.offset, self.cursor, n, shown);
        let off = self.offset;
        let note = if self.moved { "tab / enter" } else { "tab" };
        let inner = dropdown::frame(f, area, &label, note, off > 0, off + shown < n, true);
        self.area = area;
        for (i, (name, dir)) in o.entries.iter().enumerate().skip(off).take(shown) {
            let row = Rect { y: inner.y + (i - off) as u16, height: 1, ..inner };
            let hl = i == self.cursor;
            let st = if hl { theme::pill().add_modifier(Modifier::BOLD) } else if *dir { Style::default().fg(theme::pal().text).add_modifier(Modifier::BOLD) } else { Style::default().fg(theme::pal().text) };
            let dim = if hl { theme::pill() } else { theme::muted() };
            let room = (row.width as usize).saturating_sub(3);
            let text: String = if name.width() > room { name.chars().take(room.saturating_sub(1)).chain(['…']).collect() } else { name.clone() };
            let line = Line::from(vec![Span::styled(format!(" {text}"), st), Span::styled(if *dir { "/" } else { "" }, dim)]);
            f.render_widget(Paragraph::new(line).style(st), row);
            self.rows.push((row, i));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::crossterm::event::KeyModifiers;

    fn key(p: &mut Paths, ta: &mut TextArea, code: KeyCode) -> bool {
        p.key(KeyEvent::new(code, KeyModifiers::NONE), ta)
    }

    fn typed(ta: &mut TextArea, text: &str) {
        ta.set_text(text);
        ta.move_end();
    }

    #[test]
    fn lists_the_directory_as_the_path_is_typed() {
        let dir = std::env::temp_dir().join(format!("codehead-paths-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for d in ["apps/web/src", "apps/api", "docs", ".hidden"] {
            std::fs::create_dir_all(dir.join(d)).unwrap();
        }
        for f in ["apps/readme.md", "apps/web/index.ts", "apps/.env", "notes.txt"] {
            std::fs::write(dir.join(f), "x").unwrap();
        }
        let mut p = Paths::new(&dir);
        let mut ta = TextArea::new(false, "");
        // relative: `apps/` → its entries, dirs first; hidden ones only after a `.`
        typed(&mut ta, "look at apps/");
        assert_eq!(p.names(&ta), ["api/", "web/", "readme.md"]);
        typed(&mut ta, "look at apps/.");
        assert_eq!(p.names(&ta), [".env"]);
        // typed name: entries that start with it, then entries that hold it
        typed(&mut ta, "apps/e");
        assert_eq!(p.names(&ta), ["web/", "readme.md"]);
        // tab: the entry in; a dir keeps the dropdown open on its entries
        typed(&mut ta, "see apps/w");
        assert!(key(&mut p, &mut ta, KeyCode::Tab));
        assert_eq!(ta.text(), "see apps/web/");
        assert_eq!(p.names(&ta), ["src/", "index.ts"]);
        // enter: the input's key, until ↑↓ picked a row
        assert!(!key(&mut p, &mut ta, KeyCode::Enter));
        assert!(key(&mut p, &mut ta, KeyCode::Down));
        assert!(key(&mut p, &mut ta, KeyCode::Enter));
        assert_eq!(ta.text(), "see apps/web/index.ts ");
        assert!(p.names(&ta).is_empty());
        // a full file name: nothing to offer
        typed(&mut ta, "apps/web/index.ts");
        assert!(p.names(&ta).is_empty());
        // absolute and ./ paths
        typed(&mut ta, &format!("{}/apps/", dir.display()));
        assert_eq!(p.names(&ta), ["api/", "web/", "readme.md"]);
        typed(&mut ta, "./do");
        assert_eq!(p.names(&ta), ["docs/"]);
        // 0 such dir, a url, a lone slash, an @ word: 0 dropdown, keys pass
        for text in ["and/or", "https://example.com/a", "a / b /", "@apps/", "x >apps/"] {
            typed(&mut ta, text);
            assert!(p.names(&ta).is_empty(), "{text}");
            assert!(!key(&mut p, &mut ta, KeyCode::Tab), "{text}");
        }
        // esc: closed for this word; a new word opens again
        typed(&mut ta, "apps/");
        assert!(key(&mut p, &mut ta, KeyCode::Esc));
        assert!(p.names(&ta).is_empty());
        ta.insert("w");
        assert!(p.names(&ta).is_empty());
        ta.insert(" docs/../apps/");
        assert_eq!(p.names(&ta).len(), 3);
        // drawn above the prompt box
        let backend = ratatui::backend::TestBackend::new(60, 20);
        let mut term = ratatui::Terminal::new(backend).unwrap();
        let input = Rect::new(0, 14, 60, 4);
        term.draw(|f| {
            ta.render(f, input, true);
            p.render(f, &ta, f.area(), input);
        })
        .unwrap();
        assert!(p.area.height == 5 && p.area.bottom() <= input.y, "{:?}", p.area);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn root_entries_only_by_prefix() {
        let mut p = Paths::new(&std::env::temp_dir());
        let mut ta = TextArea::new(false, "");
        // `/us` → /usr (a prefix); `/zz-no-such` (a slash command): 0 dropdown
        typed(&mut ta, "/us");
        assert!(p.names(&ta).iter().any(|n| n == "usr/"), "{:?}", p.names(&ta));
        typed(&mut ta, "/general:antislop");
        assert!(p.names(&ta).is_empty());
        typed(&mut ta, "/usr/");
        assert!(p.names(&ta).iter().any(|n| n == "bin/"));
    }

    #[test]
    fn configured_aliases_complete_paths_and_expand_sent_text() {
        // The child isolates CODEHEAD_CONFIG and the process-wide config cache.
        if let Some(root) = std::env::var_os("CODEHEAD_ALIAS_TEST_ROOT") {
            let root = PathBuf::from(root);
            let mut paths = Paths::new(&root.join("elsewhere"));
            let mut input = TextArea::new(false, "");
            typed(&mut input, "read project:platform/");
            assert_eq!(paths.names(&input), ["reports/", "notes.md"]);
            assert!(key(&mut paths, &mut input, KeyCode::Tab));
            assert_eq!(input.text(), "read project:platform/reports/");
            assert_eq!(paths.names(&input), ["review.md"]);
            assert!(key(&mut paths, &mut input, KeyCode::Tab));
            assert_eq!(input.text(), "read project:platform/reports/review.md ");
            assert_eq!(crate::config::expand_aliases(&input.text()), format!("read {}/reports/review.md ", root.join("notes").display()));
            typed(&mut input, "project:platform/../");
            assert!(paths.names(&input).contains(&"elsewhere/".to_string()));
            return;
        }
        let root = std::env::temp_dir().join(format!("codehead-alias-path-{}", std::process::id()));
        std::fs::create_dir_all(root.join("notes/reports")).unwrap();
        std::fs::create_dir_all(root.join("elsewhere")).unwrap();
        std::fs::write(root.join("notes/notes.md"), "notes").unwrap();
        std::fs::write(root.join("notes/reports/review.md"), "review").unwrap();
        let file = crate::config::File { aliases: [("project:platform".into(), root.join("notes").display().to_string())].into(), ..Default::default() };
        crate::config::save_file(&root.join("codehead.toml"), "", &file).unwrap();
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "paths::tests::configured_aliases_complete_paths_and_expand_sent_text", "--nocapture"])
            .env("CODEHEAD_ALIAS_TEST_ROOT", &root).env("CODEHEAD_CONFIG", root.join("codehead.toml")).output().unwrap();
        assert!(output.status.success(), "{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
        let _ = std::fs::remove_dir_all(root);
    }
}
