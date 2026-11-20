//! The merge tab: the pull requests of a thread (the branches of its checkouts, the PR links in
//! its log), or 1 PR without a thread (the start screen's merge tab). 1 tab per PR
//! (`platform#1234`), 2 views each: overview (hero, actions, checks, comments, description) and
//! changes (the changes view on the PR's diff, a commit picker in its panel).
//!
//! Keys: ↑↓ (j k) move between the things to act on (not lines), ←→ along a row, enter acts,
//! ctrl+enter / ctrl+click opens on GitHub, [ ] the previous / next PR, < > overview / changes,
//! r reads again. Edits happen in place (inline inputs): ctrl+enter (alt+enter) saves, esc drops.
//!
//! gh use: 1 GraphQL call per read of a PR. The PR in view: every 30 s (every 12 s for 2 min
//! after an edit, 20 s while checks run); 0 reads after 5 min without a key (sleep, until the
//! next key) or with the tab out of view; max. 20 calls per minute; `r`: at once (3 s apart).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use unicode_width::UnicodeWidthStr;

use crate::gh::{self, Kind, Op, Pr, PrRef, Ready, State, Verdict};
use crate::textarea::{Outcome, TextArea};
use crate::theme;

const POLL: Duration = Duration::from_secs(30);
const POLL_FAST: Duration = Duration::from_secs(12);
const POLL_CHECKS: Duration = Duration::from_secs(20);
/// Fast polls this long after an edit.
const FAST_FOR: Duration = Duration::from_secs(120);
/// 0 polls after this long without a key.
const SLEEP: Duration = Duration::from_secs(300);
/// The PR list of a thread: read again after this long.
const ASSOC: Duration = Duration::from_secs(300);
const MIN_MANUAL: Duration = Duration::from_secs(3);
const MAX_PER_MIN: usize = 20;
/// A 2nd enter on merge within this long merges.
const ARM: Duration = Duration::from_secs(4);

/// What the host (thread view, start screen) does after a key / click.
#[derive(Clone, Debug, PartialEq)]
pub enum Act {
    None,
    Toast(String),
    /// A message for the agents (thread) / the new-thread view (0 thread), the owner sends it.
    Prompt(String),
    /// Open the changes view's file in the IDE.
    Open(String, u32),
}

/// A thing to act on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stop {
    Tabs,
    Subs,
    Title,
    Status,
    Label(usize),
    AddLabel,
    Update,
    Merge,
    Checks,
    Check(usize),
    NewComment,
    /// Thread I of the conversation, its comment K (0: the 1st, k: reply k).
    Comment(usize, usize),
    Body,
}

impl Stop {
    /// The row of stops it shares (←→ move along it).
    fn row(self) -> u8 {
        match self {
            Stop::Label(_) | Stop::AddLabel => 1,
            Stop::Update | Stop::Merge => 2,
            _ => 0,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Sub {
    #[default]
    Overview,
    Changes,
}

/// An inline input: what it edits.
enum Edit {
    Title,
    Body,
    /// A new comment (a quote of another one, or empty).
    Comment,
    /// A reply on line thread I (its 1st comment's id).
    Reply(usize, u64),
    /// Comment K of thread I (its kind, id).
    Change(usize, usize, Kind, u64),
}

enum Menu {
    /// The status dropdown: its options.
    Status(usize, Vec<(&'static str, Op)>),
    /// The own comment's dropdown: edit, delete (armed: a 2nd enter deletes).
    Mine { cur: usize, thread: usize, k: usize, armed: bool },
    /// The labels dropdown: the filter, the repo's labels (None: on their way).
    Labels { cur: usize, filter: Box<TextArea> },
}

/// 1 PR's tab.
#[derive(Default)]
struct Tab {
    pr: Option<Pr>,
    error: Option<String>,
    sub: Sub,
    focus: Option<Stop>,
    scroll: usize,
    checks_open: bool,
    edit: Option<(Edit, TextArea)>,
    menu: Option<Menu>,
    read_at: Option<Instant>,
    reading: bool,
    fast_until: Option<Instant>,
    armed_merge: Option<Instant>,
    /// An edit on its way.
    busy: Option<String>,
    changes: Option<crate::changes::View>,
}

enum Res {
    Pr(PrRef, Box<Result<Pr, String>>),
    Prs(Result<Vec<PrRef>, String>),
    Op(PrRef, Op, Result<(), String>),
    Labels(String, Result<Vec<gh::Label>, String>),
}

/// What a thread's PRs come from.
#[derive(Clone, Debug, Default)]
pub struct Sources {
    /// (checkout dir, branch).
    pub checkouts: Vec<(PathBuf, Option<String>)>,
    /// The PR links of the log.
    pub links: Vec<PrRef>,
}

pub struct Merge {
    /// The thread (None: 1 PR, the start screen).
    thread: bool,
    thread_id: String,
    pub prs: Vec<PrRef>,
    cur: usize,
    tabs: HashMap<PrRef, Tab>,
    tx: Sender<Res>,
    rx: Receiver<Res>,
    sources: Option<Sources>,
    assoc_at: Option<Instant>,
    assoc_busy: bool,
    assoc_error: Option<String>,
    /// The owner's last key / click (sleep after SLEEP).
    active: Instant,
    manual_at: Option<Instant>,
    /// The times of the calls in the last minute.
    calls: Vec<Instant>,
    /// The repo labels, by repo.
    labels: HashMap<String, Vec<gh::Label>>,
    /// The checkout of each repo (the changes view's dir, the agents' hint).
    dirs: HashMap<String, PathBuf>,
    /// Last draw: click targets, the view's area, the doc rows' screen rows.
    hits: Vec<(Rect, Stop)>,
    tab_hits: Vec<(Rect, usize)>,
    sub_hits: Vec<(Rect, Sub)>,
    menu_hits: Vec<(Rect, usize)>,
    area: Rect,
    body_area: Rect,
    edit_rect: Rect,
    pub note: Option<String>,
}

/// A row of the overview: its line, the stops on it (x from, width, stop).
struct Row {
    line: Line<'static>,
    stops: Vec<(u16, u16, Stop)>,
    /// The inline input goes here (its rows).
    input: u16,
}

impl Row {
    fn new(line: Line<'static>) -> Row {
        Row { line, stops: Vec::new(), input: 0 }
    }

    fn stop(line: Line<'static>, s: Stop) -> Row {
        let w = line.width() as u16;
        Row { line, stops: vec![(0, w, s)], input: 0 }
    }
}

fn hex(c: &str) -> Color {
    let v = u32::from_str_radix(c.trim_start_matches('#'), 16).unwrap_or(0x808080);
    Color::Rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

/// Text on a label of color BG: dark on light, light on dark.
fn on(bg: Color) -> Color {
    match bg {
        Color::Rgb(r, g, b) if (r as u32 * 299 + g as u32 * 587 + b as u32 * 114) / 1000 > 140 => Color::Rgb(20, 20, 24),
        _ => Color::Rgb(240, 240, 244),
    }
}

fn secs(s: u64) -> String {
    match s {
        0 => String::new(),
        s if s < 60 => format!("{s}s"),
        s if s < 3600 => format!("{}m {}s", s / 60, s % 60),
        s => format!("{}h {}m", s / 3600, (s % 3600) / 60),
    }
}

pub fn open_url(url: &str) {
    if cfg!(test) || url.is_empty() {
        return;
    }
    let bin = if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
    let _ = std::process::Command::new(bin).arg(url).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn();
}

impl Merge {
    /// The merge tab of a thread: its PRs come from SOURCES (read in the background).
    pub fn for_thread(id: &str) -> Merge {
        let mut m = Merge::new(true, Vec::new());
        m.thread_id = id.to_string();
        m
    }

    /// 1 PR (the start screen).
    pub fn one(pr: PrRef) -> Merge {
        Merge::new(false, vec![pr])
    }

    fn new(thread: bool, prs: Vec<PrRef>) -> Merge {
        let (tx, rx) = mpsc::channel();
        Merge {
            thread,
            thread_id: String::new(),
            prs,
            cur: 0,
            tabs: HashMap::new(),
            tx,
            rx,
            sources: None,
            assoc_at: None,
            assoc_busy: false,
            assoc_error: None,
            active: Instant::now(),
            manual_at: None,
            calls: Vec::new(),
            labels: HashMap::new(),
            dirs: HashMap::new(),
            hits: Vec::new(),
            tab_hits: Vec::new(),
            sub_hits: Vec::new(),
            menu_hits: Vec::new(),
            area: Rect::default(),
            body_area: Rect::default(),
            edit_rect: Rect::default(),
            note: None,
        }
    }

    /// The thread's sources changed (checkouts, log links): the PR list is read again when due.
    pub fn set_sources(&mut self, s: Sources) {
        let same = self.sources.as_ref().is_some_and(|o| o.checkouts == s.checkouts && o.links == s.links);
        if !same {
            // new links: at once; new checkouts: at once
            self.assoc_at = None;
        }
        self.sources = Some(s);
    }

    /// The checkout DIR of REPO (the changes view's dir, the agents' hint).
    pub fn set_dir(&mut self, repo: &str, dir: PathBuf) {
        self.dirs.insert(repo.to_string(), dir);
    }

    /// Show PR (a tab of it: in front).
    pub fn show(&mut self, pr: &PrRef) {
        if !self.prs.contains(pr) {
            self.prs.insert(0, pr.clone());
        }
        self.cur = self.prs.iter().position(|p| p == pr).unwrap_or(0);
    }

    pub fn current(&self) -> Option<&PrRef> {
        self.prs.get(self.cur)
    }

    fn tab(&mut self) -> Option<&mut Tab> {
        let pr = self.prs.get(self.cur)?.clone();
        Some(self.tabs.entry(pr).or_default())
    }

    fn tab_ref(&self) -> Option<&Tab> {
        self.tabs.get(self.prs.get(self.cur)?)
    }

    /// The tab's label of the view bar: `merge 2`.
    pub fn count(&self) -> usize {
        self.prs.len()
    }

    /// Keys typed into an inline input / a dropdown filter.
    pub fn typing(&self) -> bool {
        self.tab_ref().is_some_and(|t| t.edit.is_some() || matches!(t.menu, Some(Menu::Labels { .. })) || (t.sub == Sub::Changes && t.changes.as_ref().is_some_and(|c| c.typing())))
    }

    /// Esc is the tab's: it closes an input, a dropdown, the checks list.
    pub fn takes_esc(&self) -> bool {
        self.tab_ref().is_some_and(|t| t.edit.is_some() || t.menu.is_some() || (t.sub == Sub::Changes && t.changes.as_ref().is_some_and(|c| c.takes_esc())))
    }

    /// A gh call may go now (max. MAX_PER_MIN per minute).
    fn budget(&mut self) -> bool {
        self.calls.retain(|t| t.elapsed() < Duration::from_secs(60));
        if self.calls.len() >= MAX_PER_MIN {
            return false;
        }
        self.calls.push(Instant::now());
        true
    }

    fn spawn(&self, f: impl FnOnce() -> Res + Send + 'static) {
        let tx = self.tx.clone();
        if cfg!(test) {
            let _ = tx.send(f());
            return;
        }
        std::thread::spawn(move || {
            let _ = tx.send(f());
        });
    }

    fn read(&mut self, pr: PrRef) {
        if !self.budget() {
            return;
        }
        let t = self.tabs.entry(pr.clone()).or_default();
        t.reading = true;
        t.read_at = Some(Instant::now());
        self.spawn(move || {
            let r = gh::fetch(&pr);
            Res::Pr(pr, Box::new(r))
        });
    }

    fn read_assoc(&mut self) {
        let Some(s) = self.sources.clone() else { return };
        if !self.budget() {
            return;
        }
        self.assoc_busy = true;
        self.assoc_at = Some(Instant::now());
        self.spawn(move || Res::Prs(associate(&s)));
    }

    /// Background results in, due reads out (VISIBLE: the tab shows). True: a redraw is due.
    pub fn poll(&mut self, visible: bool) -> bool {
        let mut dirty = false;
        while let Ok(r) = self.rx.try_recv() {
            dirty = true;
            match r {
                Res::Pr(pr, r) => {
                    let t = self.tabs.entry(pr).or_default();
                    t.reading = false;
                    match *r {
                        Ok(p) => {
                            if let Some(c) = t.changes.as_mut() {
                                c.set_pr(&p);
                            }
                            t.pr = Some(p);
                            t.error = None;
                        }
                        Err(e) => t.error = Some(e),
                    }
                }
                Res::Prs(r) => {
                    self.assoc_busy = false;
                    match r {
                        Ok(list) => {
                            crate::prlist::remember(&self.thread_id, &list);
                            let keep = self.current().cloned();
                            self.prs = list;
                            self.cur = keep.and_then(|k| self.prs.iter().position(|p| *p == k)).unwrap_or(0);
                            self.assoc_error = None;
                        }
                        Err(e) => self.assoc_error = Some(e),
                    }
                }
                Res::Op(pr, op, r) => {
                    let t = self.tabs.entry(pr.clone()).or_default();
                    t.busy = None;
                    t.fast_until = Some(Instant::now() + FAST_FOR);
                    match r {
                        Ok(()) => {
                            self.note = Some(op.done().to_string());
                            if let (Some(p), Op::AddLabel(l) | Op::RemoveLabel(l)) = (t.pr.as_mut(), &op) {
                                // at once on screen; the read confirms
                                if matches!(op, Op::AddLabel(_)) {
                                    let color = self.labels.get(&pr.repo).and_then(|ls| ls.iter().find(|x| &x.name == l)).map(|x| x.color.clone()).unwrap_or_default();
                                    p.labels.push(gh::Label { name: l.clone(), color });
                                } else {
                                    p.labels.retain(|x| &x.name != l);
                                }
                            }
                            self.read(pr);
                        }
                        Err(e) => self.note = Some(format!("gh: {e}")),
                    }
                }
                Res::Labels(repo, r) => match r {
                    Ok(l) => {
                        self.labels.insert(repo, l);
                    }
                    Err(e) => self.note = Some(format!("labels: {e}")),
                },
            }
        }
        if let Some(c) = self.tab().and_then(|t| if t.sub == Sub::Changes { t.changes.as_mut() } else { None })
            && c.poll()
        {
            dirty = true;
        }
        if !visible || self.active.elapsed() > SLEEP || cfg!(test) {
            return dirty;
        }
        // the thread's PR list
        if self.thread && !self.assoc_busy && self.sources.is_some() && self.assoc_at.is_none_or(|t| t.elapsed() > ASSOC) {
            self.read_assoc();
        }
        // the PR in view
        if let Some(pr) = self.current().cloned() {
            let t = self.tabs.entry(pr.clone()).or_default();
            let every = if t.fast_until.is_some_and(|u| Instant::now() < u) {
                POLL_FAST
            } else if t.pr.as_ref().is_some_and(|p| p.checks.iter().any(|c| c.verdict == Verdict::Pending)) {
                POLL_CHECKS
            } else {
                POLL
            };
            // a merged / closed PR: rarely changes
            let every = if t.pr.as_ref().is_some_and(|p| matches!(p.state, State::Merged | State::Closed)) { every * 6 } else { every };
            if !t.reading && t.read_at.is_none_or(|r| r.elapsed() > every) {
                self.read(pr);
            }
        }
        dirty
    }

    /// The owner is at work (a key anywhere in the view): 0 sleep.
    pub fn touch(&mut self) {
        self.active = Instant::now();
    }

    /// `r`: read again now (3 s apart).
    fn manual(&mut self) -> Act {
        if self.manual_at.is_some_and(|t| t.elapsed() < MIN_MANUAL) {
            return Act::Toast("read: wait a moment".into());
        }
        self.manual_at = Some(Instant::now());
        if self.thread {
            self.assoc_at = None;
            self.read_assoc();
        }
        if let Some(pr) = self.current().cloned() {
            self.read(pr);
            if let Some(c) = self.tab().and_then(|t| t.changes.as_mut()) {
                c.refresh(true);
            }
        }
        Act::Toast("merge: reading GitHub".into())
    }

    fn run(&mut self, op: Op) -> Act {
        let Some(pr) = self.current().cloned() else { return Act::None };
        let t = self.tabs.entry(pr.clone()).or_default();
        if t.busy.is_some() {
            return Act::Toast("gh: 1 edit at a time".into());
        }
        t.busy = Some(format!("{op:?}"));
        let o = op.clone();
        self.spawn(move || {
            let r = gh::run(&pr, &o);
            Res::Op(pr, o, r)
        });
        Act::None
    }

    /// The context of the PR in view for a message to the agents.
    pub fn context(&self) -> Option<String> {
        let p = self.tab_ref()?.pr.as_ref()?;
        let (title, _) = p.ready().text();
        let mut s = format!("[codehead: context from the merge tab]\nPull request {} ({}): “{}” · {} · {} · `{}` → `{}`", p.url, p.pr.label(), p.title, p.state.label(), title, p.head, p.base);
        if let Some(d) = self.dirs.get(&p.pr.repo) {
            s.push_str(&format!("\nCheckout: {}", d.display()));
        }
        let failed: Vec<String> = p.checks.iter().filter(|c| c.verdict == Verdict::Fail).map(|c| format!("- {} ({})", check_name(c), c.url)).collect();
        if !failed.is_empty() {
            s.push_str(&format!("\nFailed checks:\n{}", failed.join("\n")));
        }
        let open: Vec<String> = p.threads.iter().filter(|t| t.path.is_some() && !t.resolved).map(|t| format!("- {}:{} {}: {}", t.path.as_deref().unwrap_or_default(), t.line.unwrap_or(0), t.root.author, t.root.body.lines().next().unwrap_or_default())).collect();
        if !open.is_empty() {
            s.push_str(&format!("\nOpen review threads:\n{}", open.join("\n")));
        }
        Some(s)
    }

    /// The message to update the branch of P (behind: a merge; conflicts: a smart resolve).
    fn update_prompt(&self, p: &Pr) -> String {
        let dir = self.dirs.get(&p.pr.repo).map(|d| format!(" The checkout: `{}`.", d.display())).unwrap_or_default();
        if p.conflicts() {
            format!(
                "The branch `{}` of pull request {} has conflicts with `{}`.{dir} Fetch, merge `origin/{}` into the branch (0 rebase, 0 force push) and resolve every conflict: keep the intent of both sides; read the commits on both sides where it is unclear. Build and run the tests, commit the merge, push. Then list each conflict and how you resolved it.",
                p.head, p.url, p.base, p.base
            )
        } else {
            format!("Update the branch `{}` of pull request {} with `{}`.{dir} Fetch, merge `origin/{}` into it (0 rebase, 0 force push), push. 0 conflicts expected: if there are any, stop and tell me.", p.head, p.url, p.base, p.base)
        }
    }

    // ---------- keys ----------

    pub fn key(&mut self, k: KeyEvent) -> Act {
        self.active = Instant::now();
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        if self.prs.is_empty() {
            return match k.code {
                KeyCode::Char('r') => self.manual(),
                _ => Act::None,
            };
        }
        // an inline input: its keys
        if self.tab_ref().is_some_and(|t| t.edit.is_some()) {
            return self.edit_key(k);
        }
        if self.tab_ref().is_some_and(|t| t.menu.is_some()) {
            return self.menu_key(k);
        }
        match k.code {
            KeyCode::Char('[') => return self.step_pr(-1),
            KeyCode::Char(']') => return self.step_pr(1),
            KeyCode::Char('<') => return self.set_sub(Sub::Overview),
            KeyCode::Char('>') => return self.set_sub(Sub::Changes),
            _ => {}
        }
        if self.tab_ref().is_some_and(|t| t.sub == Sub::Changes) {
            if ctrl && k.code == KeyCode::Char('r') {
                return self.manual();
            }
            let Some(c) = self.changes_view() else { return Act::None };
            return match c.key(k) {
                crate::changes::Act::Toast(t) => Act::Toast(t),
                crate::changes::Act::Open(f, l) => Act::Open(f, l),
                crate::changes::Act::Send(t) => Act::Prompt(t),
                _ => Act::None,
            };
        }
        let stops = self.stops();
        let Some(tab) = self.tab() else { return Act::None };
        let cur = tab.focus.filter(|s| stops.contains(s)).or(stops.get(2).copied()).or(stops.first().copied());
        tab.focus = cur;
        let pos = cur.and_then(|c| stops.iter().position(|s| *s == c)).unwrap_or(0);
        let last = stops.len().saturating_sub(1);
        let mv = |tab: &mut Tab, i: usize| tab.focus = stops.get(i).copied();
        match k.code {
            KeyCode::Down | KeyCode::Char('k') => {
                // the next stop of another row (a row of stops: its 1st)
                let row = stops.get(pos).map(|s| s.row()).unwrap_or(0);
                let mut i = pos + 1;
                while row != 0 && i <= last && stops[i].row() == row {
                    i += 1;
                }
                mv(tab, i.min(last));
            }
            KeyCode::Up | KeyCode::Char('j') => {
                let row = stops.get(pos).map(|s| s.row()).unwrap_or(0);
                let mut i = pos;
                while row != 0 && i > 0 && stops[i - 1].row() == row {
                    i -= 1;
                }
                let i = i.saturating_sub(1);
                // into a row: its 1st stop
                let r = stops[i].row();
                let mut j = i;
                while r != 0 && j > 0 && stops[j - 1].row() == r {
                    j -= 1;
                }
                mv(tab, j);
            }
            KeyCode::Home | KeyCode::Char('g') => mv(tab, 0),
            KeyCode::End | KeyCode::Char('G') => mv(tab, last),
            KeyCode::PageDown => mv(tab, (pos + 6).min(last)),
            KeyCode::PageUp => mv(tab, pos.saturating_sub(6)),
            KeyCode::Left | KeyCode::Right => {
                let right = k.code == KeyCode::Right;
                match cur {
                    Some(Stop::Tabs) => return self.step_pr(if right { 1 } else { -1 }),
                    Some(Stop::Subs) => return self.set_sub(if right { Sub::Changes } else { Sub::Overview }),
                    Some(Stop::Checks) => {
                        tab.checks_open = right;
                    }
                    Some(Stop::Check(_)) if !right => {
                        tab.checks_open = false;
                        tab.focus = Some(Stop::Checks);
                    }
                    Some(s) if s.row() != 0 => {
                        let i = if right { pos + 1 } else { pos.wrapping_sub(1) };
                        if stops.get(i).is_some_and(|x| x.row() == s.row()) {
                            mv(tab, i);
                        }
                    }
                    _ => {}
                }
            }
            KeyCode::Char('r') if !ctrl => return self.manual(),
            KeyCode::Enter if ctrl || k.modifiers.contains(KeyModifiers::ALT) || k.modifiers.contains(KeyModifiers::SUPER) => {
                if let Some(s) = cur {
                    return self.open_on_github(s);
                }
            }
            KeyCode::Enter => {
                if let Some(s) = cur {
                    return self.activate(s);
                }
            }
            _ => {}
        }
        Act::None
    }

    fn step_pr(&mut self, d: isize) -> Act {
        let n = self.prs.len();
        if n > 0 {
            self.cur = (self.cur as isize + d).rem_euclid(n as isize) as usize;
        }
        Act::None
    }

    fn set_sub(&mut self, s: Sub) -> Act {
        let dir = self.current().and_then(|p| self.dirs.get(&p.repo).cloned()).unwrap_or_else(crate::engine::data_dir);
        let Some(pr) = self.current().cloned() else { return Act::None };
        let Some(t) = self.tab() else { return Act::None };
        t.sub = s;
        t.menu = None;
        if s == Sub::Changes && t.changes.is_none() {
            let mut v = crate::changes::View::for_pr(&pr, &dir);
            if let Some(p) = &t.pr {
                v.set_pr(p);
            }
            v.refresh(true);
            t.changes = Some(v);
        }
        Act::None
    }

    fn changes_view(&mut self) -> Option<&mut crate::changes::View> {
        self.tab()?.changes.as_mut()
    }

    fn open_on_github(&mut self, s: Stop) -> Act {
        let Some(p) = self.tab_ref().and_then(|t| t.pr.as_ref()) else { return Act::None };
        let url = match s {
            Stop::Check(i) => p.checks.iter().filter(|c| c.verdict != Verdict::Skip).nth(i).map(|c| c.url.clone()),
            Stop::Comment(i, k) => p.threads.get(i).and_then(|t| if k == 0 { Some(&t.root) } else { t.replies.get(k - 1) }).map(|c| c.url.clone()),
            Stop::Checks => Some(format!("{}/checks", p.url)),
            _ => Some(p.url.clone()),
        };
        match url.filter(|u| !u.is_empty()) {
            Some(u) => {
                open_url(&u);
                Act::Toast(format!("open {u}"))
            }
            None => Act::None,
        }
    }

    fn activate(&mut self, s: Stop) -> Act {
        let Some(p) = self.tab_ref().and_then(|t| t.pr.clone()) else { return Act::None };
        let mine = p.author == p.viewer;
        let edit = |t: &mut Tab, e: Edit, text: &str, single: bool| {
            let mut ta = TextArea::new(single, "");
            ta.set_text(text);
            ta.move_end();
            t.edit = Some((e, ta));
        };
        let Some(t) = self.tab() else { return Act::None };
        match s {
            Stop::Tabs => return self.step_pr(1),
            Stop::Subs => return self.set_sub(Sub::Changes),
            Stop::Title if p.can_edit => edit(t, Edit::Title, &p.title, true),
            Stop::Body if p.can_edit => edit(t, Edit::Body, &p.body, false),
            Stop::Title | Stop::Body => return Act::Toast("0 rights to edit this pull request".into()),
            Stop::Status => {
                let opts: Vec<(&'static str, Op)> = match p.state {
                    State::Open => vec![("convert to draft", Op::Draft), ("close pull request", Op::Close)],
                    State::Draft => vec![("ready for review", Op::Ready), ("close pull request", Op::Close)],
                    State::Closed => vec![("reopen pull request", Op::Reopen)],
                    State::Merged => return Act::Toast("merged: 0 status changes".into()),
                };
                if !p.can_edit {
                    return Act::Toast("0 rights to change this pull request".into());
                }
                t.menu = Some(Menu::Status(0, opts));
            }
            Stop::Label(i) => {
                if let Some(l) = p.labels.get(i) {
                    return self.run(Op::RemoveLabel(l.name.clone()));
                }
            }
            Stop::AddLabel => {
                let mut filter = TextArea::new(true, "filter labels");
                filter.style = Some(theme::text());
                t.menu = Some(Menu::Labels { cur: 0, filter: Box::new(filter) });
                let repo = p.pr.repo.clone();
                if !self.labels.contains_key(&repo) && self.budget() {
                    self.spawn(move || {
                        let r = gh::labels(&repo);
                        Res::Labels(repo, r)
                    });
                }
            }
            Stop::Update => return Act::Prompt(self.update_prompt(&p)),
            Stop::Merge => {
                if let Some(m) = &p.auto_merge {
                    let _ = m;
                    return self.run(Op::DisableAuto);
                }
                if !matches!(p.state, State::Open) {
                    return Act::Toast(if p.state == State::Draft { "a draft: mark it ready for review first".into() } else { format!("{}: 0 merge", p.state.label()) });
                }
                let ready = p.ready() == Ready::Mergeable;
                if !t.armed_merge.is_some_and(|a| a.elapsed() < ARM) {
                    t.armed_merge = Some(Instant::now());
                    let what = if ready { "merge" } else { "merge when ready" };
                    return Act::Toast(format!("enter again: {what} ({})", p.method.to_lowercase()));
                }
                t.armed_merge = None;
                return self.run(Op::Merge(p.method.clone(), !ready));
            }
            Stop::Checks => t.checks_open = !t.checks_open,
            Stop::Check(_) => {}
            Stop::NewComment => edit(t, Edit::Comment, "", false),
            Stop::Comment(i, k) => {
                let Some(th) = p.threads.get(i) else { return Act::None };
                let Some(c) = (if k == 0 { Some(&th.root) } else { th.replies.get(k - 1) }) else { return Act::None };
                if c.mine {
                    t.menu = Some(Menu::Mine { cur: 0, thread: i, k, armed: false });
                } else if c.kind == Kind::Line {
                    edit(t, Edit::Reply(i, th.root.id), "", false);
                } else {
                    edit(t, Edit::Comment, &gh::quote(&c.author, &c.body), false);
                    t.focus = Some(Stop::NewComment);
                }
            }
        }
        let _ = mine;
        Act::None
    }

    fn edit_key(&mut self, k: KeyEvent) -> Act {
        let save = k.code == KeyCode::Enter && (k.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER));
        let Some(t) = self.tab() else { return Act::None };
        let Some((e, ta)) = t.edit.as_mut() else { return Act::None };
        let single = matches!(e, Edit::Title);
        if k.code == KeyCode::Esc {
            t.edit = None;
            return Act::Toast("edit dropped".into());
        }
        if save || (single && k.code == KeyCode::Enter) {
            let text = ta.text();
            let (e, _) = t.edit.take().unwrap_or((Edit::Title, TextArea::new(true, "")));
            let pr = t.pr.clone();
            let op = match e {
                Edit::Title if text.trim().is_empty() => return Act::Toast("a title needs text".into()),
                Edit::Title => Op::Title(text.trim().to_string()),
                Edit::Body => Op::Body(text),
                _ if text.trim().is_empty() => return Act::Toast("0 text: 0 comment".into()),
                Edit::Comment => Op::Comment(text),
                Edit::Reply(_, id) => Op::Reply(id, text),
                Edit::Change(_, _, kind, id) => Op::Edit(kind, id, text),
            };
            // at once on screen (the read confirms)
            if let (Some(mut p), Some(t)) = (pr, self.tab()) {
                match &op {
                    Op::Title(x) => p.title = x.clone(),
                    Op::Body(x) => p.body = x.clone(),
                    _ => {}
                }
                t.pr = Some(p);
            }
            return self.run(op);
        }
        if let Outcome::Ignored = ta.key(k)
            && k.code == KeyCode::Enter
            && !single
        {
            ta.newline();
        }
        Act::None
    }

    /// The labels of the dropdown: the repo's (0 on the PR), filtered.
    fn label_opts(&self, filter: &str) -> Vec<gh::Label> {
        let Some(p) = self.tab_ref().and_then(|t| t.pr.as_ref()) else { return Vec::new() };
        let all = self.labels.get(&p.pr.repo).cloned().unwrap_or_default();
        let free: Vec<gh::Label> = all.into_iter().filter(|l| !p.labels.iter().any(|x| x.name == l.name)).collect();
        crate::fuzzy::rank(&free, filter, |l| l.name.clone(), |_| String::new()).into_iter().map(|(i, _)| free[i].clone()).collect()
    }

    fn menu_key(&mut self, k: KeyEvent) -> Act {
        let filter_text = match self.tab_ref().and_then(|t| t.menu.as_ref()) {
            Some(Menu::Labels { filter, .. }) => filter.text(),
            _ => String::new(),
        };
        let label_opts = self.label_opts(&filter_text);
        let p = self.tab_ref().and_then(|t| t.pr.clone());
        let Some(t) = self.tab() else { return Act::None };
        let Some(menu) = t.menu.as_mut() else { return Act::None };
        if k.code == KeyCode::Esc {
            t.menu = None;
            return Act::None;
        }
        let n = match menu {
            Menu::Status(_, o) => o.len(),
            Menu::Mine { .. } => 2,
            Menu::Labels { .. } => label_opts.len(),
        };
        let cur = match menu {
            Menu::Status(c, _) | Menu::Mine { cur: c, .. } | Menu::Labels { cur: c, .. } => c,
        };
        match k.code {
            KeyCode::Up => *cur = cur.saturating_sub(1),
            KeyCode::Down => *cur = (*cur + 1).min(n.saturating_sub(1)),
            KeyCode::Enter => {
                let c = *cur;
                match menu {
                    Menu::Status(_, o) => {
                        let op = o.get(c).map(|x| x.1.clone());
                        t.menu = None;
                        if let Some(op) = op {
                            return self.run(op);
                        }
                    }
                    Menu::Mine { thread, k: ck, armed, .. } => {
                        let (thread, ck) = (*thread, *ck);
                        let Some(com) = p.as_ref().and_then(|p| p.threads.get(thread)).and_then(|th| if ck == 0 { Some(th.root.clone()) } else { th.replies.get(ck - 1).cloned() }) else {
                            t.menu = None;
                            return Act::None;
                        };
                        if c == 0 {
                            t.menu = None;
                            let mut ta = TextArea::new(false, "");
                            ta.set_text(&com.body);
                            ta.move_end();
                            t.edit = Some((Edit::Change(thread, ck, com.kind, com.id), ta));
                        } else if com.kind == Kind::Review {
                            t.menu = None;
                            return Act::Toast("a review's text cannot be deleted: edit it".into());
                        } else if !*armed {
                            *armed = true;
                            return Act::Toast("enter again: delete the comment".into());
                        } else {
                            t.menu = None;
                            return self.run(Op::Delete(com.kind, com.id));
                        }
                    }
                    Menu::Labels { .. } => {
                        t.menu = None;
                        if let Some(l) = label_opts.get(c) {
                            return self.run(Op::AddLabel(l.name.clone()));
                        }
                    }
                }
            }
            _ => {
                if let Menu::Labels { filter, cur } = menu {
                    filter.key(k);
                    *cur = 0;
                }
            }
        }
        Act::None
    }

    pub fn mouse(&mut self, m: MouseEvent) -> Act {
        self.active = Instant::now();
        let at = Position { x: m.column, y: m.row };
        let click = matches!(m.kind, MouseEventKind::Down(MouseButton::Left));
        if click {
            if let Some(&(_, i)) = self.tab_hits.iter().find(|(r, _)| r.contains(at)) {
                self.cur = i;
                return Act::None;
            }
            if let Some(&(_, s)) = self.sub_hits.iter().find(|(r, _)| r.contains(at)) {
                return self.set_sub(s);
            }
        }
        if self.tab_ref().is_some_and(|t| t.sub == Sub::Changes) && self.body_area.contains(at) {
            let Some(c) = self.changes_view() else { return Act::None };
            return match c.mouse(m) {
                crate::changes::Act::Toast(t) => Act::Toast(t),
                crate::changes::Act::Open(f, l) => Act::Open(f, l),
                _ => Act::None,
            };
        }
        match m.kind {
            MouseEventKind::ScrollDown => {
                if let Some(t) = self.tab() {
                    t.scroll += 3;
                }
                return Act::None;
            }
            MouseEventKind::ScrollUp => {
                if let Some(t) = self.tab() {
                    t.scroll = t.scroll.saturating_sub(3);
                }
                return Act::None;
            }
            _ => {}
        }
        if !click {
            return Act::None;
        }
        // an open dropdown: a click on a row picks it, elsewhere closes it
        if self.tab_ref().is_some_and(|t| t.menu.is_some()) {
            if let Some(&(_, i)) = self.menu_hits.iter().find(|(r, _)| r.contains(at)) {
                if let Some(t) = self.tab()
                    && let Some(Menu::Status(c, _) | Menu::Mine { cur: c, .. } | Menu::Labels { cur: c, .. }) = t.menu.as_mut()
                {
                    *c = i;
                }
                return self.menu_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
            }
            if let Some(t) = self.tab() {
                t.menu = None;
            }
            return Act::None;
        }
        if self.edit_rect.contains(at) {
            if let Some((_, ta)) = self.tab().and_then(|t| t.edit.as_mut()) {
                ta.mouse(m);
            }
            return Act::None;
        }
        let Some(&(_, s)) = self.hits.iter().find(|(r, _)| r.contains(at)) else { return Act::None };
        if let Some(t) = self.tab() {
            t.focus = Some(s);
            t.edit = None;
        }
        if m.modifiers.contains(KeyModifiers::CONTROL) || m.modifiers.contains(KeyModifiers::SUPER) {
            return self.open_on_github(s);
        }
        match s {
            // a click on a label removes it, on the checks banner toggles them
            Stop::Label(_) | Stop::AddLabel | Stop::Status | Stop::Checks | Stop::Update | Stop::Merge | Stop::NewComment | Stop::Tabs | Stop::Subs => self.activate(s),
            _ => Act::None,
        }
    }

    // ---------- draw ----------

    /// The stops of the tab in view, in order.
    fn stops(&self) -> Vec<Stop> {
        let mut v = vec![Stop::Tabs, Stop::Subs];
        let Some(t) = self.tab_ref() else { return v };
        if t.sub == Sub::Changes {
            return v;
        }
        let Some(p) = &t.pr else { return v };
        v.extend([Stop::Title, Stop::Status]);
        v.extend((0..p.labels.len()).map(Stop::Label));
        if p.can_edit {
            v.push(Stop::AddLabel);
        }
        if p.needs_update() {
            v.push(Stop::Update);
        }
        if matches!(p.state, State::Open | State::Draft) {
            v.push(Stop::Merge);
        }
        v.push(Stop::Checks);
        if t.checks_open {
            v.extend((0..p.checks.iter().filter(|c| c.verdict != Verdict::Skip).count()).map(Stop::Check));
        }
        v.push(Stop::NewComment);
        for (i, th) in p.threads.iter().enumerate() {
            v.extend((0..=th.replies.len()).map(|k| Stop::Comment(i, k)));
        }
        v.push(Stop::Body);
        v
    }

    pub fn draw(&mut self, f: &mut Frame, area: Rect, focused: bool) {
        self.area = area;
        self.hits.clear();
        self.tab_hits.clear();
        self.sub_hits.clear();
        self.menu_hits.clear();
        self.edit_rect = Rect::default();
        if area.height < 4 || area.width < 20 {
            return;
        }
        if self.prs.is_empty() {
            let msg = if self.assoc_busy || (self.sources.is_some() && self.assoc_at.is_none()) {
                "  reading the pull requests of this thread…".to_string()
            } else if let Some(e) = &self.assoc_error {
                format!("  pull requests: {e}")
            } else {
                "  0 pull requests: 0 PR of the thread's branches, 0 PR link in the thread. r: read again".to_string()
            };
            f.render_widget(Paragraph::new(Line::styled(msg, theme::muted())), area);
            return;
        }
        let focus = self.tab_ref().and_then(|t| t.focus);
        let on = |s: Stop| focused && focus == Some(s);
        // row 0: the PR tabs; row 1: the views
        let mut x = area.x + 1;
        let mut spans = Vec::new();
        for (i, pr) in self.prs.iter().enumerate() {
            let label = format!(" {} ", pr.label());
            let w = label.width() as u16;
            let st = if i == self.cur {
                if on(Stop::Tabs) { Style::default().bg(theme::pal().accent).fg(theme::pal().on_accent).add_modifier(Modifier::BOLD) } else { theme::pill() }
            } else {
                theme::muted()
            };
            self.tab_hits.push((Rect { x, y: area.y, width: w, height: 1 }, i));
            spans.push(Span::styled(label, st));
            spans.push(Span::raw(" "));
            x += w + 1;
        }
        let hint = if self.prs.len() > 1 { "  [ ]: PR" } else { "" };
        spans.push(Span::styled(hint, theme::muted()));
        f.render_widget(Paragraph::new(Line::from(spans)), Rect { height: 1, ..area });
        let sub = self.tab_ref().map(|t| t.sub).unwrap_or_default();
        let mut x = area.x + 1;
        let mut spans = Vec::new();
        for (s, label) in [(Sub::Overview, " overview "), (Sub::Changes, " changes ")] {
            let w = label.width() as u16;
            let st = if s == sub {
                if on(Stop::Subs) { Style::default().bg(theme::pal().accent).fg(theme::pal().on_accent).add_modifier(Modifier::BOLD) } else { Style::default().fg(theme::pal().accent).add_modifier(Modifier::BOLD | Modifier::UNDERLINED) }
            } else {
                theme::muted()
            };
            self.sub_hits.push((Rect { x, y: area.y + 1, width: w, height: 1 }, s));
            spans.push(Span::styled(label, st));
            x += w + 1;
        }
        let busy = self.tab_ref().map(|t| (t.reading, t.busy.is_some(), t.read_at)).unwrap_or_default();
        let state = if busy.1 {
            "  · gh …".to_string()
        } else if self.active.elapsed() > SLEEP {
            "  · asleep: a key wakes it".to_string()
        } else if busy.0 {
            "  · reading…".to_string()
        } else {
            busy.2.map(|t| format!("  · read {}s ago · r: again", t.elapsed().as_secs())).unwrap_or_default()
        };
        spans.push(Span::styled(state, theme::muted()));
        spans.push(Span::styled("  < >: view", theme::muted()));
        f.render_widget(Paragraph::new(Line::from(spans)), Rect { y: area.y + 1, height: 1, ..area });
        let body = Rect { y: area.y + 3, height: area.height.saturating_sub(3), ..area };
        self.body_area = body;
        if sub == Sub::Changes {
            if let Some(c) = self.changes_view() {
                c.draw(f, body);
                if let Some(n) = c.take_note() {
                    self.note = Some(n);
                }
            }
            return;
        }
        let Some(t) = self.tab_ref() else { return };
        let Some(p) = t.pr.clone() else {
            let msg = match &t.error {
                Some(e) => format!("  {e}"),
                None => "  reading the pull request…".into(),
            };
            f.render_widget(Paragraph::new(Line::styled(msg, theme::muted())), body);
            return;
        };
        let inner = Rect { x: body.x + 1, width: body.width.saturating_sub(2), ..body };
        let rows = self.rows(&p, inner.width as usize, focused);
        // the focused stop in view (an input: all of it)
        let fi = rows.iter().position(|r| r.stops.iter().any(|s| Some(s.2) == focus) || r.input > 0 && self.tab_ref().is_some_and(|t| t.edit.is_some()) && r.stops.is_empty() && false);
        let h = inner.height as usize;
        let scroll = {
            let t = self.tab().expect("a tab");
            if let Some(i) = fi {
                let span = rows.iter().skip(i + 1).take_while(|r| r.input > 0).map(|r| r.input as usize).sum::<usize>() + 1;
                if i < t.scroll {
                    t.scroll = i.saturating_sub(1);
                } else if i + span > t.scroll + h {
                    t.scroll = (i + span).saturating_sub(h);
                }
            }
            let total: usize = rows.iter().map(|r| (r.input as usize).max(1)).sum();
            t.scroll = t.scroll.min(total.saturating_sub(h));
            t.scroll
        };
        let mut y = inner.y;
        let mut input_at: Option<Rect> = None;
        for r in rows.iter().skip(scroll) {
            if y >= inner.bottom() {
                break;
            }
            if r.input > 0 {
                let hgt = r.input.min(inner.bottom() - y);
                input_at = Some(Rect { x: inner.x, y, width: inner.width, height: hgt });
                y += hgt;
                continue;
            }
            let rect = Rect { y, height: 1, ..inner };
            f.render_widget(Paragraph::new(r.line.clone()), rect);
            for &(x0, w, s) in &r.stops {
                self.hits.push((Rect { x: inner.x + x0, y, width: w.min(inner.width.saturating_sub(x0)), height: 1 }, s));
            }
            y += 1;
        }
        // the inline input
        if let Some(r) = input_at
            && let Some((e, ta)) = self.tab().and_then(|t| t.edit.as_mut())
        {
            let label = match e {
                Edit::Title => " title · enter: save · esc: drop ",
                Edit::Body => " description · ctrl+⏎: save · esc: drop ",
                Edit::Comment => " new comment · ctrl+⏎: post · esc: drop ",
                Edit::Reply(..) => " reply · ctrl+⏎: post · esc: drop ",
                Edit::Change(..) => " edit comment · ctrl+⏎: save · esc: drop ",
            };
            let b = Block::new().borders(Borders::ALL).border_style(theme::accent()).title(Span::styled(label, theme::accent()));
            let inside = b.inner(r);
            f.render_widget(b, r);
            ta.render(f, inside, focused);
            self.edit_rect = inside;
        }
        self.draw_menu(f, focused);
    }

    /// The overview's rows of P in W columns.
    fn rows(&self, p: &Pr, w: usize, focused: bool) -> Vec<Row> {
        let t = self.tab_ref();
        let focus = if focused { t.and_then(|t| t.focus) } else { None };
        let edit = t.and_then(|t| t.edit.as_ref());
        let input_rows = |ta: &TextArea, single: bool| -> u16 { if single { 3 } else { (ta.rows_for(w.saturating_sub(2) as u16) as u16).clamp(3, 14) + 2 } };
        let hl = |s: Stop, st: Style| if focus == Some(s) { st.bg(theme::pal().hover).add_modifier(Modifier::BOLD) } else { st };
        let mut rows: Vec<Row> = Vec::new();
        // hero: the title
        match edit {
            Some((Edit::Title, ta)) => rows.push(Row { line: Line::raw(""), stops: Vec::new(), input: input_rows(ta, true) }),
            _ => {
                let title = format!("{} #{}", p.title, p.pr.number);
                let st = hl(Stop::Title, Style::default().fg(theme::pal().text).add_modifier(Modifier::BOLD | Modifier::UNDERLINED));
                rows.push(Row::stop(Line::from(Span::styled(crate::nav::fit(&title, w), st)), Stop::Title));
            }
        }
        // the status (a dropdown), the author, the times
        let (sc, sl) = match p.state {
            State::Open => (theme::pal().ok, "● open"),
            State::Draft => (theme::pal().muted, "◌ draft"),
            State::Merged => (Color::Rgb(163, 113, 247), "◆ merged"),
            State::Closed => (theme::pal().danger, "✕ closed"),
        };
        let status = format!(" {sl} ▾ ");
        let sw = status.width() as u16;
        let st = Style::default().bg(sc).fg(theme::pal().on_accent).add_modifier(Modifier::BOLD);
        let st = if focus == Some(Stop::Status) { st.add_modifier(Modifier::UNDERLINED | Modifier::REVERSED) } else { st };
        let mut meta = format!("  {} · opened {}", p.author, gh::stamp(p.created));
        if let Some(m) = p.merged {
            meta.push_str(&format!(" · merged {}", gh::stamp(m)));
        } else if let (Some(c), State::Closed) = (p.closed, p.state) {
            meta.push_str(&format!(" · closed {}", gh::stamp(c)));
        }
        rows.push(Row { line: Line::from(vec![Span::styled(status, st), Span::styled(meta, theme::muted())]), stops: vec![(0, sw, Stop::Status)], input: 0 });
        // the branches, the size
        rows.push(Row::new(Line::from(vec![
            Span::styled(p.base.clone(), Style::default().fg(theme::pal().accent)),
            Span::styled(" ← ", theme::muted()),
            Span::styled(p.head.clone(), Style::default().fg(theme::pal().accent)),
            Span::styled("  ", theme::muted()),
            Span::styled(format!("+{}", p.additions), Style::default().fg(theme::pal().ok)),
            Span::raw(" "),
            Span::styled(format!("−{}", p.deletions), Style::default().fg(theme::pal().danger)),
            Span::styled(format!(" · {} files · {} commits", p.files, p.commits.len()), theme::muted()),
        ])));
        // the labels: x removes, + adds
        let mut spans = Vec::new();
        let mut stops = Vec::new();
        let mut x = 0u16;
        for (i, l) in p.labels.iter().enumerate() {
            let bg = hex(&l.color);
            let text = format!(" {} × ", l.name);
            let tw = text.width() as u16;
            let mut st = Style::default().bg(bg).fg(on(bg));
            if focus == Some(Stop::Label(i)) {
                st = st.add_modifier(Modifier::REVERSED | Modifier::BOLD);
            }
            spans.push(Span::styled(text, st));
            spans.push(Span::raw(" "));
            stops.push((x, tw, Stop::Label(i)));
            x += tw + 1;
        }
        if p.can_edit {
            let st = hl(Stop::AddLabel, theme::muted());
            spans.push(Span::styled(" + label ", st));
            stops.push((x, 9, Stop::AddLabel));
        } else if p.labels.is_empty() {
            spans.push(Span::styled("0 labels", theme::muted()));
        }
        rows.push(Row { line: Line::from(spans), stops, input: 0 });
        rows.push(Row::new(Line::raw("")));
        // actions: the readiness banner, update, merge
        let ready = p.ready();
        let (title, detail) = ready.text();
        let (icon, color) = match ready {
            Ready::Mergeable => ("✓", theme::pal().ok),
            Ready::Merged => ("◆", Color::Rgb(163, 113, 247)),
            Ready::Conflicts | Ready::ChecksFailed | Ready::ChangesRequested | Ready::Closed => ("✕", theme::pal().danger),
            Ready::ChecksRunning | Ready::Unknown => ("◷", theme::pal().warn),
            _ => ("!", theme::pal().warn),
        };
        let edge = Style::default().fg(color);
        rows.push(Row::new(Line::styled(format!("╭{}╮", "─".repeat(w.saturating_sub(2))), edge)));
        let mut buttons: Vec<(String, Stop, Style)> = Vec::new();
        if p.needs_update() {
            buttons.push((if p.conflicts() { " resolve conflicts ".into() } else { " update branch ".into() }, Stop::Update, Style::default().bg(theme::pal().panel).fg(theme::pal().text)));
        }
        if matches!(p.state, State::Open | State::Draft) {
            let (label, st) = match (&p.auto_merge, ready) {
                (Some(m), _) => (format!(" cancel auto-merge ({}) ", m.to_lowercase()), Style::default().bg(theme::pal().panel).fg(theme::pal().text)),
                (None, Ready::Mergeable) => (format!(" merge · {} ", p.method.to_lowercase()), Style::default().bg(theme::pal().ok).fg(theme::pal().on_accent).add_modifier(Modifier::BOLD)),
                (None, Ready::Draft) => (" merge when ready ".into(), Style::default().bg(theme::pal().panel).fg(theme::pal().muted)),
                (None, _) => (format!(" merge when ready · {} ", p.method.to_lowercase()), Style::default().bg(theme::pal().ok).fg(theme::pal().on_accent)),
            };
            buttons.push((label, Stop::Merge, st));
        }
        let bw: usize = buttons.iter().map(|b| b.0.width() + 1).sum();
        let head = format!("{icon} {title}");
        let left_w = w.saturating_sub(bw + 4);
        let mut spans = vec![Span::styled("│ ", edge), Span::styled(crate::nav::fit(&head, left_w), Style::default().fg(color).add_modifier(Modifier::BOLD))];
        let used = 2 + crate::nav::fit(&head, left_w).width();
        spans.push(Span::raw(" ".repeat(w.saturating_sub(used + bw + 1))));
        let mut stops = Vec::new();
        let mut x = w.saturating_sub(bw + 1) as u16;
        for (label, s, st) in buttons {
            let st = if focus == Some(s) { st.add_modifier(Modifier::REVERSED | Modifier::BOLD) } else { st };
            let lw = label.width() as u16;
            stops.push((x, lw, s));
            spans.push(Span::styled(label, st));
            spans.push(Span::raw(" "));
            x += lw + 1;
        }
        spans.push(Span::styled("│", edge));
        rows.push(Row { line: Line::from(spans), stops, input: 0 });
        let armed = t.and_then(|t| t.armed_merge).is_some_and(|a| a.elapsed() < ARM);
        let mut detail = detail.to_string();
        if armed {
            detail = "enter again: merge".into();
        } else if let Some(m) = &p.auto_merge {
            detail = format!("{detail} · auto-merge on ({})", m.to_lowercase());
        }
        let reviewers: Vec<String> = p.reviewers.iter().map(|(l, s)| format!("{l} {}", if s == "APPROVED" { "✓" } else if s == "CHANGES_REQUESTED" { "✕" } else { "·" })).chain(p.requested.iter().map(|r| format!("{r} ◷"))).collect();
        if !reviewers.is_empty() {
            detail = format!("{detail} · reviews: {}", reviewers.join(", "));
        }
        let d = crate::nav::fit(&format!("  {detail}"), w.saturating_sub(4));
        let pad = w.saturating_sub(d.width() + 4);
        rows.push(Row::new(Line::from(vec![Span::styled("│ ", edge), Span::styled(d, theme::muted()), Span::raw(" ".repeat(pad)), Span::styled(" │", edge)])));
        rows.push(Row::new(Line::styled(format!("╰{}╯", "─".repeat(w.saturating_sub(2))), edge)));
        // checks
        let (fail, pass, skip, pending) = p.check_counts();
        let open = t.is_some_and(|t| t.checks_open);
        let mut parts = Vec::new();
        if fail > 0 {
            parts.push(Span::styled(format!("{fail} failed"), Style::default().fg(theme::pal().danger).add_modifier(Modifier::BOLD)));
        }
        if pending > 0 {
            parts.push(Span::styled(format!("{pending} running"), Style::default().fg(theme::pal().warn)));
        }
        parts.push(Span::styled(format!("{pass} passed"), Style::default().fg(theme::pal().ok)));
        let mut spans = vec![Span::styled(format!("{} checks  ", if open { "▾" } else { "▸" }), hl(Stop::Checks, theme::bold()))];
        for (i, s) in parts.into_iter().enumerate() {
            if i > 0 {
                spans.push(Span::styled(" · ", theme::muted()));
            }
            spans.push(s);
        }
        spans.push(Span::styled(if open { "   ←: hide" } else { "   →: show" }, theme::muted()));
        let cw = Line::from(spans.clone()).width() as u16;
        rows.push(Row { line: Line::from(spans), stops: vec![(0, cw, Stop::Checks)], input: 0 });
        if skip > 0 {
            rows.push(Row::new(Line::styled(format!("  {skip} skipped"), theme::muted())));
        }
        if open {
            let shown: Vec<&gh::Check> = p.checks.iter().filter(|c| c.verdict != Verdict::Skip).collect();
            let mut last: Option<Verdict> = None;
            for (i, c) in shown.iter().enumerate() {
                if last != Some(c.verdict) {
                    let n = shown.iter().filter(|x| x.verdict == c.verdict).count();
                    let label = match c.verdict {
                        Verdict::Fail => format!("  {n} failed"),
                        Verdict::Pending => format!("  {n} running"),
                        _ => format!("  {n} successful"),
                    };
                    rows.push(Row::new(Line::styled(label, theme::muted())));
                    last = Some(c.verdict);
                }
                let (mark, mc) = match c.verdict {
                    Verdict::Fail => ("✕", theme::pal().danger),
                    Verdict::Pending => ("◷", theme::pal().warn),
                    _ => ("✓", theme::pal().ok),
                };
                let s = Stop::Check(i);
                let detail = match c.verdict {
                    Verdict::Fail if c.secs > 0 => format!("failed after {}", secs(c.secs)),
                    Verdict::Fail => "failed".into(),
                    Verdict::Pending => "running".into(),
                    _ if c.secs > 0 => format!("successful in {}", secs(c.secs)),
                    _ if !c.description.is_empty() => c.description.clone(),
                    _ => "successful".into(),
                };
                let req = if c.required { "  required" } else { "" };
                let name = check_name(c);
                let room = w.saturating_sub(detail.width() + req.len() + 8);
                let line = Line::from(vec![
                    Span::styled(format!("    {mark} "), Style::default().fg(mc)),
                    Span::styled(crate::nav::fit(&name, room), hl(s, theme::text())),
                    Span::styled(format!("  {detail}"), theme::muted()),
                    Span::styled(req, Style::default().fg(theme::pal().accent)),
                ]);
                let lw = line.width() as u16;
                rows.push(Row { line, stops: vec![(0, lw, s)], input: 0 });
            }
            if !shown.is_empty() {
                rows.push(Row::new(Line::styled("    ctrl+⏎ / ctrl+click: open the check on GitHub", theme::muted())));
            }
        }
        rows.push(Row::new(Line::raw("")));
        // comments: latest first, replies in the box
        rows.push(Row::new(Line::styled(format!("COMMENTS · {}", p.threads.len()), theme::bold())));
        match edit {
            Some((Edit::Comment, ta)) => {
                rows.push(Row::stop(Line::styled("  + write a comment", hl(Stop::NewComment, theme::accent())), Stop::NewComment));
                rows.push(Row { line: Line::raw(""), stops: Vec::new(), input: input_rows(ta, false) });
            }
            _ => rows.push(Row::stop(Line::styled("  + write a comment", hl(Stop::NewComment, theme::muted())), Stop::NewComment)),
        }
        for (i, th) in p.threads.iter().enumerate() {
            let line_c = Style::default().fg(theme::pal().line);
            let on_line = th.path.as_ref().map(|pa| format!(" · {pa}:{}", th.line.unwrap_or(0))).unwrap_or_default();
            let state = if th.resolved { " · resolved" } else if th.outdated { " · outdated" } else { "" };
            for (k, c) in std::iter::once(&th.root).chain(&th.replies).enumerate() {
                let s = Stop::Comment(i, k);
                let verdict = match c.verdict.as_str() {
                    "APPROVED" => " · approved",
                    "CHANGES_REQUESTED" => " · requested changes",
                    _ => "",
                };
                let who = if c.mine { format!("{} (you)", c.author) } else { c.author.clone() };
                let head = if k == 0 { format!("{who} · {}{verdict}{on_line}{state}", gh::stamp(c.created)) } else { format!("↳ {who} · {}", gh::stamp(c.created)) };
                let (lead, indent) = if k == 0 { ("╭─ ", "│ ") } else { ("│  ", "│    ") };
                let hint = if focus == Some(s) {
                    if c.mine { "  enter: edit / delete" } else if c.kind == Kind::Line { "  enter: reply" } else { "  enter: quote in a reply" }
                } else {
                    ""
                };
                let name_st = hl(s, Style::default().fg(if c.mine { theme::pal().accent } else { theme::pal().text }).add_modifier(Modifier::BOLD));
                let line = Line::from(vec![Span::styled(lead, line_c), Span::styled(crate::nav::fit(&head, w.saturating_sub(lead.width() + hint.width())), name_st), Span::styled(hint, theme::muted())]);
                let lw = line.width() as u16;
                rows.push(Row { line, stops: vec![(0, lw, s)], input: 0 });
                let editing = matches!(edit, Some((Edit::Change(ti, tk, ..), _)) if *ti == i && *tk == k);
                if editing {
                    if let Some((_, ta)) = edit {
                        rows.push(Row { line: Line::raw(""), stops: Vec::new(), input: input_rows(ta, false) });
                    }
                    continue;
                }
                let prefix = [Span::styled(indent, line_c)];
                for r in crate::render::markdown(&c.body, w.saturating_sub(indent.width() + 1), &prefix) {
                    rows.push(Row::new(r.line));
                }
            }
            if let Some((Edit::Reply(ti, _), ta)) = edit
                && *ti == i
            {
                rows.push(Row { line: Line::raw(""), stops: Vec::new(), input: input_rows(ta, false) });
            }
            rows.push(Row::new(Line::styled(format!("╰{}", "─".repeat(w.saturating_sub(1).min(40))), line_c)));
        }
        rows.push(Row::new(Line::raw("")));
        // the description
        rows.push(Row::stop(Line::styled(if focus == Some(Stop::Body) { "DESCRIPTION  enter: edit" } else { "DESCRIPTION" }, hl(Stop::Body, theme::bold())), Stop::Body));
        match edit {
            Some((Edit::Body, ta)) => rows.push(Row { line: Line::raw(""), stops: Vec::new(), input: input_rows(ta, false) }),
            _ if p.body.trim().is_empty() => rows.push(Row::stop(Line::styled("  0 description", theme::muted()), Stop::Body)),
            _ => {
                for r in crate::render::markdown(&p.body, w.saturating_sub(2), &[Span::raw("  ")]) {
                    let lw = r.line.width() as u16;
                    rows.push(Row { line: r.line, stops: vec![(0, lw, Stop::Body)], input: 0 });
                }
            }
        }
        rows
    }

    /// The open dropdown (status, own comment, labels) under its stop.
    fn draw_menu(&mut self, f: &mut Frame, focused: bool) {
        let Some(t) = self.tab_ref() else { return };
        let Some(m) = &t.menu else { return };
        let anchor_stop = match m {
            Menu::Status(..) => Stop::Status,
            Menu::Mine { thread, k, .. } => Stop::Comment(*thread, *k),
            Menu::Labels { .. } => Stop::AddLabel,
        };
        let anchor = self.hits.iter().find(|(_, s)| *s == anchor_stop).map(|(r, _)| *r).unwrap_or(Rect { x: self.body_area.x, y: self.body_area.y, width: 1, height: 1 });
        let (label, rows, cur, note): (String, Vec<crate::dropdown::Row>, usize, String) = match m {
            Menu::Status(c, o) => ("status".into(), o.iter().map(|(l, _)| crate::dropdown::Row { text: l.to_string(), ..Default::default() }).collect(), *c, String::new()),
            Menu::Mine { cur, armed, .. } => (
                "your comment".into(),
                vec![crate::dropdown::Row { text: "edit".into(), ..Default::default() }, crate::dropdown::Row { text: if *armed { "delete: enter again".into() } else { "delete".into() }, ..Default::default() }],
                *cur,
                String::new(),
            ),
            Menu::Labels { cur, filter } => {
                let q = filter.text();
                let opts = self.label_opts(&q);
                let loading = t.pr.as_ref().is_some_and(|p| !self.labels.contains_key(&p.pr.repo));
                let rows = opts
                    .iter()
                    .map(|l| {
                        let marks = q.split_whitespace().filter_map(|w| crate::fuzzy::word(w, &l.name)).flat_map(|(_, m)| m).collect();
                        crate::dropdown::Row { text: format!("● {}", l.name), marks: shift(marks, 2), ..Default::default() }
                    })
                    .collect();
                (format!("labels: {}", if q.is_empty() { "type to filter".to_string() } else { q }), rows, *cur, if loading { "reading…".into() } else { String::new() })
            }
        };
        let w = rows.iter().map(|r| r.text.width() + 6).max().unwrap_or(20).clamp(28, 60) as u16;
        let Some(r) = crate::dropdown::place(Rect { y: anchor.y, height: 1, ..anchor }, w, rows.len(), self.area, false) else { return };
        let (_, hits) = crate::dropdown::list(f, r, &label, &note, &rows, cur, 0, if matches!(m, Menu::Labels { .. }) { "0 labels" } else { "0 options" });
        let _ = focused;
        self.menu_hits = hits;
    }

    /// The changes view of the tab in view (PR mode), for the host's hints.
    pub fn hints(&self) -> String {
        let Some(t) = self.tab_ref() else { return "r: read again · i: prompt ".into() };
        if t.sub == Sub::Changes {
            return t.changes.as_ref().map(|c| c.hints().to_string()).unwrap_or_default();
        }
        if t.edit.is_some() {
            return "ctrl+⏎ save · esc drop ".into();
        }
        if t.menu.is_some() {
            return "↑↓ pick · ⏎ take · esc close ".into();
        }
        "j k ↑↓ move · ←→ along · ⏎ act · ctrl+⏎ GitHub · [ ] PR · < > view · r read · i prompt ".into()
    }
}

fn shift(m: Vec<usize>, by: usize) -> Vec<usize> {
    m.into_iter().map(|x| x + by).collect()
}

/// `CI / lint` of check C.
fn check_name(c: &gh::Check) -> String {
    if c.group.is_empty() || c.group == "GitHub Actions" { c.name.clone() } else { format!("{} / {}", c.group, c.name) }
}

/// The PRs of sources S: each checkout's branch (as head), then the log's links; 0 dupes.
fn associate(s: &Sources) -> Result<Vec<PrRef>, String> {
    let mut out: Vec<PrRef> = Vec::new();
    let mut err = None;
    for (dir, stored) in &s.checkouts {
        let Some(repo) = gh::repo_of_dir(dir) else { continue };
        let git = |args: &[&str]| std::process::Command::new("git").arg("-C").arg(dir).args(args).stdin(std::process::Stdio::null()).stderr(std::process::Stdio::null()).output().ok().filter(|o| o.status.success()).map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());
        let branch = git(&["rev-parse", "--abbrev-ref", "HEAD"]).filter(|b| b != "HEAD").or_else(|| stored.clone());
        let Some(branch) = branch else { continue };
        // the default branch: its PRs are other people's
        let default = git(&["symbolic-ref", "--short", "refs/remotes/origin/HEAD"]).map(|d| d.trim_start_matches("origin/").to_string());
        if default.as_deref() == Some(branch.as_str()) || matches!(branch.as_str(), "main" | "master") {
            continue;
        }
        match gh::for_branch(&repo, &branch) {
            Ok(ns) => {
                for n in ns {
                    let r = PrRef::new(&repo, n);
                    if !out.contains(&r) {
                        out.push(r);
                    }
                }
            }
            Err(e) => err = Some(e),
        }
    }
    for l in &s.links {
        if !out.contains(l) {
            out.push(l.clone());
        }
    }
    match err {
        Some(e) if out.is_empty() => Err(e),
        _ => Ok(out),
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    pub fn pr() -> Pr {
        Pr {
            pr: PrRef::new("acme/platform", 12),
            title: "Add X".into(),
            body: "the body".into(),
            url: "https://github.com/acme/platform/pull/12".into(),
            author: "me".into(),
            viewer: "me".into(),
            base: "main".into(),
            head: "feat/x".into(),
            mergeable: "MERGEABLE".into(),
            merge_state: "BEHIND".into(),
            review: "APPROVED".into(),
            can_edit: true,
            method: "SQUASH".into(),
            labels: vec![gh::Label { name: "bug".into(), color: "d73a4a".into() }],
            checks: vec![
                gh::Check { name: "lint".into(), group: "CI".into(), verdict: Verdict::Fail, url: "u1".into(), required: true, secs: 12, description: String::new() },
                gh::Check { name: "test".into(), group: "CI".into(), verdict: Verdict::Pass, url: "u2".into(), required: false, secs: 27, description: String::new() },
                gh::Check { name: "deploy".into(), group: "CD".into(), verdict: Verdict::Skip, url: "u3".into(), required: false, secs: 0, description: String::new() },
            ],
            threads: vec![
                gh::Thread { root: gh::Comment { kind: Kind::Issue, id: 1, author: "rev".into(), body: "looks off".into(), mine: false, ..Default::default() }, ..Default::default() },
                gh::Thread {
                    root: gh::Comment { kind: Kind::Line, id: 2, author: "rev".into(), body: "why?".into(), ..Default::default() },
                    replies: vec![gh::Comment { kind: Kind::Line, id: 3, author: "me".into(), body: "because".into(), mine: true, ..Default::default() }],
                    path: Some("src/x.rs".into()),
                    line: Some(4),
                    ..Default::default()
                },
            ],
            ..Default::default()
        }
    }

    pub fn with(p: Pr) -> Merge {
        let mut m = Merge::one(p.pr.clone());
        let key = p.pr.clone();
        m.tabs.entry(key).or_default().pr = Some(p);
        m
    }

    fn key(m: &mut Merge, c: KeyCode) -> Act {
        m.key(KeyEvent::new(c, KeyModifiers::NONE))
    }

    fn focus(m: &Merge) -> Option<Stop> {
        m.tab_ref().and_then(|t| t.focus)
    }

    fn draw(m: &mut Merge, w: u16, h: u16) -> String {
        let backend = ratatui::backend::TestBackend::new(w, h);
        let mut term = ratatui::Terminal::new(backend).unwrap();
        term.draw(|f| m.draw(f, f.area(), true)).unwrap();
        let buf = term.backend().buffer().clone();
        (0..h).map(|y| (0..w).map(|x| buf[(x, y)].symbol().to_string()).collect::<String>()).collect::<Vec<_>>().join("\n")
    }

    #[test]
    fn the_keys_walk_the_stops_not_the_lines() {
        let mut m = with(pr());
        // the 1st key: the title
        key(&mut m, KeyCode::Down);
        assert_eq!(focus(&m), Some(Stop::Status));
        key(&mut m, KeyCode::Down);
        // the labels row: its 1st stop; → along it
        assert_eq!(focus(&m), Some(Stop::Label(0)));
        key(&mut m, KeyCode::Right);
        assert_eq!(focus(&m), Some(Stop::AddLabel));
        // ↓ leaves the row: update + merge share 1
        key(&mut m, KeyCode::Down);
        assert_eq!(focus(&m), Some(Stop::Update));
        key(&mut m, KeyCode::Right);
        assert_eq!(focus(&m), Some(Stop::Merge));
        key(&mut m, KeyCode::Down);
        assert_eq!(focus(&m), Some(Stop::Checks));
        // → shows the checks: failed + passed (skipped: 1 gray line), ← hides them
        key(&mut m, KeyCode::Right);
        key(&mut m, KeyCode::Down);
        assert_eq!(focus(&m), Some(Stop::Check(0)));
        key(&mut m, KeyCode::Down);
        assert_eq!(focus(&m), Some(Stop::Check(1)));
        key(&mut m, KeyCode::Down);
        assert_eq!(focus(&m), Some(Stop::NewComment));
        key(&mut m, KeyCode::Up);
        key(&mut m, KeyCode::Left);
        assert_eq!(focus(&m), Some(Stop::Checks));
        assert!(!m.tab_ref().unwrap().checks_open);
        // up into a row: its 1st stop
        key(&mut m, KeyCode::Up);
        assert_eq!(focus(&m), Some(Stop::Update));
    }

    #[test]
    fn enter_on_comments_edits_own_replies_to_line_threads_quotes_others() {
        let mut m = with(pr());
        m.tab().unwrap().focus = Some(Stop::Comment(0, 0));
        key(&mut m, KeyCode::Enter);
        // someone's flat comment: a new comment that quotes it
        let t = m.tab_ref().unwrap();
        assert!(matches!(t.edit, Some((Edit::Comment, ref ta)) if ta.text().starts_with("@rev wrote:\n> looks off")));
        key(&mut m, KeyCode::Esc);
        // a line thread: a real reply
        m.tab().unwrap().focus = Some(Stop::Comment(1, 0));
        key(&mut m, KeyCode::Enter);
        assert!(matches!(m.tab_ref().unwrap().edit, Some((Edit::Reply(1, 2), _))));
        key(&mut m, KeyCode::Esc);
        // own reply: edit / delete
        m.tab().unwrap().focus = Some(Stop::Comment(1, 1));
        key(&mut m, KeyCode::Enter);
        assert!(matches!(m.tab_ref().unwrap().menu, Some(Menu::Mine { thread: 1, k: 1, .. })));
        key(&mut m, KeyCode::Enter);
        assert!(matches!(m.tab_ref().unwrap().edit, Some((Edit::Change(1, 1, Kind::Line, 3), ref ta)) if ta.text() == "because"));
    }

    #[test]
    fn merge_asks_twice_update_prompts_the_agent() {
        let mut m = with(pr());
        m.tab().unwrap().focus = Some(Stop::Merge);
        let a = key(&mut m, KeyCode::Enter);
        assert!(matches!(a, Act::Toast(ref t) if t.contains("enter again: merge when ready (squash)")), "{a:?}");
        m.tab().unwrap().focus = Some(Stop::Update);
        let a = key(&mut m, KeyCode::Enter);
        assert!(matches!(a, Act::Prompt(ref t) if t.contains("Update the branch `feat/x`") && t.contains("0 conflicts expected")), "{a:?}");
        let mut p = pr();
        p.mergeable = "CONFLICTING".into();
        let mut m = with(p);
        m.tab().unwrap().focus = Some(Stop::Update);
        let a = key(&mut m, KeyCode::Enter);
        assert!(matches!(a, Act::Prompt(ref t) if t.contains("resolve every conflict")), "{a:?}");
    }

    #[test]
    fn the_title_edits_in_place() {
        let mut m = with(pr());
        m.tab().unwrap().focus = Some(Stop::Title);
        key(&mut m, KeyCode::Enter);
        assert!(m.typing());
        for c in " now".chars() {
            key(&mut m, KeyCode::Char(c));
        }
        key(&mut m, KeyCode::Enter);
        // saved: on screen at once (the gh call fails in tests: a note)
        assert_eq!(m.tab_ref().unwrap().pr.as_ref().unwrap().title, "Add X now");
        assert!(!m.typing());
    }

    #[test]
    fn the_overview_draws() {
        let mut m = with(pr());
        m.tab().unwrap().checks_open = true;
        let s = draw(&mut m, 100, 60);
        for want in ["platform#12", "overview", "changes", "Add X #12", "open ▾", "main ← feat/x", " bug × ", "+ label", "checks failed", "update branch", "merge when ready · squash", "1 failed", "1 passed", "1 skipped", "CI / lint", "required", "COMMENTS · 2", "+ write a comment", "rev ·", "↳ me (you)", "because", "DESCRIPTION", "the body"] {
            assert!(s.contains(want), "{want} missing:\n{s}");
        }
        // the skipped check: only counted
        assert!(!s.contains("CD / deploy"));
    }

    #[test]
    fn a_label_click_removes_the_plus_opens_the_dropdown() {
        let mut m = with(pr());
        draw(&mut m, 100, 60);
        let plus = m.hits.iter().find(|(_, s)| *s == Stop::AddLabel).map(|(r, _)| *r).unwrap();
        m.mouse(MouseEvent { kind: MouseEventKind::Down(MouseButton::Left), column: plus.x, row: plus.y, modifiers: KeyModifiers::NONE });
        assert!(matches!(m.tab_ref().unwrap().menu, Some(Menu::Labels { .. })));
        // the dropdown filters as it is typed
        m.labels.insert("acme/platform".into(), vec![gh::Label { name: "ui".into(), color: "00ff00".into() }, gh::Label { name: "backend".into(), color: "0000ff".into() }, gh::Label { name: "bug".into(), color: "".into() }]);
        assert_eq!(m.label_opts("").len(), 2, "the PR's own label: not offered");
        key(&mut m, KeyCode::Char('b'));
        assert_eq!(m.label_opts("b").first().map(|l| l.name.as_str()), Some("backend"));
    }

    /// CODEHEAD_GH_LIVE=1 cargo test live_merge -- --ignored --nocapture: a real PR drawn.
    #[test]
    #[ignore]
    fn live_merge_draws_a_real_pr() {
        let pr = PrRef::new("cli/cli", 15100);
        let mut m = Merge::one(pr.clone());
        m.read(pr);
        m.poll(false);
        let t = m.tab().unwrap();
        assert!(t.pr.is_some(), "{:?}", t.error);
        t.checks_open = true;
        println!("{}", draw(&mut m, 120, 90));
        m.set_sub(Sub::Changes);
        let backend = ratatui::backend::TestBackend::new(140, 40);
        let mut term = ratatui::Terminal::new(backend).unwrap();
        // the diff: read inline (tests), applied by the next draws
        for _ in 0..100 {
            term.draw(|f| m.draw(f, f.area(), true)).unwrap();
            if m.changes_view().is_some_and(|c| !c.changes.files.is_empty()) {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        // a box with a GitHub thread in view
        if let Some(c) = m.changes_view() {
            let i = c.changes.files.iter().position(|f| f.path == "internal/authflow/revoke.go").unwrap_or(0);
            c.jump_to_file(i);
        }
        term.draw(|f| m.draw(f, f.area(), true)).unwrap();
        let buf = term.backend().buffer().clone();
        println!("{}", (0..40).map(|y| (0..140).map(|x| buf[(x, y)].symbol().to_string()).collect::<String>()).collect::<Vec<_>>().join("\n"));
    }

    #[test]
    fn the_context_names_the_pr_and_its_failed_checks() {
        let m = with(pr());
        let c = m.context().unwrap();
        assert!(c.contains("https://github.com/acme/platform/pull/12") && c.contains("CI / lint (u1)") && c.contains("src/x.rs:4 rev: why?"), "{c}");
    }
}
