//! The projects tab of the start screen: the projects of a repo (their briefs, `projects.rs`).
//! A repo row (←→ on it), the list, the brief of the project under the cursor (or its form).
//! Keys on the list: n new · enter / e edit · x done / active · d d delete · o open in the IDE.
//! The form (n, enter): a title field, a brief textarea, merged into the brief's Markdown file
//! (title in the front matter); tab switch field, ctrl+s save, esc close (edits: a 2nd esc drops them).

use std::path::PathBuf;
use std::time::{Duration, Instant};

use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Constraint, Layout, Position, Rect};
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};

use crate::projects::{self, Project};
use crate::tabgroup::TabGroup;
use crate::textarea::TextArea;
use crate::theme;

/// What the host does after a key / click.
#[derive(Clone, Debug, PartialEq)]
pub enum Out {
    None,
    Toast(String),
    /// ↑ above the repo row: the tab bar takes the keys.
    Up,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Focus {
    Repos,
    List,
}

pub struct ProjectsTab {
    /// Repos to show (the start screen's first).
    pub repos: Vec<PathBuf>,
    repo_i: usize,
    tabs: TabGroup,
    focus: Focus,
    pub items: Vec<Project>,
    /// Active threads: (repo, project slugs) each (the host's cards).
    pub threads: Vec<(PathBuf, Vec<String>)>,
    cursor: usize,
    /// The form: a new project (n) or a brief to edit (enter).
    form: Option<Form>,
    /// The last esc in the form with edits: a 2nd within 2 s drops them.
    esc_at: Option<Instant>,
    /// d once: a 2nd d deletes.
    del: bool,
    list: Rect,
}

/// The title field and the brief textarea of a project.
struct Form {
    title: TextArea,
    body: TextArea,
    on_title: bool,
    /// The project edited; None: a new project.
    slug: Option<String>,
}

impl Form {
    fn new(slug: Option<String>, title: &str, body: &str) -> Form {
        let mut t = TextArea::new(true, "project title, e.g. Website refinement");
        t.style = Some(theme::text());
        t.set_text(title);
        let mut b = TextArea::new(false, "the brief: goal, decisions, open points");
        b.style = Some(theme::text());
        b.load_text(body);
        // the cursor: line 1's end (a new brief: after `Goal: `)
        b.move_start();
        b.key(KeyEvent::from(KeyCode::End));
        Form { title: t, body: b, on_title: slug.is_none(), slug }
    }

    fn edited(&self) -> bool {
        self.title.edited() || self.body.edited()
    }
}

impl Default for ProjectsTab {
    fn default() -> Self {
        ProjectsTab { repos: Vec::new(), repo_i: 0, tabs: TabGroup::default(), focus: Focus::List, items: Vec::new(), threads: Vec::new(), cursor: 0, form: None, esc_at: None, del: false, list: Rect::default() }
    }
}

impl ProjectsTab {
    /// The tab opens: REPOS (first: the start screen's repo), read the briefs.
    pub fn enter(&mut self, repos: Vec<PathBuf>) {
        let cur = self.repo().cloned();
        self.repos = repos;
        self.repo_i = cur.and_then(|c| self.repos.iter().position(|r| *r == c)).unwrap_or(0);
        self.reload();
    }

    pub fn repo(&self) -> Option<&PathBuf> {
        self.repos.get(self.repo_i)
    }

    pub fn reload(&mut self) {
        self.items = self.repo().map(|r| projects::list(r)).unwrap_or_default();
        self.cursor = self.cursor.min(self.items.len().saturating_sub(1));
    }

    /// An input takes the keys (esc stays here).
    pub fn typing(&self) -> bool {
        self.form.is_some() || self.del
    }

    fn count(&self, slug: &str) -> usize {
        let repo = self.repo();
        self.threads.iter().filter(|(r, ps)| Some(r) == repo && ps.iter().any(|p| p == slug)).count()
    }

    pub fn key(&mut self, k: KeyEvent) -> Out {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL) || k.modifiers.contains(KeyModifiers::SUPER);
        if let Some(fm) = self.form.as_mut() {
            match k.code {
                KeyCode::Char('s') if ctrl => return self.save(),
                KeyCode::Esc => {
                    let again = self.esc_at.is_some_and(|t| t.elapsed() < Duration::from_secs(2));
                    if !fm.edited() || again {
                        self.form = None;
                        self.esc_at = None;
                    } else {
                        self.esc_at = Some(Instant::now());
                        return Out::Toast("unsaved edits: esc again drops them · ctrl+s saves".into());
                    }
                }
                KeyCode::Tab | KeyCode::BackTab => fm.on_title = !fm.on_title,
                KeyCode::Enter | KeyCode::Down if fm.on_title => fm.on_title = false,
                KeyCode::Up if !fm.on_title && fm.body.on_first_row() => fm.on_title = true,
                _ if fm.on_title => {
                    fm.title.key(k);
                }
                _ => {
                    fm.body.key(k);
                }
            }
            return Out::None;
        }
        if self.del {
            self.del = false;
            if k.code == KeyCode::Char('d') {
                return self.delete();
            }
            return Out::None;
        }
        if self.focus == Focus::Repos {
            match k.code {
                KeyCode::Up => return Out::Up,
                KeyCode::Down | KeyCode::Enter => self.focus = Focus::List,
                KeyCode::Left | KeyCode::Right => self.step_repo(if k.code == KeyCode::Right { 1 } else { -1 }),
                _ => {}
            }
            return Out::None;
        }
        let n = self.items.len();
        match k.code {
            KeyCode::Up | KeyCode::Char('k') if self.cursor == 0 => self.focus = Focus::Repos,
            KeyCode::Up | KeyCode::Char('k') => self.cursor -= 1,
            KeyCode::Down | KeyCode::Char('j') => self.cursor = (self.cursor + 1).min(n.saturating_sub(1)),
            KeyCode::Left | KeyCode::Right => self.step_repo(if k.code == KeyCode::Right { 1 } else { -1 }),
            KeyCode::Char('n') if self.repo().is_some() => {
                self.form = Some(Form::new(None, "", projects::TEMPLATE));
                self.esc_at = None;
            }
            KeyCode::Enter | KeyCode::Char('e') if n > 0 => self.open_editor(),
            KeyCode::Char('x') if n > 0 => {
                let p = self.items[self.cursor].clone();
                if let Err(e) = projects::set_done(&p, !p.done) {
                    return Out::Toast(e);
                }
                self.reload();
                self.cursor = self.items.iter().position(|x| x.slug == p.slug).unwrap_or(0);
                return Out::Toast(format!("{}: {}", p.slug, if p.done { "active" } else { "done" }));
            }
            KeyCode::Char('d') if n > 0 => {
                self.del = true;
            }
            KeyCode::Char('o') if n > 0 => return self.open_ide(),
            _ => {}
        }
        Out::None
    }

    fn step_repo(&mut self, d: isize) {
        if self.repos.is_empty() {
            return;
        }
        self.repo_i = (self.repo_i as isize + d).rem_euclid(self.repos.len() as isize) as usize;
        self.cursor = 0;
        self.reload();
    }

    fn open_editor(&mut self) {
        let Some(p) = self.items.get(self.cursor) else { return };
        let text = std::fs::read_to_string(&p.path).unwrap_or_default();
        self.form = Some(Form::new(Some(p.slug.clone()), &p.title, &projects::body_of(&text)));
        self.esc_at = None;
    }

    /// ctrl+s: title + brief → the Markdown file (a new project: made first).
    fn save(&mut self) -> Out {
        let Some(fm) = self.form.as_ref() else { return Out::None };
        let title = fm.title.text().trim().to_string();
        if title.is_empty() {
            return Out::Toast("title: 1 letter or digit at least".into());
        }
        let body = fm.body.text();
        let (path, made) = match &fm.slug {
            Some(slug) => match self.items.iter().find(|x| &x.slug == slug) {
                Some(p) => (p.path.clone(), false),
                None => return Out::Toast(format!("{slug}: gone")),
            },
            None => {
                let Some(repo) = self.repo().cloned() else { return Out::None };
                match projects::create_in(&projects::dir(&repo), &title) {
                    Ok(p) => (p.path, true),
                    Err(e) => return Out::Toast(e),
                }
            }
        };
        let old = std::fs::read_to_string(&path).unwrap_or_default();
        if let Err(e) = crate::engine::write_atomic(&path, projects::merge(&old, &title, &body).as_bytes()) {
            return Out::Toast(format!("{}: {e}", path.display()));
        }
        let slug = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        self.form = None;
        self.esc_at = None;
        self.reload();
        self.cursor = self.items.iter().position(|x| x.slug == slug).unwrap_or(0);
        self.focus = Focus::List;
        Out::Toast(if made { format!("project {slug} made") } else { format!("{slug}: saved") })
    }

    fn delete(&mut self) -> Out {
        let Some(p) = self.items.get(self.cursor).cloned() else { return Out::None };
        if let Err(e) = std::fs::remove_file(&p.path) {
            return Out::Toast(format!("{}: {e}", p.path.display()));
        }
        self.reload();
        let n = self.count(&p.slug);
        Out::Toast(if n > 0 { format!("{} deleted · {n} thread(s) keep its name until edited", p.slug) } else { format!("{} deleted", p.slug) })
    }

    fn open_ide(&mut self) -> Out {
        let Some(p) = self.items.get(self.cursor) else { return Out::None };
        let Some(e) = crate::editor::find() else { return Out::Toast("0 editor found (setting `editor`): enter edits here".into()) };
        if e.kind.in_terminal() {
            return Out::Toast("terminal editor: enter edits here".into());
        }
        let dir = p.path.parent().map(PathBuf::from).unwrap_or_default();
        let file = p.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        match crate::editor::spawn(&e, &crate::editor::args_at(e.kind, &dir, &file, 1)) {
            Ok(()) => Out::Toast(format!("{} open in the editor · r: read again", p.slug)),
            Err(err) => Out::Toast(err),
        }
    }

    pub fn paste(&mut self, text: &str) {
        if let Some(fm) = self.form.as_mut() {
            if fm.on_title {
                fm.title.insert(&text.replace('\n', " "));
            } else {
                fm.body.paste(text);
            }
        }
    }

    pub fn mouse(&mut self, m: MouseEvent) -> Out {
        let at = Position { x: m.column, y: m.row };
        if self.typing() {
            if let Some(fm) = self.form.as_mut() {
                if matches!(m.kind, MouseEventKind::Down(MouseButton::Left)) {
                    if fm.title.area().contains(at) {
                        fm.on_title = true;
                    } else if fm.body.area().contains(at) {
                        fm.on_title = false;
                    }
                }
                if fm.on_title {
                    fm.title.mouse(m);
                } else {
                    fm.body.mouse(m);
                }
            }
            return Out::None;
        }
        match m.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                if self.tabs.click(at) {
                    self.focus = Focus::Repos;
                    self.repo_i = self.tabs.i;
                    self.cursor = 0;
                    self.reload();
                } else if self.list.contains(at) {
                    let i = (at.y - self.list.y) as usize;
                    if i < self.items.len() {
                        self.focus = Focus::List;
                        if self.cursor == i {
                            self.open_editor();
                        }
                        self.cursor = i;
                    }
                }
            }
            MouseEventKind::ScrollDown => self.cursor = (self.cursor + 1).min(self.items.len().saturating_sub(1)),
            MouseEventKind::ScrollUp => self.cursor = self.cursor.saturating_sub(1),
            _ => {}
        }
        Out::None
    }

    pub fn help(&self) -> &'static [(&'static str, &'static str)] {
        &[
            ("←→", "repo"),
            ("↑↓", "project"),
            ("n", "new project (its brief: <home repo>/<repo>/projects/<slug>.md)"),
            ("n / enter / e", "form: title, brief · tab field · ctrl+s save · esc close"),
            ("x", "done / active (done: out of the pickers)"),
            ("d d", "delete the brief"),
            ("o", "open the brief in the IDE"),
        ]
    }

    pub fn hints(&self) -> &'static [(&'static str, &'static str)] {
        if let Some(fm) = &self.form {
            if fm.slug.is_none() { &[("tab", "field"), ("ctrl+s", "make"), ("esc", "cancel")] } else { &[("tab", "field"), ("ctrl+s", "save"), ("esc", "close")] }
        } else if self.del {
            &[("d", "delete"), ("any key", "keep")]
        } else {
            &[("←→", "repo"), ("n", "new"), ("enter", "edit"), ("x", "done"), ("d d", "delete"), ("o", "IDE")]
        }
    }

    pub fn draw(&mut self, f: &mut Frame, area: Rect, active: bool) {
        if area.height < 4 {
            return;
        }
        let [top, path_row, _, body] = Layout::vertical([Constraint::Length(1), Constraint::Length(1), Constraint::Length(1), Constraint::Min(1)]).areas(area);
        let labels: Vec<String> = self.repos.iter().map(|r| r.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()).collect();
        if labels.is_empty() {
            f.render_widget(Paragraph::new(Line::styled("0 repos: start a thread in a git repo first", theme::muted())), top);
            return;
        }
        self.tabs.i = self.repo_i;
        self.tabs.draw(f, top, "repo", &labels, active && self.focus == Focus::Repos);
        if let Some(r) = self.repo() {
            let dir = projects::dir(r);
            f.render_widget(Paragraph::new(Line::styled(format!("      {}", crate::app::tilde(&dir.display().to_string())), theme::muted())), path_row);
        }
        let [left, _, right] = Layout::horizontal([Constraint::Percentage(40), Constraint::Length(1), Constraint::Fill(1)]).areas(body);
        self.list = Rect { height: left.height.saturating_sub(2), ..left };
        let mut lines: Vec<Line> = Vec::new();
        if self.items.is_empty() {
            lines.push(Line::styled("0 projects · n: new project", theme::muted()));
            lines.push(Line::styled("1 project per feature or goal, e.g. `website refinement`", theme::muted()));
        }
        for (i, p) in self.items.iter().enumerate() {
            let cur = i == self.cursor && self.focus == Focus::List;
            let st = if cur && active { theme::pill().add_modifier(Modifier::BOLD) } else if p.done { theme::muted() } else { theme::text() };
            let n = self.count(&p.slug);
            let mut s = vec![Span::styled(format!(" {} ", p.title), st)];
            s.push(Span::styled(format!("  {n} thread{}", if n == 1 { "" } else { "s" }), theme::muted()));
            if p.done {
                s.push(Span::styled("  done", theme::fg(theme::pal().success())));
            }
            lines.push(Line::from(s));
        }
        f.render_widget(Paragraph::new(lines), self.list);
        if let Some(fm) = self.form.as_mut() {
            // like the new thread view: a label above each field, the input on the panel color
            let [l_title, i_title, _, l_body, i_body] = Layout::vertical([Constraint::Length(1), Constraint::Length(1), Constraint::Length(1), Constraint::Length(1), Constraint::Min(1)]).areas(right);
            let label = |text: &str, on: bool| Line::styled(text.to_string(), if on { theme::accent_bold() } else { theme::muted() });
            let head = if fm.slug.is_none() { "new project · title" } else { "title" };
            f.render_widget(Paragraph::new(label(head, fm.on_title)), l_title);
            f.render_widget(Paragraph::new("").style(theme::panel()), i_title);
            fm.title.render(f, Rect { x: i_title.x + 1, width: i_title.width.saturating_sub(2), ..i_title }, fm.on_title);
            f.render_widget(Paragraph::new(label("brief", !fm.on_title)), l_body);
            f.render_widget(Paragraph::new("").style(theme::panel()), i_body);
            fm.body.render(f, Rect { x: i_body.x + 1, width: i_body.width.saturating_sub(2), ..i_body }, !fm.on_title);
        } else if let Some(p) = self.items.get(self.cursor) {
            let text = std::fs::read_to_string(&p.path).unwrap_or_default();
            // the front matter: muted; headings: accent
            let mut front = text.starts_with("---");
            let mut lines: Vec<Line> = Vec::new();
            for (i, l) in text.lines().enumerate() {
                let st = if front { theme::muted() } else if l.starts_with('#') { theme::accent_bold() } else { theme::text() };
                lines.push(Line::styled(l.to_string(), st));
                if front && i > 0 && l.trim() == "---" {
                    front = false;
                }
            }
            f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), right);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn k(c: KeyCode) -> KeyEvent {
        KeyEvent::from(c)
    }

    #[test]
    fn new_edit_done_delete() {
        let _g = crate::engine::ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let repo = std::env::temp_dir().join(format!("ch-projtab-{}", std::process::id())).join("demo");
        let _ = std::fs::remove_dir_all(projects::dir(&repo));
        let mut t = ProjectsTab::default();
        t.enter(vec![repo.clone()]);
        assert!(t.items.is_empty());
        t.key(k(KeyCode::Char('n')));
        assert!(t.form.as_ref().is_some_and(|f| f.on_title), "n: the form, on the title");
        for c in "Bug fixes".chars() {
            t.key(k(KeyCode::Char(c)));
        }
        t.key(k(KeyCode::Enter));
        assert!(t.form.as_ref().is_some_and(|f| !f.on_title), "enter: the brief");
        for c in "ship it".chars() {
            t.key(k(KeyCode::Char(c)));
        }
        assert!(matches!(t.key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL)), Out::Toast(_)));
        assert!(t.form.is_none());
        assert_eq!(t.items.len(), 1);
        let text = std::fs::read_to_string(&t.items[0].path).unwrap();
        assert!(text.starts_with("---\ntitle: Bug fixes\n---\n\nGoal: ship it\n"), "{text}");
        t.key(k(KeyCode::Enter));
        assert_eq!(t.form.as_ref().map(|f| f.title.text()), Some("Bug fixes".into()));
        t.key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL));
        assert!(t.form.is_none());
        t.key(k(KeyCode::Char('x')));
        assert!(t.items[0].done);
        t.key(k(KeyCode::Char('d')));
        t.key(k(KeyCode::Char('d')));
        assert!(t.items.is_empty());
        assert_eq!(t.key(k(KeyCode::Up)), Out::None);
        assert_eq!(t.key(k(KeyCode::Up)), Out::Up);
        let _ = std::fs::remove_dir_all(projects::dir(&repo));
    }
}
