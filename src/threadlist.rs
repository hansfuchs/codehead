//! The thread list of both threads tabs: the menu's (ctrl+p: a quick switch, type to filter) and
//! the start screen's (manage: preview, multi-select, letter keys). 1 look: thread cards in
//! sections (open · stale · shipped), the repo tabs, the view tabs (active ·
//! archived; the start screen also: worktrees without thread, claude sessions: their rows are the
//! host's), a name filter.
//!
//! Actions on the card under the cursor (the start screen: or the selection): open, archive /
//! unarchive, edit, delete. ctrl+.: their dropdown (both tabs); the start screen also: a u e d.
//! Archive and delete ask first (1 row). Edit: a form (title, repo, linked repos, agents). The
//! jobs run off the UI thread; their result shows on the list's last row.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph};

use crate::nav::{self, Card};
use crate::tabgroup::TabGroup;
use crate::textarea::TextArea;
use crate::theme;

/// Menu (quick switch) or start screen (manage).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Switch,
    Manage,
}

/// The view tabs.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum View {
    Active,
    Archived,
    /// Start screen: worktrees without a thread (the host's rows).
    Worktrees,
    /// Start screen: Claude Code sessions (the host's rows).
    Claude,
}

impl View {
    fn label(self) -> &'static str {
        match self {
            View::Active => "active",
            View::Archived => "archived",
            View::Worktrees => "worktrees",
            View::Claude => "claude",
        }
    }

    /// Thread cards (else the host's rows).
    pub fn cards(self) -> bool {
        matches!(self, View::Active | View::Archived)
    }
}

/// Where the keys go: the repo tabs, the view tabs, the list.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Focus {
    Repos,
    Views,
    List,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Act {
    Open,
    Archive,
    Unarchive,
    Edit,
    Delete,
}

impl Act {
    fn key(self) -> &'static str {
        match self {
            Act::Open => "enter",
            Act::Archive => "a",
            Act::Unarchive => "u",
            Act::Edit => "e",
            Act::Delete => "d",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Act::Open => "open",
            Act::Archive => "archive: worktree goes, branch stays",
            Act::Unarchive => "unarchive",
            Act::Edit => "edit: title, repo, linked repos, agents",
            Act::Delete => "delete: thread + worktree",
        }
    }
}

/// What the host does after a key / click.
#[derive(Clone, Debug, PartialEq)]
pub enum Out {
    /// Done here.
    None,
    /// Not a key of the list: the host's.
    Pass,
    /// ↑ above the repo tabs: the host's tab bar.
    Up,
    Open(Card),
    /// The thread in view got archived / deleted: the host goes to the start screen.
    Home,
}

/// A repo tab.
#[derive(Clone, Debug, PartialEq)]
pub enum RepoTab {
    Repo(PathBuf),
    Outside,
    All,
}

impl RepoTab {
    pub fn keeps(&self, repo: Option<&Path>) -> bool {
        match self {
            RepoTab::Repo(p) => repo == Some(p.as_path()),
            RepoTab::Outside => repo.is_none(),
            RepoTab::All => true,
        }
    }

    fn label(&self) -> String {
        match self {
            RepoTab::Repo(p) => repo_name(p),
            RepoTab::Outside => "outside git".into(),
            RepoTab::All => "all".into(),
        }
    }
}

fn repo_name(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| p.display().to_string())
}

/// Archive / delete wait for a yes.
struct Pending {
    act: Act,
    ids: Vec<String>,
}

/// A list row: a section label or a card (its position in `visible`).
#[derive(Clone, Copy, PartialEq, Debug)]
enum Row {
    Label(u8, usize),
    Card(usize),
}

impl Row {
    fn height(self) -> u16 {
        match self {
            Row::Label(..) => 1,
            Row::Card(_) => nav::CARD_H,
        }
    }
}

/// A job's result shows this long.
const STATUS_FOR: Duration = Duration::from_secs(8);

/// Thread cards, read in the background (also saved: the next list shows them at once).
fn read_cards() -> Receiver<Vec<Card>> {
    let (tx, rx) = crate::bus::channel();
    std::thread::spawn(move || {
        let cards = nav::load();
        crate::background::save("cards", &cards);
        let _ = tx.send(cards);
        crate::bus::wake();
    });
    rx
}

pub struct ThreadList {
    pub mode: Mode,
    pub cards: Vec<Card>,
    /// The thread in view (the menu over a thread): its repo tab first, `here` on its card.
    here: Option<String>,
    /// The repo of the start screen (its tab first; None: `all` first).
    home_repo: Option<PathBuf>,
    loading: Option<Receiver<Vec<Card>>>,
    cards_at: Option<Instant>,
    /// `landed::version` of the cards' lands.
    land_v: u64,
    /// Bus topics: a thread changed → the cards again.
    topics: std::sync::mpsc::Receiver<crate::bus::Topic>,
    cards_due: bool,
    /// Data version of the cards (a new read dissolves in).
    version: u64,
    fx: crate::dissolve::Dissolve,
    pub filter: TextArea,
    /// Manage: the filter takes the keys (ctrl+f; enter / esc: back to the list).
    pub filtering: bool,
    pub cursor: usize,
    /// 1st virtual row in view.
    offset: u16,
    /// Manage: the picked threads (space).
    pub sel: HashSet<String>,
    repos: TabGroup,
    repo_pick: Option<RepoTab>,
    views: TabGroup,
    pub view: View,
    pub focus: Focus,
    /// Manage: rows of the host's views (worktrees, claude): the counts on the view tabs, their
    /// repos in the repo tabs.
    pub extra: [(usize, Vec<Option<PathBuf>>); 2],
    /// The actions dropdown: its cursor.
    actions: Option<usize>,
    pending: Option<Pending>,
    job: Option<Receiver<(String, bool)>>,
    /// Jobs run on their own thread (tests: at once, the perf case: on their thread).
    pub inline: bool,
    /// Cards a job acts on, as the job leaves them (Some: their new state; None: gone): shown at
    /// once, over every read until the 1st read after the job's end.
    moved: HashMap<String, Option<String>>,
    /// A job ended: the next read clears `moved`.
    settle: bool,
    status: Option<(String, bool, Instant)>,
    /// A job ended: the host reads its own rows again (`take_changed`).
    changed: bool,
    form: Option<Form>,
    /// Agent names (the edit form's agents).
    pub roster: Vec<String>,
    /// Last draw: card rows, dropdown rows, the list.
    rows: Vec<(Rect, usize)>,
    act_rows: Vec<(Rect, Act)>,
    pub list: Rect,
}

impl ThreadList {
    /// CARDS: the host's (empty: the last read at once, a new read in the background).
    pub fn new(mode: Mode, here: Option<String>, home_repo: Option<PathBuf>, cards: Vec<Card>) -> ThreadList {
        let loading = (cards.is_empty() && !cfg!(test)).then(read_cards);
        let cards = if cards.is_empty() && !cfg!(test) { crate::background::snapshot("cards").unwrap_or_default() } else { cards };
        let mut filter = TextArea::new(true, if mode == Mode::Switch { "filter threads" } else { "ctrl+f: fuzzy search thread names" });
        filter.style = Some(theme::text());
        ThreadList {
            mode,
            cards,
            here,
            home_repo,
            cards_at: loading.is_some().then(Instant::now),
            land_v: crate::landed::version(),
            topics: crate::bus::subscribe(),
            cards_due: false,
            loading,
            version: 0,
            fx: Default::default(),
            filter,
            filtering: false,
            cursor: 0,
            offset: 0,
            sel: HashSet::new(),
            repos: TabGroup::default(),
            repo_pick: None,
            views: TabGroup::default(),
            view: View::Active,
            focus: Focus::List,
            extra: Default::default(),
            actions: None,
            pending: None,
            job: None,
            inline: cfg!(test),
            moved: HashMap::new(),
            settle: false,
            status: None,
            changed: false,
            form: None,
            roster: Vec::new(),
            rows: Vec::new(),
            act_rows: Vec::new(),
            list: Rect::default(),
        }
    }

    /// A new read now (a job ended, the host's `r`).
    pub fn reload(&mut self) {
        if !cfg!(test) && self.loading.is_none() {
            self.loading = Some(read_cards());
            self.cards_at = Some(Instant::now());
        }
    }

    /// A background read on its way, a job, a status that times out: the host draws often.
    pub fn waiting(&self) -> bool {
        self.loading.is_some() || self.job.is_some() || self.status.is_some()
    }

    /// A job ended since the last call: the host reads its rows again.
    pub fn take_changed(&mut self) -> bool {
        std::mem::take(&mut self.changed)
    }

    /// The list takes esc / enter itself (a form, a question, the dropdown, the filter in edit).
    pub fn modal(&self) -> bool {
        self.form.is_some() || self.pending.is_some() || self.actions.is_some() || self.filtering
    }

    /// Reads, a job's end, the status's end. SHOWN: the list shows (reads every 2s).
    pub fn poll(&mut self, shown: bool) {
        if let Some(rx) = &self.loading
            && let Ok(c) = rx.try_recv()
        {
            self.loading = None;
            // the 1st read after the job's end: the store holds what the job did
            if std::mem::take(&mut self.settle) {
                self.moved.clear();
            }
            self.set_cards(c);
        }
        // a thread's files changed (state, log, draft): the cards again (0 timer)
        for t in self.topics.try_iter() {
            self.cards_due |= matches!(t, crate::bus::Topic::Thread(..) | crate::bus::Topic::Rescan(_));
        }
        let due = self.cards_at.is_none() || self.cards_due;
        if shown && due && self.loading.is_none() && !cfg!(test) {
            self.cards_due = false;
            self.reload();
        }
        // new landed states: on the cards at once (0 wait for the next card read)
        let lv = crate::landed::version();
        if shown && lv != self.land_v {
            self.land_v = lv;
            let mut c = self.cards.clone();
            for k in &mut c {
                k.land = crate::landed::of(&k.id).or(k.land.take());
            }
            self.set_cards(c);
        }
        if shown {
            crate::landed::visible();
        }
        if let Some(rx) = &self.job
            && let Ok((text, err)) = rx.try_recv()
        {
            self.job = None;
            self.done(text, err);
        }
        if self.status.as_ref().is_some_and(|s| s.2.elapsed() > STATUS_FOR) {
            self.status = None;
        }
    }

    fn done(&mut self, text: String, err: bool) {
        self.status = Some((text, err, Instant::now()));
        self.changed = true;
        // a read from before the end: dropped, a new one ends the moved cards
        self.loading = None;
        self.settle = true;
        self.reload();
    }

    /// New cards (a read): the cursor stays on its thread. Moved cards (a job on its way) as
    /// the job leaves them.
    pub fn set_cards(&mut self, mut c: Vec<Card>) {
        if !self.moved.is_empty() {
            c.retain(|k| !matches!(self.moved.get(&k.id), Some(None)));
            for k in &mut c {
                if let Some(Some(s)) = self.moved.get(&k.id) {
                    k.state = s.clone();
                }
            }
        }
        if c == self.cards {
            return;
        }
        let picked = self.current().map(|c| c.id.clone());
        self.cards = c;
        self.version += 1;
        let ids: HashSet<&str> = self.cards.iter().map(|c| c.id.as_str()).collect();
        self.sel.retain(|s| ids.contains(s.as_str()));
        if let Some(id) = picked
            && let Some(p) = self.visible().iter().position(|&i| self.cards[i].id == id)
        {
            self.cursor = p;
        }
    }

    pub fn query(&self) -> String {
        self.filter.text()
    }

    // ---------- tabs ----------

    fn views_all(&self) -> Vec<View> {
        match self.mode {
            Mode::Switch => vec![View::Active, View::Archived],
            Mode::Manage => vec![View::Active, View::Archived, View::Worktrees, View::Claude],
        }
    }

    fn view_label(&self, v: View) -> String {
        let n = match v {
            View::Active => self.cards.iter().filter(|c| !c.archived()).count(),
            View::Archived => self.cards.iter().filter(|c| c.archived()).count(),
            View::Worktrees => self.extra[0].0,
            View::Claude => self.extra[1].0,
        };
        format!("{} {n}", v.label())
    }

    /// The repo tabs: the first repo (the thread in view's, else the start screen's), the other
    /// repos (most threads first, then A→Z), outside of git repos (when a row is), all.
    pub fn repo_tabs(&self) -> Vec<RepoTab> {
        let here = self.here.as_deref().and_then(|id| self.cards.iter().find(|c| c.id == id)).and_then(|c| c.repo.clone()).or_else(|| self.home_repo.clone());
        let mut counts: Vec<(PathBuf, usize)> = Vec::new();
        let mut outside = false;
        let extra = self.extra.iter().flat_map(|e| e.1.iter().cloned());
        for r in self.cards.iter().map(|c| c.repo.clone()).chain(extra) {
            match r {
                Some(p) => match counts.iter_mut().find(|(q, _)| *q == p) {
                    Some(e) => e.1 += 1,
                    None => counts.push((p, 1)),
                },
                None => outside = true,
            }
        }
        counts.sort_by(|a, b| (Some(&b.0) == here.as_ref()).cmp(&(Some(&a.0) == here.as_ref())).then(b.1.cmp(&a.1)).then_with(|| repo_name(&a.0).cmp(&repo_name(&b.0))));
        let mut v: Vec<RepoTab> = counts.into_iter().map(|(p, _)| RepoTab::Repo(p)).collect();
        if outside {
            v.push(RepoTab::Outside);
        }
        v.push(RepoTab::All);
        v
    }

    /// The repo tab in use: the pick, else the thread in view's repo (the menu), else all.
    pub fn repo_tab(&self) -> RepoTab {
        let tabs = self.repo_tabs();
        if let Some(p) = self.repo_pick.as_ref().filter(|p| tabs.contains(p)) {
            return p.clone();
        }
        if self.here.is_some()
            && let Some(t) = tabs.first()
        {
            return t.clone();
        }
        RepoTab::All
    }

    /// Repo tab I's area in the last draw.
    #[cfg(test)]
    pub fn repo_rect(&self, i: usize) -> Option<Rect> {
        self.repos.rect_of(i)
    }

    /// A row of REPO passes the repo tab (the host's rows too).
    pub fn keeps(&self, repo: Option<&Path>) -> bool {
        self.repo_tab().keeps(repo)
    }

    pub fn step_repo(&mut self, d: isize) {
        let tabs = self.repo_tabs();
        let cur = self.repo_tab();
        self.repos.i = tabs.iter().position(|t| *t == cur).unwrap_or(0);
        self.repos.step(d, tabs.len());
        self.repo_pick = tabs.get(self.repos.i).cloned();
        self.cursor = 0;
        self.offset = 0;
    }

    pub fn step_view(&mut self, d: isize) {
        let all = self.views_all();
        self.views.i = all.iter().position(|v| *v == self.view).unwrap_or(0);
        self.views.step(d, all.len());
        self.set_view(all[self.views.i]);
    }

    pub fn set_view(&mut self, v: View) {
        if v != self.view {
            self.view = v;
            self.cursor = 0;
            self.offset = 0;
            self.sel.clear();
        }
    }

    // ---------- rows ----------

    /// Section of card C: 0 open (working, a question, work not in the base yet, 0 code yet, 0
    /// landed state read yet), 1 stale (open, 0 code, idle 3 days), 2 shipped.
    fn section(c: &Card) -> u8 {
        let stale = || c.stale(crate::engine::now_ms());
        if c.busy || c.land.as_ref().is_some_and(|l| l.open()) {
            0
        } else if c.blocked {
            if stale() { 1 } else { 0 }
        } else if c.shipped() {
            2
        } else if stale() {
            1
        } else {
            0
        }
    }

    /// The cards of the view (filter, repo tab applied), by section, then match / activity.
    pub fn visible(&self) -> Vec<usize> {
        if !self.view.cards() {
            return Vec::new();
        }
        let tab = self.repo_tab();
        let archived = self.view == View::Archived;
        let mut v: Vec<usize> = nav::filter(&self.cards, &self.filter.text()).into_iter().filter(|&i| self.cards[i].archived() == archived && tab.keeps(self.cards[i].repo.as_deref())).collect();
        if !archived {
            v.sort_by_key(|&i| Self::section(&self.cards[i]));
        }
        v
    }

    fn layout(&self, vis: &[usize]) -> Vec<Row> {
        let mut rows = Vec::new();
        let mut last = None;
        for (pos, &i) in vis.iter().enumerate() {
            let s = if self.view == View::Archived { 9 } else { Self::section(&self.cards[i]) };
            if last != Some(s) {
                let n = vis.iter().filter(|&&j| if self.view == View::Archived { true } else { Self::section(&self.cards[j]) == s }).count();
                rows.push(Row::Label(s, n));
                last = Some(s);
            }
            rows.push(Row::Card(pos));
        }
        rows
    }

    fn label_text(s: u8, n: usize) -> String {
        match s {
            0 => format!("open · {n}"),
            1 => format!("stale · {n} · 0 code, idle 3+ days: go on or archive"),
            2 => format!("shipped: archive? · {n}"),
            _ => format!("archived · {n} · branches kept, worktrees gone"),
        }
    }

    pub fn current(&self) -> Option<&Card> {
        self.visible().get(self.cursor).map(|&i| &self.cards[i])
    }

    /// The targets of an action: the selection (manage), else the card under the cursor.
    fn targets(&self) -> Vec<Card> {
        if self.mode == Mode::Manage && !self.sel.is_empty() {
            return self.visible().into_iter().map(|i| self.cards[i].clone()).filter(|c| self.sel.contains(&c.id)).collect();
        }
        self.current().cloned().into_iter().collect()
    }

    /// The actions on the targets.
    fn acts(&self) -> Vec<Act> {
        let t = self.targets();
        if t.is_empty() {
            return Vec::new();
        }
        let mut v = Vec::new();
        if t.len() == 1 {
            v.push(Act::Open);
        }
        if t.iter().any(|c| !c.archived()) {
            v.push(Act::Archive);
        }
        if t.iter().any(Card::archived) {
            v.push(Act::Unarchive);
        }
        if t.len() == 1 {
            v.push(Act::Edit);
        }
        v.push(Act::Delete);
        v
    }

    // ---------- actions ----------

    pub fn run(&mut self, act: Act) -> Out {
        self.actions = None;
        let t = self.targets();
        if t.is_empty() || !self.acts().contains(&act) {
            return Out::None;
        }
        match act {
            Act::Open => Out::Open(t[0].clone()),
            Act::Edit => {
                self.form = Form::open(&t[0].id, &self.cards, &self.roster);
                if self.form.is_none() {
                    self.status = Some(("thread gone".into(), true, Instant::now()));
                }
                Out::None
            }
            Act::Unarchive => {
                let ids = t.iter().filter(|c| c.archived()).map(|c| c.id.clone()).collect();
                self.start(Act::Unarchive, ids);
                Out::None
            }
            Act::Archive => {
                let ids = t.iter().filter(|c| !c.archived()).map(|c| c.id.clone()).collect();
                self.pending = Some(Pending { act, ids });
                Out::None
            }
            Act::Delete => {
                self.pending = Some(Pending { act, ids: t.iter().map(|c| c.id.clone()).collect() });
                Out::None
            }
        }
    }

    /// The yes to a question: its job starts. The thread in view among its threads: home.
    fn confirm(&mut self) -> Out {
        let Some(p) = self.pending.take() else { return Out::None };
        let home = self.here.as_ref().is_some_and(|h| p.ids.contains(h));
        self.start(p.act, p.ids);
        if home { Out::Home } else { Out::None }
    }

    /// ACT on IDS, off the UI thread (tests: at once).
    fn start(&mut self, act: Act, ids: Vec<String>) {
        self.sel.clear();
        for id in &ids {
            let to = match act {
                Act::Archive => Some("archived".to_string()),
                Act::Unarchive => Some("open".to_string()),
                _ => None,
            };
            self.moved.insert(id.clone(), to);
        }
        let cards = self.cards.clone();
        self.set_cards(cards);
        // the next list (menu closed, opened again) starts from the moved cards too
        crate::background::save("cards", &self.cards);
        let work = move || {
            let mut ok = Vec::new();
            let mut errs = Vec::new();
            for id in &ids {
                let r = match act {
                    Act::Archive => crate::threads::archive(id),
                    Act::Unarchive => crate::threads::unarchive(id),
                    Act::Delete => delete(id),
                    _ => Ok(String::new()),
                };
                match r {
                    Ok(t) => ok.push(t),
                    Err(e) => errs.push(e),
                }
            }
            let verb = match act {
                Act::Archive => "archived",
                Act::Unarchive => "unarchived",
                _ => "deleted",
            };
            match (ok.len(), errs.first()) {
                (1, None) if ids.len() == 1 => (ok[0].clone(), false),
                (n, None) => (format!("{n} threads {verb}"), false),
                (0, Some(e)) => (e.clone(), true),
                (n, Some(e)) => (format!("{n} threads {verb} · {} failed: {e}", errs.len()), true),
            }
        };
        if self.inline {
            let (text, err) = work();
            self.done(text, err);
            return;
        }
        let (tx, rx) = crate::bus::channel();
        std::thread::spawn(move || {
            let _ = tx.send(work());
        });
        self.job = Some(rx);
        self.status = Some((format!("{}…", match act {
            Act::Archive => "archiving",
            Act::Unarchive => "unarchiving",
            _ => "deleting",
        }), false, Instant::now()));
    }

    // ---------- keys ----------

    pub fn key(&mut self, k: KeyEvent) -> Out {
        self.poll(true);
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        if let Some(form) = self.form.as_mut() {
            match form.key(k) {
                FormOut::Stay => {}
                FormOut::Close => self.form = None,
                FormOut::Save(id, e) => {
                    self.form = None;
                    self.edit(id, e);
                }
            }
            return Out::None;
        }
        if self.pending.is_some() {
            return match k.code {
                KeyCode::Enter | KeyCode::Char('y') => self.confirm(),
                _ => {
                    self.pending = None;
                    Out::None
                }
            };
        }
        if let Some(cur) = self.actions {
            let acts = self.acts();
            match k.code {
                KeyCode::Esc => self.actions = None,
                _ if crate::ctxmenu::is_key(&k) => self.actions = None,
                KeyCode::Up => self.actions = Some(cur.saturating_sub(1)),
                KeyCode::Down => self.actions = Some((cur + 1).min(acts.len().saturating_sub(1))),
                KeyCode::Enter => return acts.get(cur).map_or(Out::None, |&a| self.run(a)),
                KeyCode::Char(c) => {
                    if let Some(&a) = acts.iter().find(|a| a.key() == c.to_string()) {
                        return self.run(a);
                    }
                }
                _ => {}
            }
            return Out::None;
        }
        if self.filtering {
            match k.code {
                KeyCode::Esc | KeyCode::Enter => self.filtering = false,
                KeyCode::Up => self.cursor = self.cursor.saturating_sub(1),
                KeyCode::Down => self.cursor = (self.cursor + 1).min(self.count().saturating_sub(1)),
                _ => self.type_filter(k),
            }
            return Out::None;
        }
        match self.focus {
            Focus::Repos => {
                match k.code {
                    KeyCode::Up => return Out::Up,
                    KeyCode::Down => self.focus = Focus::Views,
                    KeyCode::Enter => self.focus = Focus::List,
                    KeyCode::Left | KeyCode::Right => self.step_repo(if k.code == KeyCode::Right { 1 } else { -1 }),
                    KeyCode::Char(_) if self.mode == Mode::Switch && !ctrl => {
                        self.focus = Focus::List;
                        self.type_filter(k);
                    }
                    _ => return Out::Pass,
                }
                Out::None
            }
            Focus::Views => {
                match k.code {
                    KeyCode::Up => self.focus = Focus::Repos,
                    KeyCode::Down | KeyCode::Enter => self.focus = Focus::List,
                    KeyCode::Left | KeyCode::Right => self.step_view(if k.code == KeyCode::Right { 1 } else { -1 }),
                    KeyCode::Char(_) if self.mode == Mode::Switch && !ctrl => {
                        self.focus = Focus::List;
                        self.type_filter(k);
                    }
                    _ => return Out::Pass,
                }
                Out::None
            }
            Focus::List => self.list_key(k),
        }
    }

    /// Rows of the view (the host's views: their count).
    fn count(&self) -> usize {
        match self.view {
            View::Worktrees => self.extra[0].0,
            View::Claude => self.extra[1].0,
            _ => self.visible().len(),
        }
    }

    fn type_filter(&mut self, k: KeyEvent) {
        let before = self.filter.text();
        self.filter.key(k);
        if self.filter.text() != before {
            self.cursor = 0;
            self.offset = 0;
        }
    }

    fn list_key(&mut self, k: KeyEvent) -> Out {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        let n = self.visible().len();
        let cards = self.view.cards();
        match k.code {
            KeyCode::Left | KeyCode::Right if !ctrl => {
                self.step_repo(if k.code == KeyCode::Right { 1 } else { -1 });
                return Out::None;
            }
            KeyCode::Char('f') if ctrl && self.mode == Mode::Manage => {
                self.filtering = true;
                return Out::None;
            }
            _ => {}
        }
        // the host's views: the host moves its rows (↑ on its top row: `focus_views`)
        if !cards {
            return match k.code {
                KeyCode::Esc if !self.filter.text().is_empty() => {
                    self.filter.set_text("");
                    Out::None
                }
                _ => Out::Pass,
            };
        }
        match k.code {
            KeyCode::Up | KeyCode::Char('k') if k.code == KeyCode::Up || self.mode == Mode::Manage => {
                if self.cursor == 0 {
                    self.focus = Focus::Views;
                } else {
                    self.cursor -= 1;
                }
            }
            KeyCode::Down | KeyCode::Char('j') if k.code == KeyCode::Down || self.mode == Mode::Manage => {
                if self.cursor + 1 < n {
                    self.cursor += 1;
                }
            }
            KeyCode::PageUp => self.cursor = self.cursor.saturating_sub(5),
            KeyCode::PageDown if n > 0 => self.cursor = (self.cursor + 5).min(n - 1),
            KeyCode::Enter => return self.run(Act::Open),
            _ if crate::ctxmenu::is_key(&k) => {
                if !self.acts().is_empty() {
                    self.actions = Some(0);
                }
            }
            _ if self.mode == Mode::Switch => self.type_filter(k),
            KeyCode::Char(' ') => {
                if let Some(id) = self.current().map(|c| c.id.clone()) {
                    if !self.sel.remove(&id) {
                        self.sel.insert(id);
                    }
                    if self.cursor + 1 < n {
                        self.cursor += 1;
                    }
                }
            }
            KeyCode::Char('a') if !ctrl => return self.run(Act::Archive),
            KeyCode::Char('u') if !ctrl => return self.run(Act::Unarchive),
            KeyCode::Char('e') if !ctrl => return self.run(Act::Edit),
            KeyCode::Char('d') if !ctrl => return self.run(Act::Delete),
            KeyCode::Esc if !self.filter.text().is_empty() => {
                self.filter.set_text("");
                self.cursor = 0;
            }
            KeyCode::Esc if !self.sel.is_empty() => self.sel.clear(),
            _ => return Out::Pass,
        }
        Out::None
    }

    /// The host's top row: ↑ goes to the view tabs.
    pub fn focus_views(&mut self) {
        self.focus = Focus::Views;
    }

    fn edit(&mut self, id: String, e: crate::threads::Edit) {
        let work = move || match crate::threads::edit(&id, &e) {
            Ok(t) => (t, false),
            Err(e) => (e, true),
        };
        if cfg!(test) {
            let (t, err) = work();
            self.done(t, err);
            return;
        }
        let (tx, rx) = crate::bus::channel();
        std::thread::spawn(move || {
            let _ = tx.send(work());
        });
        self.job = Some(rx);
        self.status = Some(("saving…".into(), false, Instant::now()));
    }

    // ---------- mouse ----------

    pub fn mouse(&mut self, m: MouseEvent) -> Out {
        self.poll(true);
        let at = Position { x: m.column, y: m.row };
        if self.form.is_some() {
            return Out::None;
        }
        if self.mode == Mode::Switch && self.filter.mouse(m) {
            self.filter.copied = None;
            return Out::None;
        }
        match m.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                if let Some(&(_, a)) = self.act_rows.iter().find(|(r, _)| r.contains(at)) {
                    return self.run(a);
                }
                self.actions = None;
                self.pending = None;
                if self.repos.click(at) {
                    self.focus = Focus::Repos;
                    self.repo_pick = self.repo_tabs().get(self.repos.i).cloned();
                    self.cursor = 0;
                    self.offset = 0;
                    return Out::None;
                }
                if self.views.click(at) {
                    self.focus = Focus::Views;
                    let all = self.views_all();
                    self.set_view(all[self.views.i.min(all.len() - 1)]);
                    return Out::None;
                }
                let Some(&(_, pos)) = self.rows.iter().find(|(r, _)| r.contains(at)) else { return Out::Pass };
                self.focus = Focus::List;
                // the menu: a click opens; the start screen: a click picks, a 2nd opens
                if self.mode == Mode::Switch || pos == self.cursor {
                    self.cursor = pos;
                    return self.run(Act::Open);
                }
                self.cursor = pos;
                Out::None
            }
            MouseEventKind::ScrollDown if self.view.cards() && self.list.contains(at) => {
                self.cursor = (self.cursor + 1).min(self.visible().len().saturating_sub(1));
                Out::None
            }
            MouseEventKind::ScrollUp if self.view.cards() && self.list.contains(at) => {
                self.cursor = self.cursor.saturating_sub(1);
                Out::None
            }
            _ => Out::Pass,
        }
    }

    /// A card / tab under AT (hover).
    pub fn row_at(&self, at: Position) -> Option<Rect> {
        self.act_rows.iter().map(|(r, _)| *r).chain(self.rows.iter().map(|(r, _)| *r)).find(|r| r.contains(at))
    }

    // ---------- draw ----------

    /// Filter, repo tabs, view tabs, the list (the host's views: `list`, the host draws them).
    pub fn draw(&mut self, f: &mut Frame, area: Rect, tick: usize) {
        self.poll(true);
        self.rows.clear();
        self.act_rows.clear();
        if area.height < 5 {
            return;
        }
        let row = |i: u16| Rect { y: area.y + i, height: 1, ..area };
        // the filter
        let fr = row(0);
        let total = self.cards.iter().filter(|c| c.archived() == (self.view == View::Archived)).count();
        let shown = self.visible().len();
        let count = if self.view.cards() && shown != total { format!("{shown} of {total} ") } else { String::new() };
        let cw = unicode_width::UnicodeWidthStr::width(count.as_str()) as u16;
        let input = Rect { x: fr.x + 2, width: fr.width.saturating_sub(2 + cw), ..fr };
        match self.mode {
            Mode::Switch => {
                f.render_widget(Paragraph::new(Span::styled("› ", theme::accent())), Rect { width: 2, ..fr });
                self.filter.render(f, input, true);
            }
            Mode::Manage => {
                let label = Style::default().fg(if self.filtering { theme::pal().accent } else { theme::pal().subtext0 });
                f.render_widget(Paragraph::new(Span::styled("filter › ", label)), fr);
                let input = Rect { x: fr.x + 9, width: fr.width.saturating_sub(9 + cw), ..fr };
                if self.filtering || !self.filter.text().is_empty() {
                    self.filter.render(f, input, self.filtering);
                } else {
                    f.render_widget(Paragraph::new(Span::styled("ctrl+f: fuzzy search thread names", theme::fg(theme::pal().overlay0))), input);
                }
            }
        }
        if cw > 0 {
            f.render_widget(Paragraph::new(Line::styled(count, theme::fg(theme::pal().subtext0)).right_aligned()), fr);
        }
        // repo tabs, view tabs
        let tabs = self.repo_tabs();
        let cur = self.repo_tab();
        self.repos.i = tabs.iter().position(|t| *t == cur).unwrap_or(0);
        let labels: Vec<String> = tabs.iter().map(RepoTab::label).collect();
        self.repos.draw(f, row(1), "repo", &labels, self.focus == Focus::Repos);
        let all = self.views_all();
        self.views.i = all.iter().position(|v| *v == self.view).unwrap_or(0);
        let labels: Vec<String> = all.iter().map(|v| self.view_label(*v)).collect();
        self.views.draw(f, row(2), "show", &labels, self.focus == Focus::Views);
        let mut list = Rect { y: area.y + 4, height: area.height.saturating_sub(4), ..area };
        // the question / the status: the last row
        let foot = Rect { y: list.bottom().saturating_sub(1), height: 1, ..list };
        let foot_line = if let Some(p) = &self.pending {
            let n = p.ids.len();
            // threads with work not in the base: the question names it
            let open: Vec<String> = p.ids.iter().filter_map(|id| self.cards.iter().find(|c| &c.id == id)?.land.clone().filter(|l| l.open())).map(|l| l.badge()).collect();
            let ask_archive = match open.as_slice() {
                [] => format!(" archive {n} thread(s)? "),
                [one] if n == 1 => format!(" {one}: archive anyway? "),
                _ => format!(" archive {n} thread(s), {} open (work not landed)? ", open.len()),
            };
            let (ask, why) = match p.act {
                Act::Archive => (ask_archive, "hosts stop, worktrees go (uncommitted files → git stash), branches stay"),
                _ => (format!(" delete {n} thread(s) + worktree(s)? "), "uncommitted files → git stash, pushed branches go"),
            };
            Some(Line::from(vec![Span::styled(ask, theme::pill().add_modifier(Modifier::BOLD)), Span::styled(" enter: yes · esc: no  ", theme::accent()), Span::styled(why, theme::muted())]))
        } else {
            self.status.as_ref().map(|(t, err, _)| Line::styled(format!(" {t}"), if *err { theme::fg(theme::pal().error()) } else { theme::fg(theme::pal().success()) }))
        };
        if foot_line.is_some() {
            list.height = list.height.saturating_sub(2);
        }
        self.list = list;
        if self.view.cards() {
            self.draw_cards(f, list, tick);
        }
        if let Some(l) = foot_line {
            f.render_widget(Clear, foot);
            f.render_widget(Paragraph::new(l), foot);
        }
        self.draw_actions(f, list);
        if let Some(form) = self.form.as_mut() {
            form.draw(f, area);
        }
    }

    fn draw_cards(&mut self, f: &mut Frame, list: Rect, tick: usize) {
        let vis = self.visible();
        if vis.is_empty() {
            let msg = if self.loading.is_some() && self.cards.is_empty() {
                "reading threads…".to_string()
            } else if !self.filter.text().trim().is_empty() {
                "0 matches".into()
            } else if self.view == View::Archived {
                "0 archived threads".into()
            } else if self.repo_tab() != RepoTab::All {
                "0 threads in this repo".into()
            } else {
                "0 threads".into()
            };
            f.render_widget(Paragraph::new(Span::styled(msg, theme::muted())), Rect { height: 1, ..list });
            return;
        }
        self.cursor = self.cursor.min(vis.len() - 1);
        let rows = self.layout(&vis);
        // virtual y of each row
        let mut ys = Vec::with_capacity(rows.len());
        let mut y = 0u16;
        for r in &rows {
            ys.push(y);
            y += r.height();
        }
        let total = y;
        let ci = rows.iter().position(|r| *r == Row::Card(self.cursor)).unwrap_or(0);
        // the label above the cursor's card stays in view with it
        let top = if ci > 0 && matches!(rows[ci - 1], Row::Label(..)) { ys[ci - 1] } else { ys[ci] };
        let bottom = ys[ci] + nav::CARD_H;
        if top < self.offset {
            self.offset = top;
        } else if bottom > self.offset + list.height {
            self.offset = bottom.saturating_sub(list.height);
        }
        self.offset = self.offset.min(total.saturating_sub(list.height));
        let notes = crate::notify::unseen(self.here.as_deref());
        let focused = self.focus == Focus::List && !self.filtering;
        for (r, &vy) in rows.iter().zip(&ys) {
            if vy < self.offset || vy + r.height() > self.offset + list.height {
                continue;
            }
            let y = list.y + vy - self.offset;
            match *r {
                Row::Label(s, n) => {
                    let st = if s == 2 { theme::fg(theme::pal().success()).add_modifier(Modifier::BOLD) } else { theme::muted().add_modifier(Modifier::BOLD) };
                    f.render_widget(Paragraph::new(Span::styled(Self::label_text(s, n), st)), Rect { y, height: 1, ..list });
                }
                Row::Card(pos) => {
                    let rect = Rect { y, height: nav::CARD_H, ..list };
                    let c = &self.cards[vis[pos]];
                    nav::draw_card(f.buffer_mut(), rect, c, pos == self.cursor && focused, self.here.as_deref() == Some(c.id.as_str()), true, tick);
                    crate::notify::card_badge(f.buffer_mut(), rect, &notes, &c.id);
                    if self.mode == Mode::Manage {
                        let on = self.sel.contains(&c.id);
                        let mark = if on { Span::styled(" ◼ ", theme::accent_bold()) } else { Span::styled(" ◻ ", theme::line()) };
                        f.render_widget(Paragraph::new(mark), Rect { x: rect.x + 1, y: rect.y, width: 3, height: 1 });
                    }
                    self.rows.push((rect, pos));
                }
            }
        }
        self.fx.apply(f, list, self.version);
        let above = rows.iter().zip(&ys).filter(|(r, y)| matches!(r, Row::Card(_)) && **y < self.offset).count();
        let below = rows.iter().zip(&ys).filter(|(r, y)| matches!(r, Row::Card(_)) && **y + nav::CARD_H > self.offset + list.height).count();
        if above + below > 0 {
            let r = Rect { y: list.y.saturating_sub(1), height: 1, ..list };
            f.render_widget(Paragraph::new(Span::styled(format!("{above}↑ · {below}↓"), theme::muted())).right_aligned(), r);
        }
    }

    /// The actions dropdown: right of the list, below the cursor's card.
    fn draw_actions(&mut self, f: &mut Frame, list: Rect) {
        let Some(cur) = self.actions else { return };
        let acts = self.acts();
        if acts.is_empty() {
            self.actions = None;
            return;
        }
        let w = acts.iter().map(|a| a.label().chars().count() as u16 + 10).max().unwrap_or(20).min(list.width);
        let h = acts.len() as u16 + 2;
        let anchor = self.rows.iter().find(|(_, p)| *p == self.cursor).map_or(list.y, |(r, _)| r.y + 2);
        let y = anchor.min(list.bottom().saturating_sub(h)).max(list.y);
        let r = Rect { x: list.right().saturating_sub(w + 1), y, width: w, height: h.min(list.height) };
        f.render_widget(Clear, r);
        let n = self.targets().len();
        let title = if n > 1 { format!(" {n} threads ") } else { " actions ".into() };
        let block = Block::new().borders(Borders::ALL).border_type(BorderType::Plain).border_style(theme::accent()).title(Span::styled(title, theme::accent_bold())).style(theme::text());
        let inner = block.inner(r);
        f.render_widget(block, r);
        for (i, a) in acts.iter().enumerate() {
            let rr = Rect { y: inner.y + i as u16, height: 1, ..inner };
            if rr.y >= inner.bottom() {
                break;
            }
            let hl = i == cur.min(acts.len() - 1);
            let st = if hl { theme::pill() } else { Style::default() };
            let line = Line::from(vec![Span::styled(format!(" {:<6}", a.key()), if hl { st.add_modifier(Modifier::BOLD) } else { theme::accent() }), Span::styled(a.label().to_string(), if hl { st } else { theme::text() })]);
            f.render_widget(Paragraph::new(line).style(st), rr);
            self.act_rows.push((rr, *a));
        }
    }

    /// Foot hints of the list.
    pub fn hints(&self) -> Vec<(&'static str, &'static str)> {
        if self.form.is_some() {
            return vec![("↑↓ tab", "field"), ("←→", "repo"), ("space", "toggle"), ("ctrl+s", "save"), ("esc", "cancel")];
        }
        if self.pending.is_some() {
            return vec![("enter / y", "yes"), ("esc", "no")];
        }
        if self.actions.is_some() {
            return vec![("↑↓", "move"), ("enter", "run"), ("letter", "its action"), ("esc", "close")];
        }
        match (self.mode, self.focus) {
            (_, Focus::Repos) => vec![("←→", "repo"), ("↓", "views"), ("↑", "tabs")],
            (_, Focus::Views) => vec![("←→", "view"), ("↑", "repos"), ("↓", "list")],
            (Mode::Switch, Focus::List) => vec![("type", "filter"), ("←→", "repo"), ("↑↓", "move"), ("enter", "open"), ("ctrl+.", "actions")],
            (Mode::Manage, Focus::List) => vec![("space", "select"), ("a", "archive"), ("u", "unarchive"), ("e", "edit"), ("d", "delete"), ("ctrl+.", "actions")],
        }
    }
}

/// Delete thread ID: its row from the store (linked worktree or in place), `threads::remove`.
fn delete(id: &str) -> Result<String, String> {
    let w = crate::threads::row_of(id).ok_or_else(|| format!("no thread {id}"))?;
    let repo = w.repo.clone().unwrap_or_else(|| PathBuf::from(&w.path));
    let lines = crate::threads::remove(&repo, &[w], &crate::engine::logs_dir());
    let text: Vec<String> = lines.iter().skip(1).map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect::<String>().trim().to_string()).collect();
    match text.iter().find(|l| l.starts_with("skip:") || l.contains("failed") || l.starts_with("worktree:") || l.starts_with("branch:")) {
        Some(e) => Err(e.clone()),
        None => Ok(format!("deleted · {}", text.join(" · "))),
    }
}

// ---------- edit form ----------

enum FormOut {
    Stay,
    Close,
    Save(String, crate::threads::Edit),
}

/// The edit form of a thread: title, repo (←→), linked repos and agents (space: on / off).
struct Form {
    id: String,
    title: TextArea,
    /// Repo picks: None = the repo of its dirs (AUTO).
    repos: Vec<Option<String>>,
    auto: String,
    repo_i: usize,
    links: Vec<(String, bool)>,
    agents: Vec<(String, bool)>,
    row: usize,
    error: Option<String>,
}

impl Form {
    fn open(id: &str, cards: &[Card], roster: &[String]) -> Option<Form> {
        let t = crate::engine::store::load(id)?;
        let auto = crate::threads::repo_root(Path::new(&t.dir)).or_else(|| t.worktree.as_deref().and_then(|p| crate::threads::repo_root(Path::new(p))));
        let mut repos: Vec<Option<String>> = vec![None];
        for r in cards.iter().filter_map(|c| c.repo.as_ref()).map(|p| p.display().to_string()).chain(t.repo.clone()) {
            if !repos.contains(&Some(r.clone())) && Some(r.as_str()) != auto.as_ref().and_then(|a| a.to_str()) {
                repos.push(Some(r));
            }
        }
        repos[1..].sort_by_key(|r| r.as_deref().map(|p| repo_name(Path::new(p))));
        let repo_i = t.repo.as_ref().and_then(|r| repos.iter().position(|x| x.as_ref() == Some(r))).unwrap_or(0);
        let mut title = TextArea::new(true, "title");
        title.style = Some(theme::text());
        title.set_text(t.title.lines().next().unwrap_or_default());
        let mut agents: Vec<(String, bool)> = t.agents.iter().map(|a| (a.clone(), true)).collect();
        for a in roster {
            if !agents.iter().any(|(n, _)| n == a) {
                agents.push((a.clone(), false));
            }
        }
        Some(Form {
            id: t.id.clone(),
            title,
            repos,
            auto: auto.map_or_else(|| "outside git".into(), |p| repo_name(&p)),
            repo_i,
            links: t.checkouts.iter().map(|c| (c.dir.clone(), true)).collect(),
            agents,
            row: 0,
            error: None,
        })
    }

    fn n_rows(&self) -> usize {
        2 + self.links.len() + self.agents.len()
    }

    fn key(&mut self, k: KeyEvent) -> FormOut {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL) || k.modifiers.contains(KeyModifiers::SUPER);
        let n = self.n_rows();
        match k.code {
            KeyCode::Esc => return FormOut::Close,
            KeyCode::Char('s') if ctrl => return self.save(),
            KeyCode::Enter if ctrl || self.row == 0 => return self.save(),
            KeyCode::Up | KeyCode::BackTab => self.row = self.row.saturating_sub(1),
            KeyCode::Down | KeyCode::Tab => self.row = (self.row + 1).min(n - 1),
            _ if self.row == 0 => {
                self.title.key(k);
            }
            KeyCode::Left | KeyCode::Right if self.row == 1 => {
                let d: isize = if k.code == KeyCode::Right { 1 } else { -1 };
                self.repo_i = (self.repo_i as isize + d).rem_euclid(self.repos.len() as isize) as usize;
            }
            KeyCode::Char(' ') | KeyCode::Enter if self.row >= 2 => {
                let i = self.row - 2;
                match self.links.get_mut(i) {
                    Some(l) => l.1 = !l.1,
                    None => {
                        if let Some(a) = self.agents.get_mut(i - self.links.len()) {
                            a.1 = !a.1;
                        }
                    }
                }
            }
            _ => {}
        }
        FormOut::Stay
    }

    fn save(&mut self) -> FormOut {
        let agents: Vec<String> = self.agents.iter().filter(|a| a.1).map(|a| a.0.clone()).collect();
        if agents.is_empty() {
            self.error = Some("1 agent at least".into());
            return FormOut::Stay;
        }
        if self.title.text().trim().is_empty() {
            self.error = Some("title required".into());
            self.row = 0;
            return FormOut::Stay;
        }
        let e = crate::threads::Edit {
            title: self.title.text(),
            repo: self.repos.get(self.repo_i).cloned().flatten(),
            checkouts: self.links.iter().filter(|l| l.1).map(|l| l.0.clone()).collect(),
            agents,
        };
        FormOut::Save(self.id.clone(), e)
    }

    fn draw(&mut self, f: &mut Frame, area: Rect) {
        let w = area.width.saturating_sub(4).min(76);
        let body = 2 + 1 + self.links.len().max(1) + 1 + self.agents.len() + 2 + usize::from(self.error.is_some());
        let h = (body as u16 + 2).min(area.height);
        let r = Rect { x: area.x + (area.width - w) / 2, y: area.y + 1.min(area.height.saturating_sub(h)), width: w, height: h };
        f.render_widget(Clear, r);
        let block = Block::new().borders(Borders::ALL).border_type(BorderType::Plain).border_style(theme::accent()).title(Span::styled(" edit thread ", theme::accent_bold())).style(theme::text());
        let inner = block.inner(r);
        f.render_widget(block, r);
        let inner = Rect { x: inner.x + 1, width: inner.width.saturating_sub(2), ..inner };
        let mut y = inner.y;
        let line = |f: &mut Frame, y: u16, l: Line| {
            if y < inner.bottom() {
                f.render_widget(Paragraph::new(l), Rect { y, height: 1, ..inner });
            }
        };
        let label = |on: bool, t: &str| Span::styled(format!("{t:<8}"), if on { theme::accent_bold() } else { theme::muted() });
        line(f, y, Line::from(label(self.row == 0, "title")));
        if y < inner.bottom() {
            self.title.render(f, Rect { x: inner.x + 8, y, width: inner.width.saturating_sub(8), height: 1 }, self.row == 0);
        }
        y += 1;
        let repo = match self.repos.get(self.repo_i).cloned().flatten() {
            Some(p) => format!("{} · {}", repo_name(Path::new(&p)), p),
            None => format!("auto: {} (the repo of its dir)", self.auto),
        };
        let st = if self.row == 1 { theme::pill() } else { theme::text() };
        line(f, y, Line::from(vec![label(self.row == 1, "repo"), Span::styled(format!("‹ {repo} ›"), st), Span::styled("  ←→", theme::muted())]));
        y += 1;
        let check = |on: bool, hl: bool, text: String| {
            let st = if hl { theme::pill() } else { theme::text() };
            vec![Span::styled(if on { "◼ " } else { "◻ " }, if on { theme::accent() } else { theme::line() }), Span::styled(text, st)]
        };
        line(f, y, Line::styled("linked repos (space: keep / drop)", theme::muted().add_modifier(Modifier::BOLD)));
        y += 1;
        if self.links.is_empty() {
            line(f, y, Line::styled("  0 linked repos", theme::muted()));
            y += 1;
        }
        for (i, (dir, on)) in self.links.iter().enumerate() {
            let mut s = vec![Span::raw("  ")];
            s.extend(check(*on, self.row == 2 + i, dir.clone()));
            line(f, y, Line::from(s));
            y += 1;
        }
        line(f, y, Line::styled("agents (space: on / off; the 1st on: lead)", theme::muted().add_modifier(Modifier::BOLD)));
        y += 1;
        let lead = self.agents.iter().position(|a| a.1);
        for (i, (name, on)) in self.agents.iter().enumerate() {
            let mut s = vec![Span::raw("  ")];
            s.extend(check(*on, self.row == 2 + self.links.len() + i, name.clone()));
            if Some(i) == lead {
                s.push(Span::styled("  lead", theme::muted()));
            }
            line(f, y, Line::from(s));
            y += 1;
        }
        if let Some(e) = &self.error {
            line(f, y, Line::styled(e.clone(), theme::fg(theme::pal().error())));
            y += 1;
        }
        y += 1;
        line(f, y, Line::styled("ctrl+s: save · esc: cancel · live agents leave / join at once", theme::muted()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn k(c: KeyCode) -> KeyEvent {
        KeyEvent::new(c, KeyModifiers::NONE)
    }

    fn ctrl(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
    }

    fn card(id: &str, f: impl FnOnce(&mut Card)) -> Card {
        let mut c = Card { id: id.into(), title: id.into(), state: "open".into(), ..Default::default() };
        f(&mut c);
        c
    }

    fn screen(l: &mut ThreadList, w: u16, h: u16) -> String {
        let mut t = ratatui::Terminal::new(ratatui::backend::TestBackend::new(w, h)).unwrap();
        t.draw(|f| l.draw(f, f.area(), 0)).unwrap();
        let b = t.backend().buffer().clone();
        (0..h).map(|y| (0..w).map(|x| b[(x, y)].symbol().to_string()).collect::<String>()).collect::<Vec<_>>().join("\n")
    }

    /// Sections: open (working, a question, code not landed, 0 code yet, 0 state read), stale
    /// (open with 0 code, idle 3 days: a question too), shipped; archived threads only in their
    /// view.
    #[test]
    fn sections_and_the_archived_view() {
        use crate::landed::Land;
        let now = crate::engine::now_ms();
        let hour = 3600 * 1000;
        let cards = vec![
            card("idle", |_| {}),
            card("asks", |c| c.blocked = true),
            card("busy", |c| c.busy = true),
            card("shipped", |c| c.land = Some(Land::Landed)),
            card("talk", |c| {
                c.land = Some(Land::NoCode);
                c.done = true;
            }),
            card("new", |c| {
                c.land = Some(Land::NoCode);
                c.activity = now - hour;
            }),
            card("dusty", |c| {
                c.land = Some(Land::NoCode);
                c.activity = now - 5 * 24 * hour - hour;
            }),
            card("loose", |c| {
                c.land = Some(Land::Unpushed(3));
                c.activity = now - 9 * 24 * hour;
            }),
            card("waits", |c| {
                c.blocked = true;
                c.land = Some(Land::NoCode);
                c.activity = now - 4 * 24 * hour;
            }),
            card("old", |c| c.state = "archived".into()),
        ];
        let mut l = ThreadList::new(Mode::Switch, None, None, cards);
        let ids: Vec<String> = l.visible().iter().map(|&i| l.cards[i].id.clone()).collect();
        // 0 code: shipped only when the agents reported done; idle 3 days: stale (code: never)
        assert_eq!(ids, ["idle", "asks", "busy", "new", "loose", "dusty", "waits", "shipped", "talk"]);
        let s = screen(&mut l, 80, 90);
        for x in ["open · 5", "stale · 2", "shipped: archive? · 2", "◇ 3 unpushed", "⇲ shipped", "⇲ nothing to ship", "auto-archive in 25d"] {
            assert!(s.contains(x), "{x}: {s}");
        }
        for x in ["needs you ·", "recent ·", "working ·", "threads ·"] {
            assert!(!s.contains(x), "{x}: {s}");
        }
        assert_eq!(s.matches("nothing to ship").count(), 1, "{s}");
        assert!(s.contains("active 9") && s.contains("archived 1") && !s.contains("old"), "{s}");

        // ↑ from the 1st card: the view tabs; → archived
        l.key(k(KeyCode::Up));
        assert_eq!(l.focus, Focus::Views);
        l.key(k(KeyCode::Right));
        l.key(k(KeyCode::Down));
        assert_eq!((l.view, l.focus), (View::Archived, Focus::List));
        let s = screen(&mut l, 70, 40);
        assert!(s.contains("old") && s.contains("archived · 1") && s.contains("▣ archived"), "{s}");
    }

    /// The menu: letters filter; ctrl+o → the actions; a → the question; enter → archived.
    #[test]
    fn switch_mode_archives_by_ctrl_dot_and_a_after_a_yes() {
        let dir = std::env::temp_dir();
        let t = crate::engine::store::Thread::new("archive from menu", "task", &dir, vec!["ada".into()]);
        t.save().unwrap();
        let mut l = ThreadList::new(Mode::Switch, None, None, vec![card(&t.id, |c| c.title = "archive from menu".into())]);
        l.key(k(KeyCode::Char('a')));
        assert_eq!(l.query(), "a", "a letter filters in the menu");
        l.key(k(KeyCode::Backspace));
        l.key(ctrl('.'));
        let s = screen(&mut l, 80, 30);
        assert!(s.contains("actions") && s.contains("archive: worktree goes") && s.contains("delete"), "{s}");
        l.key(k(KeyCode::Char('a')));
        let s = screen(&mut l, 80, 30);
        assert!(s.contains("archive 1 thread(s)?") && s.contains("enter: yes"), "{s}");
        assert_eq!(crate::engine::store::load(&t.id).unwrap().state, "open", "0 archive before the yes");
        l.key(k(KeyCode::Enter));
        assert_eq!(crate::engine::store::load(&t.id).unwrap().state, "archived");
        assert!(l.take_changed());
        let s = screen(&mut l, 80, 30);
        assert!(s.contains("archived"), "{s}");
        let _ = crate::engine::store::delete(&t.id);
    }

    /// The thread in view archived from the menu: home.
    #[test]
    fn archiving_the_thread_in_view_goes_home() {
        let dir = std::env::temp_dir();
        let t = crate::engine::store::Thread::new("here", "task", &dir, vec!["ada".into()]);
        t.save().unwrap();
        let mut l = ThreadList::new(Mode::Switch, Some(t.id.clone()), None, vec![card(&t.id, |_| {})]);
        l.key(ctrl('.'));
        l.key(k(KeyCode::Char('a')));
        assert_eq!(l.key(k(KeyCode::Char('y'))), Out::Home);
        let _ = crate::engine::store::delete(&t.id);
    }

    /// Archive of a thread with work not landed: the question names the work; 2 picks, 1 open:
    /// the count.
    #[test]
    fn archive_of_an_open_thread_names_the_work() {
        use crate::landed::Land;
        let cards = vec![card("loose", |c| c.land = Some(Land::Unpushed(3))), card("done", |c| c.land = Some(Land::Landed))];
        let mut l = ThreadList::new(Mode::Manage, None, None, cards);
        l.key(k(KeyCode::Char('a')));
        let s = screen(&mut l, 120, 30);
        assert!(s.contains("3 unpushed: archive anyway?"), "{s}");
        l.key(k(KeyCode::Esc));

        l.key(k(KeyCode::Char(' ')));
        l.key(k(KeyCode::Char(' ')));
        l.key(k(KeyCode::Char('a')));
        let s = screen(&mut l, 120, 30);
        assert!(s.contains("archive 2 thread(s), 1 open (work not landed)?"), "{s}");
    }

    /// The start screen: space picks, a asks for every pick, esc says no; e opens the form,
    /// ctrl+s saves the title.
    #[test]
    fn manage_mode_selects_asks_and_edits() {
        let dir = std::env::temp_dir();
        let t = crate::engine::store::Thread::new("old title", "task", &dir, vec!["ada".into()]);
        t.save().unwrap();
        let mut l = ThreadList::new(Mode::Manage, None, None, vec![card(&t.id, |_| {}), card("other", |_| {})]);
        l.roster = vec!["ada".into(), "bob".into()];
        l.key(k(KeyCode::Char(' ')));
        l.key(k(KeyCode::Char(' ')));
        assert_eq!(l.sel.len(), 2);
        l.key(k(KeyCode::Char('a')));
        let s = screen(&mut l, 100, 30);
        assert!(s.contains("archive 2 thread(s)?"), "{s}");
        l.key(k(KeyCode::Esc));
        assert!(l.pending.is_none() && l.sel.len() == 2);
        l.key(k(KeyCode::Esc));
        assert!(l.sel.is_empty(), "esc: 0 picks");

        l.cursor = 0;
        l.key(k(KeyCode::Char('e')));
        let s = screen(&mut l, 100, 30);
        assert!(s.contains("edit thread") && s.contains("old title") && s.contains("auto:") && s.contains("bob"), "{s}");
        for _ in 0.."old title".len() {
            l.key(k(KeyCode::Backspace));
        }
        for c in "new title".chars() {
            l.key(k(KeyCode::Char(c)));
        }
        // agents: bob on (rows: title, repo, 0 links, ada, bob)
        l.key(k(KeyCode::Down));
        l.key(k(KeyCode::Down));
        l.key(k(KeyCode::Down));
        l.key(k(KeyCode::Char(' ')));
        l.key(ctrl('s'));
        let s = crate::engine::store::load(&t.id).unwrap();
        assert_eq!((s.title.as_str(), s.agents.clone()), ("new title", vec!["ada".to_string(), "bob".to_string()]));
        let _ = crate::engine::store::delete(&t.id);
    }

    /// Repo tabs: the thread in view's repo first and picked; ←→ steps.
    #[test]
    fn the_repo_tab_of_the_thread_in_view_comes_first() {
        let cards = vec![card("a", |c| c.repo = Some("/r/one".into())), card("b", |c| c.repo = Some("/r/two".into())), card("c", |c| c.repo = Some("/r/two".into()))];
        let mut l = ThreadList::new(Mode::Switch, Some("a".into()), None, cards);
        assert_eq!(l.repo_tab(), RepoTab::Repo("/r/one".into()));
        assert_eq!(l.visible().len(), 1);
        l.key(k(KeyCode::Right));
        assert_eq!(l.repo_tab(), RepoTab::Repo("/r/two".into()));
        assert_eq!(l.visible().len(), 2);
    }
}
