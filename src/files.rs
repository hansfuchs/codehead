//! The files view of a thread: the files its agents wrote or edited (their tool calls), and the
//! home repo's notes of the thread (`thread: <id>` on line 1), newest first; a preview right
//! (Markdown rendered, code in syntax colors). yazi (full screen) browses anything else.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::engine::store::{self, Kind};
use crate::theme;

/// Where a file comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// An agent's Write / Edit call.
    Agent,
    /// A home repo note of this thread.
    Note,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub path: PathBuf,
    pub source: Source,
    /// Last change, unix s.
    pub at: u64,
}

/// Tool inputs that name the file they write.
const WRITE_TOOLS: [&str; 5] = ["Write", "Edit", "MultiEdit", "NotebookEdit", "str_replace_based_edit_tool"];

/// Files the agents of thread ID wrote or edited (absolute; relative ones from DIR), each once.
pub fn agent_files(id: &str, dir: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    for e in store::events(id) {
        if let Kind::Tool { name, input, .. } = e.kind
            && (WRITE_TOOLS.contains(&name.as_str()) || input["_kind"] == "edit")
            && let Some(p) = input["file_path"].as_str().or_else(|| input["path"].as_str()).or_else(|| input["notebook_path"].as_str())
        {
            let p = if Path::new(p).is_absolute() { PathBuf::from(p) } else { dir.join(p) };
            if !out.contains(&p) {
                out.push(p);
            }
        }
    }
    out
}

/// Markdown files under ROOT whose line 1 names thread ID (`thread: <id>`), at most 4 dirs deep.
pub fn notes_of(root: &Path, id: &str) -> Vec<PathBuf> {
    fn walk(d: &Path, depth: usize, id: &str, out: &mut Vec<PathBuf>) {
        let Ok(rd) = std::fs::read_dir(d) else { return };
        for e in rd.flatten() {
            let p = e.path();
            let name = e.file_name();
            if name.to_string_lossy().starts_with('.') {
                continue;
            }
            if p.is_dir() && depth < 4 {
                walk(&p, depth + 1, id, out);
            } else if p.extension().is_some_and(|x| x == "md") {
                use std::io::{BufRead, BufReader};
                let first = std::fs::File::open(&p).ok().and_then(|f| BufReader::new(f).lines().next()).and_then(Result::ok).unwrap_or_default();
                if first.starts_with("thread:") && first.contains(id) {
                    out.push(p);
                }
            }
        }
    }
    let mut out = Vec::new();
    walk(root, 0, id, &mut out);
    out
}

fn mtime(p: &Path) -> u64 {
    std::fs::metadata(p).and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs())
}

/// The files of thread ID (DIR: its work dir): existing ones, newest first.
pub fn load(id: &str, dir: &Path) -> Vec<Entry> {
    let mut v: Vec<Entry> = agent_files(id, dir).into_iter().filter(|p| p.is_file()).map(|p| Entry { at: mtime(&p), path: p, source: Source::Agent }).collect();
    if let Some(home) = crate::config::home_repo() {
        for p in notes_of(&home, id) {
            if !v.iter().any(|e| e.path == p) {
                v.push(Entry { at: mtime(&p), path: p, source: Source::Note });
            }
        }
    }
    v.sort_by_key(|e| std::cmp::Reverse(e.at));
    v
}

/// What a key asks the thread view to do.
#[derive(Clone, Debug, PartialEq)]
pub enum Act {
    None,
    Toast(String),
    /// The IDE at line 1 of this file.
    Open(PathBuf),
    /// yazi with this file revealed.
    Yazi(PathBuf),
}

pub struct View {
    id: String,
    dir: PathBuf,
    pub files: Vec<Entry>,
    at: usize,
    /// Preview: rows per (path, width, mtime), scroll.
    cache: HashMap<(PathBuf, u16, u64), Vec<Line<'static>>>,
    scroll: usize,
    loaded: Option<Instant>,
    list: Rect,
    preview: Rect,
}

impl View {
    pub fn new(id: &str, dir: &Path) -> View {
        View { id: id.to_string(), dir: dir.to_path_buf(), files: Vec::new(), at: 0, cache: HashMap::new(), scroll: 0, loaded: None, list: Rect::default(), preview: Rect::default() }
    }

    /// Read the list again (NOW: even if fresh; else every 3 s while open).
    pub fn refresh(&mut self, now: bool) {
        if !now && self.loaded.is_some_and(|t| t.elapsed() < Duration::from_secs(3)) {
            return;
        }
        let cur = self.files.get(self.at).map(|e| e.path.clone());
        self.files = load(&self.id, &self.dir);
        self.loaded = Some(Instant::now());
        self.at = cur.and_then(|p| self.files.iter().position(|e| e.path == p)).unwrap_or(0);
    }

    pub fn count(&self) -> usize {
        self.files.len()
    }

    /// PATH as shown: relative to the work dir, else `~/…`.
    fn shown(&self, p: &Path) -> String {
        if let Ok(r) = p.strip_prefix(&self.dir) {
            return r.display().to_string();
        }
        let home = crate::config::home();
        match p.strip_prefix(&home) {
            Ok(r) => format!("~/{}", r.display()),
            Err(_) => p.display().to_string(),
        }
    }

    fn preview_lines(&mut self, p: &Path, width: u16) -> Vec<Line<'static>> {
        let key = (p.to_path_buf(), width, mtime(p));
        if let Some(l) = self.cache.get(&key) {
            return l.clone();
        }
        let text = std::fs::read(p).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_else(|e| format!("{e}"));
        let w = usize::from(width);
        let lines: Vec<Line<'static>> = if p.extension().is_some_and(|x| x == "md") {
            crate::render::markdown(&text, w, &[]).into_iter().map(|r| r.line).collect()
        } else {
            let lang = p.extension().map(|x| x.to_string_lossy().into_owned()).unwrap_or_else(|| "text".into());
            crate::render::code_rows(&text, &lang, w, &[], true, None).into_iter().skip(1).map(|r| r.line).collect()
        };
        if self.cache.len() > 64 {
            self.cache.clear();
        }
        self.cache.insert(key, lines.clone());
        lines
    }

    pub fn draw(&mut self, f: &mut Frame, area: Rect) {
        self.refresh(false);
        if self.files.is_empty() {
            f.render_widget(Paragraph::new(Line::styled("  0 files yet: the files the agents write, and the home repo's notes of this thread · ctrl+y: yazi", theme::muted())), area);
            return;
        }
        let lw = (area.width * 2 / 5).clamp(24, 60).min(area.width);
        let list = Rect { width: lw, ..area };
        let preview = Rect { x: area.x + lw + 1, width: area.width.saturating_sub(lw + 1), ..area };
        self.list = list;
        self.preview = preview;
        self.at = self.at.min(self.files.len() - 1);
        let h = usize::from(list.height);
        let top = self.at.saturating_sub(h.saturating_sub(1));
        let now = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map_or(0, |d| d.as_secs());
        for (i, e) in self.files.iter().enumerate().skip(top).take(h) {
            let tag = match e.source {
                Source::Agent => Span::styled("✎ ", Style::default().fg(theme::pal().accent)),
                Source::Note => Span::styled("◆ ", Style::default().fg(theme::pal().ok)),
            };
            let age = crate::notes::ago(SystemTime::UNIX_EPOCH + Duration::from_secs(e.at.min(now)));
            let name = self.shown(&e.path);
            let room = usize::from(lw).saturating_sub(age.chars().count() + 5);
            let st = if i == self.at { theme::pill() } else { Style::default() };
            let line = Line::from(vec![Span::raw(" "), tag, Span::styled(crate::nav::fit(&name, room), if i == self.at { st } else { theme::text() }), Span::styled(format!(" {age}"), theme::muted())]);
            f.render_widget(Paragraph::new(line).style(st), Rect { y: list.y + (i - top) as u16, height: 1, ..list });
        }
        // the divider
        for y in area.y..area.bottom() {
            f.buffer_mut()[(area.x + lw, y)].set_symbol("│").set_style(theme::line());
        }
        let p = self.files[self.at].path.clone();
        let lines = self.preview_lines(&p, preview.width.saturating_sub(1));
        self.scroll = self.scroll.min(lines.len().saturating_sub(1));
        let head = Line::from(vec![Span::styled(format!(" {} ", self.shown(&p)), theme::accent_bold()), Span::styled(format!("{} lines · o: IDE · y: yazi · c: copy path", lines.len()), theme::muted())]);
        f.render_widget(Paragraph::new(head), Rect { height: 1, ..preview });
        let body: Vec<Line> = lines.into_iter().skip(self.scroll).take(usize::from(preview.height.saturating_sub(2))).collect();
        f.render_widget(Paragraph::new(body), Rect { x: preview.x + 1, y: preview.y + 2, height: preview.height.saturating_sub(2), width: preview.width.saturating_sub(1) });
    }

    pub fn key(&mut self, k: KeyEvent) -> Act {
        let n = self.files.len();
        let page = usize::from(self.preview.height.saturating_sub(3)).max(1);
        match k.code {
            KeyCode::Down | KeyCode::Char('j') if n > 0 => {
                self.at = (self.at + 1).min(n - 1);
                self.scroll = 0;
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.at = self.at.saturating_sub(1);
                self.scroll = 0;
            }
            KeyCode::PageDown | KeyCode::Char(' ') => self.scroll += page,
            KeyCode::PageUp => self.scroll = self.scroll.saturating_sub(page),
            KeyCode::Char('g') | KeyCode::Home => self.scroll = 0,
            KeyCode::Char('o') => return self.files.get(self.at).map_or(Act::None, |e| Act::Open(e.path.clone())),
            KeyCode::Char('y') => return self.files.get(self.at).map_or(Act::None, |e| Act::Yazi(e.path.clone())),
            KeyCode::Char('c') => {
                if let Some(e) = self.files.get(self.at) {
                    let p = e.path.display().to_string();
                    let ok = crate::clip::copy(&p);
                    return Act::Toast(if ok { format!("copied {p}") } else { "0 clipboard tool".into() });
                }
            }
            KeyCode::Char('r') => self.refresh(true),
            _ => {}
        }
        Act::None
    }

    pub fn mouse(&mut self, m: MouseEvent) {
        let at = ratatui::layout::Position { x: m.column, y: m.row };
        match m.kind {
            MouseEventKind::ScrollDown if self.preview.contains(at) => self.scroll += 3,
            MouseEventKind::ScrollUp if self.preview.contains(at) => self.scroll = self.scroll.saturating_sub(3),
            MouseEventKind::Down(_) if self.list.contains(at) => {
                let h = usize::from(self.list.height);
                let top = self.at.saturating_sub(h.saturating_sub(1));
                let i = top + usize::from(m.row - self.list.y);
                if i < self.files.len() {
                    self.at = i;
                    self.scroll = 0;
                }
            }
            _ => {}
        }
    }
}

/// yazi's config dir owned by codehead: a theme from the palette (0 change to ~/.config/yazi).
pub fn yazi_config() -> PathBuf {
    let dir = crate::config::home().join(".cache/codehead/yazi");
    let _ = std::fs::create_dir_all(&dir);
    let hex = |c| theme::hex(c);
    let p = theme::pal();
    let theme_toml = format!(
        "[mgr]\ncwd = {{ fg = \"{acc}\" }}\nhovered = {{ fg = \"{on}\", bg = \"{acc}\" }}\npreview_hovered = {{ underline = true }}\nborder_style = {{ fg = \"{line}\" }}\n\n[status]\nsep_left = {{ open = \"\", close = \"\" }}\nsep_right = {{ open = \"\", close = \"\" }}\n\n[mode]\nnormal_main = {{ fg = \"{on}\", bg = \"{acc}\", bold = true }}\nnormal_alt = {{ fg = \"{acc}\", bg = \"{panel}\" }}\n",
        acc = hex(p.accent),
        on = hex(p.on_accent),
        line = hex(p.line),
        panel = hex(p.panel)
    );
    let _ = std::fs::write(dir.join("theme.toml"), theme_toml);
    // enter on a file: pick it (back to codehead), as yazi.nvim does
    let _ = std::fs::write(dir.join("keymap.toml"), "[[mgr.prepend_keymap]]\non = \"<Enter>\"\nrun = \"open\"\ndesc = \"pick: back to codehead\"\n");
    dir
}

/// The yazi command for TARGET (a dir, or a file to reveal): argv, env, the chooser file.
pub fn yazi_cmd(target: &Path) -> (Vec<String>, Vec<(String, String)>, PathBuf) {
    let chooser = std::env::temp_dir().join(format!("codehead-yazi-{}", std::process::id()));
    let _ = std::fs::remove_file(&chooser);
    let argv = vec!["yazi".to_string(), target.display().to_string(), format!("--chooser-file={}", chooser.display())];
    (argv, vec![("YAZI_CONFIG_HOME".into(), yazi_config().display().to_string())], chooser)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_files_come_from_write_and_edit_calls_once() {
        let _env = crate::engine::ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let t = store::Thread::new("f", "t", Path::new("/w"), vec!["ada".into()]);
        std::fs::create_dir_all(store::thread_dir(&t.id)).unwrap();
        t.save().unwrap();
        let tool = |name: &str, input: serde_json::Value| store::Event::new(Some("ada"), Kind::Tool { id: "1".into(), name: name.into(), input });
        for e in [
            tool("Write", serde_json::json!({"file_path": "/w/notes/plan.md"})),
            tool("Edit", serde_json::json!({"file_path": "src/a.rs"})),
            tool("Read", serde_json::json!({"file_path": "/w/README.md"})),
            tool("Edit", serde_json::json!({"file_path": "/w/notes/plan.md"})),
        ] {
            store::append(&t.id, &e).unwrap();
        }
        assert_eq!(agent_files(&t.id, Path::new("/w")), [PathBuf::from("/w/notes/plan.md"), PathBuf::from("/w/src/a.rs")]);
        let _ = std::fs::remove_dir_all(store::thread_dir(&t.id));
    }

    #[test]
    fn notes_of_a_thread_by_their_1st_line() {
        let d = std::env::temp_dir().join(format!("codehead-notes-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("area/plans")).unwrap();
        std::fs::write(d.join("area/plans/a.md"), "thread: abc123 · branch: main · status: open\n# A").unwrap();
        std::fs::write(d.join("area/plans/b.md"), "thread: other · branch: main\n").unwrap();
        std::fs::write(d.join("area/INDEX.md"), "- plans/a.md\n").unwrap();
        assert_eq!(notes_of(&d, "abc123"), [d.join("area/plans/a.md")]);
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn yazi_runs_with_codeheads_config_and_a_chooser_file() {
        let (argv, env, chooser) = yazi_cmd(Path::new("/w/src"));
        assert_eq!(argv[..2], ["yazi".to_string(), "/w/src".to_string()]);
        assert!(argv[2].starts_with("--chooser-file=") && argv[2].ends_with(&chooser.display().to_string()));
        assert_eq!(env[0].0, "YAZI_CONFIG_HOME");
        let dir = PathBuf::from(&env[0].1);
        assert!(std::fs::read_to_string(dir.join("theme.toml")).unwrap().contains("[mgr]"));
    }
}
