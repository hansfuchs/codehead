//! Repo filter of both threads tabs (start screen, F2 menu): a chip `repo › all ▾` and its
//! dropdown (all repos, each repo of the rows with its row count, rows outside any repo).
//! ←→ on the list: the previous / next repo at once. The menu command "filter by repo" or a
//! click on the chip opens the dropdown; ↑↓ move, type: filter, enter: pick, esc: close.
//! The last pick holds for the session in both tabs.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::{Position, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use unicode_width::UnicodeWidthStr;

use crate::dropdown;
use crate::theme;

/// Pick of a filter: None = every repo; Some(empty path) = rows outside any git repo.
pub type Choice = Option<PathBuf>;

/// Last pick of the session (both tabs share it).
static LAST: Mutex<Choice> = Mutex::new(None);

/// 1 dropdown row: its pick, label (repo dir name), detail (count, path).
#[derive(Clone, Debug, PartialEq)]
pub struct Opt {
    pub value: Choice,
    pub label: String,
    pub detail: String,
}

fn outside() -> PathBuf {
    PathBuf::new()
}

/// `~/…` form of P.
fn short(p: &Path) -> String {
    let home = std::env::var("HOME").unwrap_or_default();
    match p.strip_prefix(&home) {
        Ok(rest) if !home.is_empty() => format!("~/{}", rest.display()),
        _ => p.display().to_string(),
    }
}

/// Dropdown rows for the repos of the rows: all, then each repo (most rows first, then A→Z),
/// then rows outside any repo (when some are).
pub fn options(repos: &[Option<&Path>]) -> Vec<Opt> {
    let mut counts: Vec<(PathBuf, usize)> = Vec::new();
    let mut none = 0;
    for r in repos {
        match r {
            Some(p) => match counts.iter_mut().find(|(q, _)| q == p) {
                Some(c) => c.1 += 1,
                None => counts.push((p.to_path_buf(), 1)),
            },
            None => none += 1,
        }
    }
    let name = |p: &Path| p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| p.display().to_string());
    counts.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| name(&a.0).cmp(&name(&b.0))));
    let mut v = vec![Opt { value: None, label: "all repos".into(), detail: repos.len().to_string() }];
    v.extend(counts.into_iter().map(|(p, n)| Opt { label: name(&p), detail: format!("{n} · {}", short(&p)), value: Some(p) }));
    if none > 0 {
        v.push(Opt { value: Some(outside()), label: "outside git repos".into(), detail: none.to_string() });
    }
    v
}

/// Open dropdown: cursor, 1st row in view, query.
struct Open {
    cursor: usize,
    offset: usize,
    query: String,
}

pub struct RepoFilter {
    pub choice: Choice,
    open: Option<Open>,
    /// Last draw: the chip, the dropdown rows (area, index in OPTIONS).
    chip: Rect,
    rows: Vec<(Rect, usize)>,
    area: Rect,
}

impl Default for RepoFilter {
    fn default() -> Self {
        Self::new()
    }
}

impl RepoFilter {
    /// The session's last pick (tests: all repos).
    pub fn new() -> RepoFilter {
        let choice = if cfg!(test) { None } else { LAST.lock().map(|c| c.clone()).unwrap_or_default() };
        RepoFilter { choice, open: None, chip: Rect::default(), rows: Vec::new(), area: Rect::default() }
    }

    /// The session's last pick, if another tab changed it.
    pub fn sync(&mut self) {
        if !cfg!(test)
            && let Ok(c) = LAST.lock()
        {
            self.choice = c.clone();
        }
    }

    /// A row of REPO passes the filter.
    pub fn keeps(&self, repo: Option<&Path>) -> bool {
        match &self.choice {
            None => true,
            Some(p) if p.as_os_str().is_empty() => repo.is_none(),
            Some(p) => repo == Some(p.as_path()),
        }
    }

    pub fn is_open(&self) -> bool {
        self.open.is_some()
    }

    /// Opens the dropdown, its cursor on the current pick.
    pub fn open(&mut self, opts: &[Opt]) {
        let cursor = opts.iter().position(|o| o.value == self.choice).unwrap_or(0);
        self.open = Some(Open { cursor, offset: 0, query: String::new() });
    }

    pub fn close(&mut self) {
        self.open = None;
    }

    /// The pick from the dropdown. True: the choice changed.
    fn pick(&mut self, value: Choice) -> bool {
        self.open = None;
        if !cfg!(test)
            && let Ok(mut c) = LAST.lock()
        {
            *c = value.clone();
        }
        let changed = self.choice != value;
        self.choice = value;
        changed
    }

    /// Options after the query of the open dropdown: (index in OPTS, marks).
    fn shown(&self, opts: &[Opt]) -> Vec<(usize, Vec<usize>)> {
        let q = self.open.as_ref().map(|o| o.query.trim().to_lowercase()).unwrap_or_default();
        if q.is_empty() {
            return (0..opts.len()).map(|i| (i, Vec::new())).collect();
        }
        let mut v: Vec<(i64, usize, Vec<usize>)> = opts.iter().enumerate().filter_map(|(i, o)| crate::fuzzy::word(&q, &o.label).map(|(s, m)| (-s, i, m))).collect();
        v.sort_by_key(|x| (x.0, x.1));
        v.into_iter().map(|(_, i, m)| (i, m)).collect()
    }

    /// ←→: the option D steps from the current pick (round). True: the choice changed.
    pub fn step(&mut self, d: isize, opts: &[Opt]) -> bool {
        if opts.is_empty() {
            return false;
        }
        let at = opts.iter().position(|o| o.value == self.choice).unwrap_or(0) as isize;
        let to = opts[(at + d).rem_euclid(opts.len() as isize) as usize].value.clone();
        self.pick(to)
    }

    /// Key for the open dropdown. Returns (handled, choice changed).
    pub fn key(&mut self, k: KeyEvent, opts: &[Opt]) -> (bool, bool) {
        if self.open.is_none() {
            return (false, false);
        }
        let shown = self.shown(opts);
        let n = shown.len();
        let typed = !k.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER);
        let Some(o) = self.open.as_mut() else { return (false, false) };
        match k.code {
            KeyCode::Esc => self.open = None,
            KeyCode::Up if n > 0 => o.cursor = (o.cursor + n - 1) % n,
            KeyCode::Down if n > 0 => o.cursor = (o.cursor + 1) % n,
            KeyCode::PageUp => o.cursor = o.cursor.saturating_sub(dropdown::MAX_ROWS),
            KeyCode::PageDown if n > 0 => o.cursor = (o.cursor + dropdown::MAX_ROWS).min(n - 1),
            KeyCode::Enter => {
                let value = shown.get(o.cursor).map(|&(i, _)| opts[i].value.clone());
                return (true, value.is_some_and(|v| self.pick(v)));
            }
            KeyCode::Backspace if typed => {
                o.query.pop();
                o.cursor = 0;
            }
            KeyCode::Char(c) if typed => {
                o.query.push(c);
                o.cursor = 0;
            }
            _ => {}
        }
        (true, false)
    }

    /// Click at AT. Returns (handled, choice changed): on the chip it toggles the dropdown, on a
    /// row it picks, elsewhere it closes an open dropdown (handled: the click stops there).
    pub fn click(&mut self, at: Position, opts: &[Opt]) -> (bool, bool) {
        if self.chip.contains(at) {
            if self.is_open() { self.close() } else { self.open(opts) }
            return (true, false);
        }
        if !self.is_open() {
            return (false, false);
        }
        if let Some(&(_, i)) = self.rows.iter().find(|(r, _)| r.contains(at)) {
            return (true, opts.get(i).is_some_and(|o| self.pick(o.value.clone())));
        }
        let inside = self.area.contains(at);
        if !inside {
            self.close();
        }
        (true, false)
    }

    #[cfg(test)]
    pub fn chip_rect(&self) -> Rect {
        self.chip
    }

    /// Drawn row of option I (tests: clicks).
    #[cfg(test)]
    pub fn row_rect(&self, i: usize) -> Option<Rect> {
        self.rows.iter().find(|(_, j)| *j == i).map(|(r, _)| *r)
    }

    /// Label of the current pick.
    fn label(&self) -> String {
        match &self.choice {
            None => "all".into(),
            Some(p) if p.as_os_str().is_empty() => "outside git repos".into(),
            Some(p) => p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| p.display().to_string()),
        }
    }

    /// The chip, right-aligned in ROW (`repo › codehead ▾  ←→`). Returns its width.
    pub fn draw_chip(&mut self, f: &mut Frame, row: Rect) -> u16 {
        let on = self.choice.is_some();
        let label = self.label();
        let spans = vec![
            Span::styled("repo › ", if self.is_open() { theme::accent() } else { theme::muted() }),
            Span::styled(format!("{label} ▾"), if on { theme::accent() } else { theme::text() }),
            Span::styled("  ←→ ", theme::panel().fg(theme::pal().line)),
        ];
        let line = Line::from(spans);
        let w = (line.width() as u16).min(row.width);
        let r = Rect { x: row.right().saturating_sub(w), width: w, ..row };
        f.render_widget(Paragraph::new(line), r);
        self.chip = r;
        w
    }

    /// The open dropdown below the chip, inside BOUNDS.
    pub fn draw_menu(&mut self, f: &mut Frame, opts: &[Opt], bounds: Rect) {
        let Some((q, cur, off)) = self.open.as_ref().map(|o| (o.query.clone(), o.cursor, o.offset)) else {
            self.rows.clear();
            self.area = Rect::default();
            return;
        };
        let shown = self.shown(opts);
        let n = shown.len();
        let items: Vec<dropdown::Row> =
            shown.iter().map(|(i, m)| dropdown::Row { text: opts[*i].label.clone(), detail: opts[*i].detail.clone(), now: opts[*i].value == self.choice, marks: m.clone() }).collect();
        let w = items.iter().map(|r| r.text.width() + r.detail.width() + 8).max().unwrap_or(0).clamp(36, 80) as u16;
        let anchor = Rect { x: self.chip.right().saturating_sub(w), width: 1, ..self.chip };
        let Some(r) = dropdown::place(anchor, w, n, bounds, false) else { return };
        let cur = cur.min(n.saturating_sub(1));
        let label = if q.is_empty() { "repo".to_string() } else { format!("repo › {q}") };
        let note = if q.is_empty() { "type: filter".to_string() } else { format!("{n} of {}", opts.len()) };
        let (off, drawn) = dropdown::list(f, r, &label, &note, &items, cur, off, "0 matches");
        if let Some(o) = self.open.as_mut() {
            o.cursor = cur;
            o.offset = off;
        }
        self.area = r;
        self.rows = drawn.into_iter().map(|(row, j)| (row, shown[j].0)).collect();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(c: KeyCode) -> KeyEvent {
        KeyEvent::new(c, KeyModifiers::NONE)
    }

    #[test]
    fn options_count_rows_per_repo() {
        let a = PathBuf::from("/x/alpha");
        let b = PathBuf::from("/x/beta");
        let o = options(&[Some(&a), Some(&b), Some(&b), None]);
        let labels: Vec<&str> = o.iter().map(|o| o.label.as_str()).collect();
        assert_eq!(labels, ["all repos", "beta", "alpha", "outside git repos"]);
        assert_eq!(o[1].detail, "2 · /x/beta");
        assert_eq!(o[0].detail, "4");
    }

    #[test]
    fn pick_filters_rows() {
        let a = PathBuf::from("/x/alpha");
        let b = PathBuf::from("/x/beta");
        let opts = options(&[Some(&a), Some(&b), None]);
        let mut f = RepoFilter::new();
        assert!(f.keeps(Some(&a)) && f.keeps(None));
        f.open(&opts);
        // type "bet": beta only; enter picks it
        for c in "bet".chars() {
            f.key(key(KeyCode::Char(c)), &opts);
        }
        assert_eq!(f.key(key(KeyCode::Enter), &opts), (true, true));
        assert!(!f.is_open());
        assert!(f.keeps(Some(&b)) && !f.keeps(Some(&a)) && !f.keeps(None));
        // outside git repos: rows with 0 repo only
        f.open(&opts);
        f.key(key(KeyCode::Down), &opts);
        f.key(key(KeyCode::Enter), &opts);
        assert!(f.keeps(None) && !f.keeps(Some(&b)));
        // esc closes, 0 change
        f.open(&opts);
        assert_eq!(f.key(key(KeyCode::Esc), &opts), (true, false));
        assert!(!f.is_open());
        assert_eq!(f.key(key(KeyCode::Down), &opts), (false, false));
    }

    #[test]
    fn repo_root_follows_worktree_git_file() {
        let base = std::env::temp_dir().join(format!("codehead-repofilter-{}", std::process::id()));
        let main = base.join("main");
        let wt = base.join("wt");
        std::fs::create_dir_all(main.join(".git/worktrees/wt")).unwrap();
        std::fs::create_dir_all(wt.join("src")).unwrap();
        std::fs::write(wt.join(".git"), format!("gitdir: {}\n", main.join(".git/worktrees/wt").display())).unwrap();
        assert_eq!(crate::threads::repo_root(&main.join("deep/er")), Some(main.clone()));
        assert_eq!(crate::threads::repo_root(&wt.join("src")), Some(main.clone()));
        let _ = std::fs::remove_dir_all(&base);
    }
}
