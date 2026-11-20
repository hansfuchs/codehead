//! The owner's pull requests (every repo, every state), grouped by repo (the repo of the newest
//! PR first), newest first in a repo: the start screen's merge tab and the menu's. Each PR with
//! its thread, if one has it: a checkout of the thread on the PR's branch, or the PR among the
//! thread's PRs the merge tab found (`<data>/prs/threads.json`).
//!
//! Read in the background: the last list from `<data>/prs/mine.json` at once, GitHub again when
//! it is older than 2 min (r: now, 3 s apart).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use serde_json::{Value, json};
use unicode_width::UnicodeWidthStr;

use crate::gh::{self, Item, PrRef, State};
use crate::theme;

const STALE: Duration = Duration::from_secs(120);
const MIN_MANUAL: Duration = Duration::from_secs(3);
/// Pages of 100 PRs read.
const PAGES: usize = 5;

/// A thread of a PR.
#[derive(Clone, Debug, PartialEq)]
pub struct Link {
    pub id: String,
    pub title: String,
    pub dir: PathBuf,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub item: Item,
    pub thread: Option<Link>,
}

/// What a key / click picked.
#[derive(Clone, Debug, PartialEq)]
pub enum Pick {
    None,
    Open(Entry),
    Toast(String),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Filter {
    #[default]
    All,
    Open,
    Merged,
    Closed,
}

impl Filter {
    fn keeps(self, s: State) -> bool {
        match self {
            Filter::All => true,
            Filter::Open => matches!(s, State::Open | State::Draft),
            Filter::Merged => s == State::Merged,
            Filter::Closed => s == State::Closed,
        }
    }

    fn next(self) -> Filter {
        match self {
            Filter::All => Filter::Open,
            Filter::Open => Filter::Merged,
            Filter::Merged => Filter::Closed,
            Filter::Closed => Filter::All,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Filter::All => "all",
            Filter::Open => "open",
            Filter::Merged => "merged",
            Filter::Closed => "closed",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
enum RowKind {
    Repo(String, usize),
    Pr(usize),
}

#[derive(Default)]
pub struct PrList {
    entries: Vec<Entry>,
    read: Option<Receiver<Result<Vec<Entry>, String>>>,
    at: Option<Instant>,
    cached: bool,
    error: Option<String>,
    filter: Filter,
    /// Index into the PR rows (headers skipped).
    cursor: usize,
    offset: usize,
    rows: Vec<(Rect, usize)>,
}

fn dir() -> PathBuf {
    crate::engine::data_dir().join("prs")
}

fn item_json(i: &Item) -> Value {
    json!({"repo": i.pr.repo, "number": i.pr.number, "title": i.title, "state": i.state.label(), "created": i.created, "head": i.head})
}

fn item_of(v: &Value) -> Option<Item> {
    let state = match v["state"].as_str()? {
        "draft" => State::Draft,
        "merged" => State::Merged,
        "closed" => State::Closed,
        _ => State::Open,
    };
    Some(Item { pr: PrRef::new(v["repo"].as_str()?, v["number"].as_u64()?), title: v["title"].as_str().unwrap_or_default().into(), state, created: v["created"].as_u64().unwrap_or(0), head: v["head"].as_str().unwrap_or_default().into() })
}

/// The thread THREAD has PRS (the merge tab found them): the index of the lists.
pub fn remember(thread: &str, prs: &[PrRef]) {
    if cfg!(test) {
        return;
    }
    let p = dir().join("threads.json");
    let _ = crate::engine::locked(&dir().join("threads.lock"), || {
        let mut v: Value = std::fs::read(&p).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_else(|| json!({}));
        if !v.is_object() {
            v = json!({});
        }
        v[thread] = json!(prs.iter().map(PrRef::key).collect::<Vec<_>>());
        let _ = std::fs::create_dir_all(dir());
        crate::engine::write_atomic(&p, v.to_string().as_bytes())
    });
}

/// PR → its thread: the remembered lists, then the threads' checkouts on the PR's branch.
fn link(items: &[Item]) -> Vec<Option<Link>> {
    let threads: Vec<crate::engine::store::Thread> = crate::engine::store::all().into_iter().filter(|t| t.state != "archived" && t.state != "deleted").collect();
    let remembered: Value = std::fs::read(dir().join("threads.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_else(|| json!({}));
    let mut by_pr: HashMap<String, &crate::engine::store::Thread> = HashMap::new();
    for t in &threads {
        for k in remembered[&t.id].as_array().into_iter().flatten().filter_map(Value::as_str) {
            by_pr.entry(k.to_string()).or_insert(t);
        }
    }
    // (repo, branch) of every checkout: 1 git call per dir
    let mut repo_of: HashMap<String, Option<String>> = HashMap::new();
    let mut by_branch: HashMap<(String, String), &crate::engine::store::Thread> = HashMap::new();
    for t in &threads {
        for c in t.all_checkouts() {
            let Some(b) = c.branch.clone() else { continue };
            let repo = repo_of.entry(c.dir.clone()).or_insert_with(|| gh::repo_of_dir(std::path::Path::new(&c.dir))).clone();
            if let Some(r) = repo {
                by_branch.entry((r, b)).or_insert(t);
            }
        }
    }
    items
        .iter()
        .map(|i| {
            let t = by_pr.get(&i.pr.key()).or_else(|| by_branch.get(&(i.pr.repo.clone(), i.head.clone())))?;
            Some(Link { id: t.id.clone(), title: t.title.clone(), dir: t.work_dir() })
        })
        .collect()
}

fn load() -> Result<Vec<Entry>, String> {
    let items = gh::mine(PAGES)?;
    let _ = std::fs::create_dir_all(dir());
    let _ = crate::engine::write_atomic(&dir().join("mine.json"), json!(items.iter().map(item_json).collect::<Vec<_>>()).to_string().as_bytes());
    let links = link(&items);
    Ok(items.into_iter().zip(links).map(|(item, thread)| Entry { item, thread }).collect())
}

fn cached() -> Vec<Entry> {
    let v: Value = std::fs::read(dir().join("mine.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
    v.as_array().into_iter().flatten().filter_map(item_of).map(|item| Entry { item, thread: None }).collect()
}

impl PrList {
    /// Tests: ENTRIES as read.
    #[cfg(test)]
    pub fn with(entries: Vec<Entry>) -> PrList {
        PrList { entries, at: Some(Instant::now()), cached: true, ..Default::default() }
    }

    /// The list shows: the cache at once, a read when it is stale.
    pub fn ensure(&mut self) {
        if !self.cached && !cfg!(test) {
            self.cached = true;
            self.entries = cached();
        }
        if self.read.is_none() && self.at.is_none_or(|t| t.elapsed() > STALE) {
            self.start();
        }
    }

    fn start(&mut self) {
        self.at = Some(Instant::now());
        if cfg!(test) {
            return;
        }
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(load());
        });
        self.read = Some(rx);
    }

    /// A finished read applies. True: a redraw is due.
    pub fn poll(&mut self) -> bool {
        let Some(rx) = &self.read else { return false };
        let Ok(r) = rx.try_recv() else { return false };
        self.read = None;
        match r {
            Ok(e) => {
                self.entries = e;
                self.error = None;
            }
            Err(e) => self.error = Some(e),
        }
        true
    }

    /// The rows: per repo (the newest PR's repo first) a head, its PRs newest first.
    fn layout(&self) -> (Vec<RowKind>, Vec<usize>) {
        let mut repos: Vec<(String, u64)> = Vec::new();
        for e in self.entries.iter().filter(|e| self.filter.keeps(e.item.state)) {
            match repos.iter_mut().find(|r| r.0 == e.item.pr.repo) {
                Some(r) => r.1 = r.1.max(e.item.created),
                None => repos.push((e.item.pr.repo.clone(), e.item.created)),
            }
        }
        repos.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        let mut rows = Vec::new();
        let mut prs = Vec::new();
        for (repo, _) in repos {
            let mut of: Vec<usize> = (0..self.entries.len()).filter(|&i| self.entries[i].item.pr.repo == repo && self.filter.keeps(self.entries[i].item.state)).collect();
            of.sort_by(|&a, &b| self.entries[b].item.created.cmp(&self.entries[a].item.created));
            rows.push(RowKind::Repo(repo, of.len()));
            for i in of {
                rows.push(RowKind::Pr(i));
                prs.push(i);
            }
        }
        (rows, prs)
    }

    pub fn key(&mut self, k: KeyEvent) -> Pick {
        let (_, prs) = self.layout();
        let n = prs.len();
        match k.code {
            KeyCode::Up | KeyCode::Char('j') => self.cursor = self.cursor.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('k') => self.cursor = (self.cursor + 1).min(n.saturating_sub(1)),
            KeyCode::PageUp => self.cursor = self.cursor.saturating_sub(10),
            KeyCode::PageDown => self.cursor = (self.cursor + 10).min(n.saturating_sub(1)),
            KeyCode::Home | KeyCode::Char('g') => self.cursor = 0,
            KeyCode::End | KeyCode::Char('G') => self.cursor = n.saturating_sub(1),
            KeyCode::Char('s') => {
                self.filter = self.filter.next();
                self.cursor = 0;
                return Pick::Toast(format!("pull requests: {}", self.filter.label()));
            }
            KeyCode::Char('r') => {
                if self.read.is_some() || self.at.is_some_and(|t| t.elapsed() < MIN_MANUAL) {
                    return Pick::Toast("pull requests: a read on its way".into());
                }
                self.start();
                return Pick::Toast("pull requests: reading GitHub".into());
            }
            KeyCode::Enter => {
                if let Some(e) = prs.get(self.cursor).map(|&i| self.entries[i].clone()) {
                    return Pick::Open(e);
                }
            }
            _ => {}
        }
        Pick::None
    }

    pub fn mouse(&mut self, m: MouseEvent) -> Pick {
        let at = Position { x: m.column, y: m.row };
        match m.kind {
            MouseEventKind::ScrollDown => self.offset += 3,
            MouseEventKind::ScrollUp => self.offset = self.offset.saturating_sub(3),
            MouseEventKind::Down(MouseButton::Left) => {
                if let Some(&(_, k)) = self.rows.iter().find(|(r, _)| r.contains(at)) {
                    if self.cursor == k {
                        return self.key(KeyEvent::from(KeyCode::Enter));
                    }
                    self.cursor = k;
                }
            }
            _ => {}
        }
        Pick::None
    }

    pub fn draw(&mut self, f: &mut Frame, area: Rect, focused: bool) {
        self.rows.clear();
        if area.height < 2 {
            return;
        }
        let (rows, prs) = self.layout();
        self.cursor = self.cursor.min(prs.len().saturating_sub(1));
        let state = if self.read.is_some() {
            "reading GitHub…".to_string()
        } else if let Some(e) = &self.error {
            format!("gh: {e}")
        } else {
            self.at.map(|t| format!("read {}s ago", t.elapsed().as_secs())).unwrap_or_default()
        };
        let head = Line::from(vec![
            Span::styled(format!("your pull requests · {} · {} ", prs.len(), self.filter.label()), theme::bold()),
            Span::styled(format!("· {state} · s: state · r: read again · enter: open"), theme::muted()),
        ]);
        f.render_widget(Paragraph::new(head), Rect { height: 1, ..area });
        let list = Rect { y: area.y + 2, height: area.height.saturating_sub(2), ..area };
        if rows.is_empty() {
            let msg = if self.read.is_some() { "  reading…" } else { "  0 pull requests" };
            f.render_widget(Paragraph::new(Line::styled(msg, theme::muted())), list);
            return;
        }
        let h = list.height as usize;
        let cur_row = prs.get(self.cursor).and_then(|&i| rows.iter().position(|r| *r == RowKind::Pr(i))).unwrap_or(0);
        if cur_row < self.offset + 1 {
            self.offset = cur_row.saturating_sub(1);
        } else if cur_row >= self.offset + h {
            self.offset = cur_row + 1 - h;
        }
        self.offset = self.offset.min(rows.len().saturating_sub(h));
        let w = list.width as usize;
        for (k, r) in rows.iter().enumerate().skip(self.offset).take(h) {
            let y = list.y + (k - self.offset) as u16;
            let rect = Rect { y, height: 1, ..list };
            match r {
                RowKind::Repo(name, n) => {
                    let line = Line::from(vec![Span::styled(format!(" {name}"), theme::accent_bold()), Span::styled(format!("  {n}"), theme::muted())]);
                    f.render_widget(Paragraph::new(line), rect);
                }
                RowKind::Pr(i) => {
                    let e = &self.entries[*i];
                    let pick = prs.iter().position(|x| x == i).unwrap_or(0);
                    let (mark, c) = state_mark(e.item.state);
                    let thread = e.thread.as_ref().map(|t| format!("  ⧉ {}", crate::nav::fit(t.title.lines().next().unwrap_or_default(), 28))).unwrap_or_default();
                    let right = format!("{}{thread} ", gh::stamp(e.item.created));
                    let num = format!("   #{:<6}", e.item.pr.number);
                    let room = w.saturating_sub(num.width() + right.width() + 4);
                    let title = crate::nav::fit(&e.item.title, room);
                    let pad = room.saturating_sub(title.width());
                    let on = pick == self.cursor;
                    let base = if on && focused { theme::pill() } else { Style::default() };
                    let line = Line::from(vec![
                        Span::styled(num, base.fg(theme::pal().muted)),
                        Span::styled(format!("{mark} "), base.fg(c)),
                        Span::styled(title, if on { base.add_modifier(Modifier::BOLD) } else { base.fg(theme::pal().text) }),
                        Span::styled(" ".repeat(pad + 1), base),
                        Span::styled(right, base.fg(theme::pal().muted)),
                    ]);
                    f.render_widget(Paragraph::new(line).style(base), rect);
                    self.rows.push((rect, pick));
                }
            }
        }
    }
}

fn state_mark(s: State) -> (&'static str, Color) {
    match s {
        State::Open => ("●", theme::pal().ok),
        State::Draft => ("◌", theme::pal().muted),
        State::Merged => ("◆", Color::Rgb(163, 113, 247)),
        State::Closed => ("✕", theme::pal().danger),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(repo: &str, n: u64, created: u64, state: State) -> Entry {
        Entry { item: Item { pr: PrRef::new(repo, n), title: format!("pr {n}"), state, created, head: format!("b{n}") }, thread: None }
    }

    #[test]
    fn grouped_by_repo_newest_first() {
        let l = PrList::with(vec![entry("a/x", 1, 10, State::Open), entry("b/y", 2, 30, State::Merged), entry("a/x", 3, 20, State::Closed), entry("b/y", 4, 5, State::Draft)]);
        let (rows, prs) = l.layout();
        assert_eq!(rows, vec![RowKind::Repo("b/y".into(), 2), RowKind::Pr(1), RowKind::Pr(3), RowKind::Repo("a/x".into(), 2), RowKind::Pr(2), RowKind::Pr(0)]);
        assert_eq!(prs, vec![1, 3, 2, 0]);
    }

    #[test]
    fn the_state_filter_and_enter() {
        let mut l = PrList::with(vec![entry("a/x", 1, 10, State::Open), entry("a/x", 2, 20, State::Merged), entry("a/x", 3, 30, State::Draft)]);
        assert!(matches!(l.key(KeyEvent::from(KeyCode::Char('s'))), Pick::Toast(_)));
        assert_eq!(l.layout().1, vec![2, 0], "open: drafts too");
        assert!(matches!(l.key(KeyEvent::from(KeyCode::Down)), Pick::None));
        assert_eq!(l.key(KeyEvent::from(KeyCode::Enter)), Pick::Open(l.entries[0].clone()));
    }

    /// CODEHEAD_GH_LIVE=1 cargo test live_prlist -- --ignored --nocapture
    #[test]
    #[ignore]
    fn live_prlist_draws() {
        let e = load().unwrap();
        assert!(!e.is_empty());
        let mut l = PrList::with(e);
        let backend = ratatui::backend::TestBackend::new(120, 16);
        let mut term = ratatui::Terminal::new(backend).unwrap();
        term.draw(|f| l.draw(f, f.area(), true)).unwrap();
        let buf = term.backend().buffer().clone();
        println!("{}", (0..16).map(|y| (0..120).map(|x| buf[(x, y)].symbol().to_string()).collect::<String>()).collect::<Vec<_>>().join("\n"));
    }

    #[test]
    fn items_round_trip_the_cache() {
        let i = entry("a/x", 7, 99, State::Draft).item;
        assert_eq!(item_of(&item_json(&i)), Some(i));
    }
}
