//! The menu (ctrl+p): 1 modal for the start screen and the thread view. Tabs: quick (commands; in
//! a thread: per agent its flow, remove, replace), threads (filter + thread cards), search (the
//! messages of every thread), tokens. 3 focus levels: the tab bar, a tab's tab group (threads:
//! repos, merge: states), its list. ↑ at the top of a list: up 1 level; ↓: down 1 level. ←→ on
//! the tab bar (a list without a tab group too): next / previous tab; on a tab group: its tabs.
//! enter: run, a single esc or tab closes.

use std::sync::mpsc::{self, Receiver};
use std::time::Instant;

use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph};

use crate::agents::Agent;
use crate::nav::{self, Card};
use crate::textarea::TextArea;
use crate::theme;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tab {
    Quick,
    Threads,
    /// The owner's pull requests.
    Merge,
    Search,
    Tokens,
}

const TABS: [(Tab, &str); 5] = [(Tab::Quick, "quick"), (Tab::Threads, "threads"), (Tab::Merge, "merge"), (Tab::Search, "search"), (Tab::Tokens, "tokens")];

/// A tab of the start screen the menu opens.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Home {
    New,
    Settings,
    /// The merge tab, a PR open in it (0 thread has it).
    Merge(Option<crate::gh::PrRef>),
    /// The flows tab, a flow open on its rail.
    Flows(Option<String>),
}

/// What the host does after a key / click.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    /// Menu stays open.
    None,
    Close,
    Home(Home),
    Open(Card),
    /// A search result: thread CARD, scrolled to its message of time TS.
    Jump(Card, u64),
    /// Quick command of the host (its id).
    Command(usize),
    /// A PR of the merge tab: its thread's merge tab, else the start screen's.
    Pr(crate::prlist::Entry),
}

/// Quick command of the host: label, hotkey hint, id, section (a heading row over each section).
pub struct Item {
    pub label: String,
    pub key: String,
    pub id: usize,
    pub section: &'static str,
}

/// Where the keys go: the tab bar, the tab group of the tab (threads, merge), the list.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Level {
    Bar,
    Group,
    Body,
}

pub struct Menu {
    pub tab: Tab,
    pub level: Level,
    items: Vec<Item>,
    qcursor: usize,
    filter: TextArea,
    cards: Vec<Card>,
    loading: Option<Receiver<Vec<Card>>>,
    /// Start of the last cards read: the threads tab reads again every 2s (busy, unread, blocked live).
    cards_at: Option<Instant>,
    /// Data version of the cards: up with each read that brings new cards (they dissolve in).
    cards_version: u64,
    cards_fx: crate::dissolve::Dissolve,
    tcursor: usize,
    toffset: usize,
    /// Repo tabs of the threads tab (←→): the current thread's repo, the others by thread count,
    /// rows outside of git repos, all.
    repos: crate::tabgroup::TabGroup,
    /// The picked repo tab (None: the default, the 1st tab).
    repo_pick: Option<RepoTab>,
    /// Thread in view (None: start screen).
    thread: Option<String>,
    /// The agents (their colors in the search tab).
    roster: Vec<Agent>,
    started: Instant,
    /// Tokens tab: the last read, the read running, its time, the first line in view.
    detail: Option<crate::cost::Detail>,
    detail_rx: Option<Receiver<crate::cost::Detail>>,
    detail_at: Option<Instant>,
    dscroll: u16,
    /// Search tab: the query, every thread's messages (read once, in the background), the hits of
    /// QUERY (indexes into MSGS) and the query they are for, cursor, 1st hit in view.
    search: TextArea,
    msgs: Option<Vec<crate::search::Msg>>,
    msgs_rx: Option<Receiver<Vec<crate::search::Msg>>>,
    /// Start of the last messages read: the search tab reads again every 5s (new messages).
    msgs_at: Option<Instant>,
    hits: Vec<usize>,
    hits_for: String,
    scursor: usize,
    soffset: usize,
    /// Last draw: tab labels, rows (clicks).
    tab_rects: Vec<(Rect, Tab)>,
    rows: Vec<(Rect, usize)>,
    frame: Rect,
    /// Merge tab: the owner's PRs.
    prs: crate::prlist::PrList,
}

/// Thread cards, read in the background (also saved: the next menu shows them at once).
fn read_cards() -> Receiver<Vec<Card>> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let cards = nav::load();
        crate::background::save("cards", &cards);
        let _ = tx.send(cards);
    });
    rx
}

/// Time between 2 reads of the cards (threads tab) / the messages (search tab) while they show.
const CARDS_EVERY: std::time::Duration = std::time::Duration::from_secs(2);
const MSGS_EVERY: std::time::Duration = std::time::Duration::from_secs(5);

fn builtins() -> Vec<Item> {
    vec![
        Item { label: "new thread: start screen, new tab".into(), key: "n".into(), id: usize::MAX, section: "codehead" },
        Item { label: "settings: start screen, settings tab".into(), key: String::new(), id: usize::MAX - 1, section: "codehead" },
        Item { label: "search: the messages of every thread".into(), key: "f".into(), id: usize::MAX - 2, section: "codehead" },
    ]
}

impl Menu {
    /// ITEMS: the host's quick commands (after new + settings). THREAD: the thread in view.
    /// CARDS: thread cards the host has (empty: read in the background now).
    pub fn new(items: Vec<Item>, thread: Option<String>, roster: Vec<Agent>, cards: Vec<Card>) -> Menu {
        // tests: 0 thread reads (they write the seen file)
        let loading = (cards.is_empty() && !cfg!(test)).then(read_cards);
        // the cards of the last read at once; the read's new cards dissolve in
        let cards = if cards.is_empty() && !cfg!(test) { crate::background::snapshot("cards").unwrap_or_default() } else { cards };
        let mut all = builtins();
        all.extend(items);
        let mut filter = TextArea::new(true, "filter threads");
        filter.style = Some(theme::text());
        let mut search = TextArea::new(true, "search messages: you + agents, every thread");
        search.style = Some(theme::text());
        Menu {
            tab: Tab::Quick,
            level: Level::Body,
            items: all,
            qcursor: 0,
            filter,
            cards,
            cards_at: loading.is_some().then(Instant::now),
            loading,
            cards_version: 0,
            cards_fx: Default::default(),
            tcursor: 0,
            toffset: 0,
            repos: crate::tabgroup::TabGroup::default(),
            repo_pick: None,
            thread,
            roster,
            started: Instant::now(),
            detail: None,
            detail_rx: None,
            detail_at: None,
            dscroll: 0,
            search,
            msgs: None,
            msgs_rx: None,
            msgs_at: None,
            hits: Vec::new(),
            hits_for: String::new(),
            scursor: 0,
            soffset: 0,
            tab_rects: Vec::new(),
            rows: Vec::new(),
            frame: Rect::default(),
            prs: crate::prlist::PrList::default(),
        }
    }



    /// Tokens tab: a new read every 2s while it shows.
    fn poll_detail(&mut self) {
        if let Some(rx) = &self.detail_rx
            && let Ok(d) = rx.try_recv()
        {
            self.detail = Some(d);
            self.detail_rx = None;
        }
        let due = self.detail_at.is_none_or(|t| t.elapsed() >= std::time::Duration::from_secs(2));
        if self.tab == Tab::Tokens && self.detail_rx.is_none() && due && !cfg!(test) {
            let (tx, rx) = mpsc::channel();
            let id = self.thread.clone();
            std::thread::spawn(move || {
                let _ = tx.send(crate::cost::detail(id.as_deref()));
            });
            self.detail_rx = Some(rx);
            self.detail_at = Some(Instant::now());
        }
    }

    /// A background read on its way (threads, messages, tokens, PRs): the host draws often.
    pub fn waiting(&self) -> bool {
        self.loading.is_some() || self.msgs_rx.is_some() || self.detail_rx.is_some() || (self.tab == Tab::Merge && self.prs.reading())
    }

    fn poll(&mut self) {
        if self.tab == Tab::Merge {
            self.prs.ensure();
            self.prs.poll();
        }
        if let Some(rx) = &self.loading
            && let Ok(c) = rx.try_recv()
        {
            self.loading = None;
            self.set_cards(c);
        }
        // the threads tab: a new read every 2s while it shows
        let due = |at: Option<Instant>, every| at.is_none_or(|t| t.elapsed() >= every);
        if self.tab == Tab::Threads && self.loading.is_none() && due(self.cards_at, CARDS_EVERY) && !cfg!(test) {
            self.loading = Some(read_cards());
            self.cards_at = Some(Instant::now());
        }
        if let Some(rx) = &self.msgs_rx
            && let Ok(m) = rx.try_recv()
        {
            self.msgs_rx = None;
            self.set_msgs(m);
        }
        // the search tab: a new read on its 1st show, then every 5s while it shows
        if self.tab == Tab::Search && self.msgs_rx.is_none() && due(self.msgs_at, MSGS_EVERY) && !cfg!(test) {
            let (tx, rx) = mpsc::channel();
            std::thread::spawn(move || {
                let _ = tx.send(crate::search::load());
            });
            self.msgs_rx = Some(rx);
            self.msgs_at = Some(Instant::now());
        }
        self.refresh_hits();
    }

    /// New cards (a read): the cursor stays on its thread (a busy thread moves up the list).
    fn set_cards(&mut self, c: Vec<Card>) {
        if c == self.cards {
            return;
        }
        let picked = self.visible().get(self.tcursor).map(|&i| self.cards[i].id.clone());
        self.cards = c;
        self.cards_version += 1;
        if let Some(id) = picked
            && let Some(p) = self.visible().iter().position(|&i| self.cards[i].id == id)
        {
            self.tcursor = p;
        }
    }

    /// New messages (a read): the hits again, the cursor stays on its message.
    pub fn set_msgs(&mut self, m: Vec<crate::search::Msg>) {
        let key = |m: &crate::search::Msg| (m.thread.clone(), m.ts, m.from.clone());
        let picked = self.hits.get(self.scursor).and_then(|&i| self.msgs.as_ref()?.get(i)).map(key);
        self.msgs = Some(m);
        let q = self.search.text();
        let msgs = self.msgs.as_deref().unwrap_or_default();
        self.hits = crate::search::find(msgs, &q);
        self.hits_for = q;
        self.scursor = picked.and_then(|k| self.hits.iter().position(|&i| key(&msgs[i]) == k)).unwrap_or(0);
    }

    /// The hits of the query, if it changed (or the messages came).
    fn refresh_hits(&mut self) {
        let q = self.search.text();
        if q == self.hits_for {
            return;
        }
        self.hits = self.msgs.as_deref().map(|m| crate::search::find(m, &q)).unwrap_or_default();
        self.hits_for = q;
        self.scursor = 0;
        self.soffset = 0;
    }

    /// The card of thread ID (the threads tab's, else 1 from the message).
    fn card_of(&self, m: &crate::search::Msg) -> Card {
        self.cards.iter().find(|c| c.id == m.thread).cloned().unwrap_or_else(|| Card { id: m.thread.clone(), title: m.title.clone(), dir: m.dir.clone(), ..Default::default() })
    }

    fn jump(&self, hit: usize) -> Action {
        let Some(m) = self.hits.get(hit).and_then(|&i| self.msgs.as_ref()?.get(i)) else { return Action::None };
        Action::Jump(self.card_of(m), m.ts)
    }

    fn search_key(&mut self, k: KeyEvent) -> Action {
        let n = self.hits.len();
        match k.code {
            KeyCode::Up => self.scursor = self.scursor.saturating_sub(1),
            KeyCode::Down if n > 0 => self.scursor = (self.scursor + 1).min(n - 1),
            KeyCode::PageUp => self.scursor = self.scursor.saturating_sub(5),
            KeyCode::PageDown if n > 0 => self.scursor = (self.scursor + 5).min(n - 1),
            KeyCode::Enter => return self.jump(self.scursor),
            _ => {
                self.search.key(k);
                self.refresh_hits();
            }
        }
        Action::None
    }

    /// The threads tab's cards (filter + repo applied); the current thread always on top.
    fn visible(&self) -> Vec<usize> {
        let tab = self.repo_tab();
        let mut v: Vec<usize> = nav::filter(&self.cards, &self.filter.text()).into_iter().filter(|&i| tab.keeps(self.cards[i].repo.as_deref())).collect();
        if let Some(p) = v.iter().position(|&i| self.thread.as_deref() == Some(self.cards[i].id.as_str())) {
            let here = v.remove(p);
            v.insert(0, here);
        }
        v
    }

    /// The repo tabs: the current thread's repo, the other repos (most threads first, then A→Z),
    /// outside of git repos (when a thread is), all.
    fn repo_tabs(&self) -> Vec<RepoTab> {
        let here = self.thread.as_deref().and_then(|id| self.cards.iter().find(|c| c.id == id)).and_then(|c| c.repo.clone());
        let mut counts: Vec<(std::path::PathBuf, usize)> = Vec::new();
        let mut outside = false;
        for c in &self.cards {
            match &c.repo {
                Some(p) => match counts.iter_mut().find(|(q, _)| q == p) {
                    Some(e) => e.1 += 1,
                    None => counts.push((p.clone(), 1)),
                },
                None => outside = true,
            }
        }
        let name = |p: &std::path::Path| p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| p.display().to_string());
        counts.sort_by(|a, b| (Some(&b.0) == here.as_ref()).cmp(&(Some(&a.0) == here.as_ref())).then(b.1.cmp(&a.1)).then_with(|| name(&a.0).cmp(&name(&b.0))));
        let mut v: Vec<RepoTab> = counts.into_iter().map(|(p, _)| RepoTab::Repo(p)).collect();
        if outside {
            v.push(RepoTab::Outside);
        }
        v.push(RepoTab::All);
        v
    }

    /// The repo tab in use: the pick, else the current thread's repo (0 thread: all).
    fn repo_tab(&self) -> RepoTab {
        let tabs = self.repo_tabs();
        if let Some(p) = self.repo_pick.as_ref().filter(|p| tabs.contains(p)) {
            return p.clone();
        }
        if self.thread.is_some() && let Some(t) = tabs.first() {
            return t.clone();
        }
        RepoTab::All
    }

    /// ←→ on the repo tabs.
    fn step_repo(&mut self, d: isize) {
        let tabs = self.repo_tabs();
        let cur = self.repo_tab();
        self.repos.i = tabs.iter().position(|t| *t == cur).unwrap_or(0);
        self.repos.step(d, tabs.len());
        self.repo_pick = tabs.get(self.repos.i).cloned();
        self.tcursor = 0;
        self.toffset = 0;
    }






    fn switch(&mut self, d: isize) {
        let i = TABS.iter().position(|(t, _)| *t == self.tab).unwrap_or(0) as isize;
        self.tab = TABS[(i + d).rem_euclid(TABS.len() as isize) as usize].0;
        self.level = Level::Bar;
    }

    /// The tab has a tab group (threads: repos, merge: states).
    fn grouped(&self) -> bool {
        matches!(self.tab, Tab::Threads | Tab::Merge)
    }

    /// The list of the tab shows its 1st row (↑: up 1 level).
    fn at_top(&self) -> bool {
        match self.tab {
            Tab::Quick => self.qcursor == 0,
            Tab::Threads => self.tcursor == 0,
            Tab::Merge => self.prs.at_top(),
            Tab::Search => self.scursor == 0,
            Tab::Tokens => self.dscroll == 0,
        }
    }

    /// Keys on the tab bar / the tab group. Some: the key is done.
    fn level_key(&mut self, k: KeyEvent) -> Option<Action> {
        let up = k.code == KeyCode::Up;
        match self.level {
            Level::Body if up && self.at_top() => {
                self.level = if self.grouped() { Level::Group } else { Level::Bar };
                Some(Action::None)
            }
            Level::Body => None,
            Level::Bar => {
                match k.code {
                    KeyCode::Left | KeyCode::Right => self.switch(if k.code == KeyCode::Right { 1 } else { -1 }),
                    KeyCode::Down | KeyCode::Enter => self.level = if self.grouped() { Level::Group } else { Level::Body },
                    KeyCode::Up => {}
                    // a letter: the list (its filter) takes it
                    _ => {
                        self.level = Level::Body;
                        return None;
                    }
                }
                Some(Action::None)
            }
            Level::Group => {
                match k.code {
                    KeyCode::Up => self.level = Level::Bar,
                    KeyCode::Down | KeyCode::Enter => self.level = Level::Body,
                    KeyCode::Left | KeyCode::Right if self.tab == Tab::Threads => self.step_repo(if k.code == KeyCode::Right { 1 } else { -1 }),
                    KeyCode::Left | KeyCode::Right => {
                        self.prs.key(k);
                    }
                    _ => {
                        self.level = Level::Body;
                        return None;
                    }
                }
                Some(Action::None)
            }
        }
    }

    pub fn key(&mut self, k: KeyEvent) -> Action {
        self.poll();
        let ctrl = k.modifiers.contains(ratatui::crossterm::event::KeyModifiers::CONTROL);
        // tab / shift+tab: the next / previous menu tab, from every level; a tab with a tab group
        // (threads: repos, merge: states): ←→ move in the group; the other tabs: ←→ switch them
        // too; ctrl+←→: the filter's word jumps
        let group = self.grouped();
        match k.code {
            // esc or the menu key again: closed
            KeyCode::Esc => return Action::Close,
            _ if crate::config::menu_key().matches(&k) => return Action::Close,
            KeyCode::Tab | KeyCode::BackTab if !ctrl => {
                self.switch(if k.code == KeyCode::Tab { 1 } else { -1 });
                return Action::None;
            }
            KeyCode::Right | KeyCode::Left if !ctrl && !group && self.level == Level::Body => {
                self.switch(if k.code == KeyCode::Right { 1 } else { -1 });
                return Action::None;
            }
            _ if self.level != Level::Body || k.code == KeyCode::Up => {
                if let Some(a) = self.level_key(k) {
                    return a;
                }
            }
            _ => {}
        }
        match k.code {
            KeyCode::Right | KeyCode::Left if self.tab == Tab::Threads => {
                self.step_repo(if k.code == KeyCode::Right { 1 } else { -1 });
                return Action::None;
            }
            _ => {}
        }
        match self.tab {
            Tab::Quick => self.quick_key(k),
            Tab::Threads => self.threads_key(k),
            Tab::Search => self.search_key(k),
            Tab::Merge => match self.prs.key(k) {
                crate::prlist::Pick::Open(e) => Action::Pr(e),
                _ => Action::None,
            },
            Tab::Tokens => {
                match k.code {
                    KeyCode::Up | KeyCode::Char('k') => self.dscroll = self.dscroll.saturating_sub(1),
                    KeyCode::Down | KeyCode::Char('j') => self.dscroll = self.dscroll.saturating_add(1),
                    KeyCode::PageUp => self.dscroll = self.dscroll.saturating_sub(10),
                    KeyCode::PageDown => self.dscroll = self.dscroll.saturating_add(10),
                    KeyCode::Char('r') => self.detail_at = None,
                    _ => {}
                }
                Action::None
            }
        }
    }

    fn quick_key(&mut self, k: KeyEvent) -> Action {
        let n = self.items.len();
        match k.code {
            KeyCode::Up | KeyCode::Char('k') => self.qcursor = self.qcursor.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') => self.qcursor = (self.qcursor + 1).min(n - 1),
            KeyCode::Char('n') => return Action::Home(Home::New),
            KeyCode::Char('f') => {
                self.tab = Tab::Search;
                self.level = Level::Body;
            }
            KeyCode::Enter => return self.run_item(self.qcursor),
            _ => {}
        }
        Action::None
    }

    fn run_item(&mut self, i: usize) -> Action {
        match self.items.get(i).map(|it| it.id) {
            Some(usize::MAX) => Action::Home(Home::New),
            Some(id) if id == usize::MAX - 2 => {
                self.tab = Tab::Search;
                self.level = Level::Body;
                Action::None
            }
            Some(id) if id == usize::MAX - 1 => Action::Home(Home::Settings),
            Some(id) => Action::Command(id),
            None => Action::None,
        }
    }

    fn threads_key(&mut self, k: KeyEvent) -> Action {
        let vis = self.visible();
        let n = vis.len();
        match k.code {
            KeyCode::Up => self.tcursor = self.tcursor.saturating_sub(1),
            KeyCode::Down if n > 0 => self.tcursor = (self.tcursor + 1).min(n - 1),
            KeyCode::PageUp => self.tcursor = self.tcursor.saturating_sub(5),
            KeyCode::PageDown if n > 0 => self.tcursor = (self.tcursor + 5).min(n - 1),
            KeyCode::Enter => {
                return vis.get(self.tcursor).map(|&i| Action::Open(self.cards[i].clone())).unwrap_or(Action::None);
            }
            _ => {
                let before = self.filter.text();
                self.filter.key(k);
                if self.filter.text() != before {
                    self.tcursor = 0;
                    self.toffset = 0;
                }
            }
        }
        Action::None
    }



    /// A click: a tab label switches, a row runs (agents: selects; remove needs enter). Outside: close.
    pub fn mouse(&mut self, m: MouseEvent) -> Action {
        self.poll();
        let at = Position { x: m.column, y: m.row };
        // the filter / search input: its own mouse (cursor, selection + copy)
        let input = match self.tab {
            Tab::Threads => Some(&mut self.filter),
            Tab::Search => Some(&mut self.search),
            _ => None,
        };
        if let Some(ta) = input
            && ta.mouse(m)
        {
            ta.copied = None;
            self.level = Level::Body;
            return Action::None;
        }
        match m.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                // a repo tab of the threads tab
                if self.tab == Tab::Threads && self.repos.click(at) {
                    self.level = Level::Group;
                    self.repo_pick = self.repo_tabs().get(self.repos.i).cloned();
                    self.tcursor = 0;
                    self.toffset = 0;
                    return Action::None;
                }
                if !self.frame.contains(at) {
                    return Action::Close;
                }
                if let Some((_, t)) = self.tab_rects.iter().find(|(r, _)| r.contains(at)) {
                    self.tab = *t;
                    self.level = Level::Bar;
                    return Action::None;
                }
                if self.tab == Tab::Merge {
                    self.level = if self.prs.tab_at(at) { Level::Group } else { Level::Body };
                    return match self.prs.mouse(m) {
                        crate::prlist::Pick::Open(e) => Action::Pr(e),
                        _ => Action::None,
                    };
                }
                let Some(&(_, i)) = self.rows.iter().find(|(r, _)| r.contains(at)) else { return Action::None };
                self.level = Level::Body;
                match self.tab {
                    Tab::Quick => {
                        self.qcursor = i;
                        return self.run_item(i);
                    }
                    Tab::Threads => {
                        let vis = self.visible();
                        if let Some(p) = vis.iter().position(|&v| v == i) {
                            self.tcursor = p;
                        }
                        return Action::Open(self.cards[i].clone());
                    }
                    Tab::Search => {
                        self.scursor = i;
                        return self.jump(i);
                    }
                    Tab::Tokens | Tab::Merge => {}
                }
            }
            MouseEventKind::ScrollDown if self.tab == Tab::Threads => {
                let n = self.visible().len();
                if n > 0 {
                    self.tcursor = (self.tcursor + 1).min(n - 1);
                }
            }
            MouseEventKind::ScrollUp if self.tab == Tab::Threads => self.tcursor = self.tcursor.saturating_sub(1),
            MouseEventKind::ScrollDown | MouseEventKind::ScrollUp if self.tab == Tab::Merge => {
                self.prs.mouse(m);
            }
            MouseEventKind::ScrollDown if self.tab == Tab::Search => self.scursor = (self.scursor + 1).min(self.hits.len().saturating_sub(1)),
            MouseEventKind::ScrollUp if self.tab == Tab::Search => self.scursor = self.scursor.saturating_sub(1),
            MouseEventKind::ScrollDown if self.tab == Tab::Tokens => self.dscroll = self.dscroll.saturating_add(1),
            MouseEventKind::ScrollUp if self.tab == Tab::Tokens => self.dscroll = self.dscroll.saturating_sub(1),
            _ => {}
        }
        Action::None
    }

    /// Row under AT (hover).
    pub fn row_at(&self, at: Position) -> Option<Rect> {
        self.rows.iter().find(|(r, _)| r.contains(at)).map(|(r, _)| *r).or_else(|| self.tab_rects.iter().find(|(r, _)| r.contains(at)).map(|(r, _)| *r))
    }

    /// The menu in AREA (the whole screen), as a command palette: centered on the screen's
    /// width, its top on the row below the header row (row 0).
    pub fn render(&mut self, f: &mut Frame, area: Rect) {
        self.poll();
        self.poll_detail();
        // wide screens: up to 100 columns (45% of the width), 76 at least
        let w = area.width.saturating_sub(4).min((area.width * 45 / 100).clamp(76, 100));
        let top = area.y + 1.min(area.height.saturating_sub(8));
        let h = area.bottom().saturating_sub(top + 1).min(32).max(8.min(area.height));
        let r = Rect { x: area.x + (area.width - w) / 2, y: top, width: w, height: h };
        self.frame = r;
        // a modal: the screen behind it fades
        theme::dim(f.buffer_mut(), area);
        f.render_widget(Clear, r);
        let block = Block::new()
            .borders(Borders::ALL)
            .border_type(BorderType::Plain)
            .border_style(theme::accent())
            .title(Span::styled(" menu ", theme::accent_bold()))
            .style(theme::text());
        let inner = block.inner(r);
        f.render_widget(block, r);
        let inner = Rect { x: inner.x + 1, width: inner.width.saturating_sub(2), ..inner };
        if inner.height < 4 {
            return;
        }
        // tab bar
        let mut x = inner.x;
        self.tab_rects.clear();
        for (t, label) in TABS {
            let text = format!(" {label} ");
            let wd = text.chars().count() as u16;
            let st = match (t == self.tab, self.level) {
                (true, Level::Bar) => theme::pill().add_modifier(Modifier::BOLD),
                (true, _) => theme::panel().add_modifier(Modifier::BOLD),
                _ => theme::muted(),
            };
            let rr = Rect { x, y: inner.y, width: wd.min(inner.right().saturating_sub(x)), height: 1 };
            f.render_widget(Paragraph::new(Span::styled(text, st)), rr);
            self.tab_rects.push((rr, t));
            x += wd + 1;
        }
        let body = Rect { y: inner.y + 2, height: inner.height.saturating_sub(3), ..inner };
        let foot = Rect { y: inner.bottom() - 1, height: 1, ..inner };
        self.rows.clear();
        let hints: &[(&str, &str)] = match self.tab {
            Tab::Quick => {
                self.draw_quick(f, body);
                &[("↑↓", "move"), ("enter", "run"), ("←→", "tabs"), ("esc", "close")]
            }
            Tab::Threads => {
                self.draw_threads(f, body);
                match self.level {
                    Level::Bar => &[("←→", "tabs"), ("↓", "repos"), ("type", "filter"), ("esc", "close")],
                    Level::Group => &[("←→", "repo"), ("↑", "tabs"), ("↓", "threads"), ("esc", "close")],
                    Level::Body => &[("type", "filter"), ("←→", "repo"), ("↑↓", "move"), ("enter", "open"), ("tab", "tabs"), ("esc", "close")],
                }
            }
            Tab::Merge => {
                self.prs.draw_levels(f, body, self.level == Level::Group, self.level == Level::Body);
                match self.level {
                    Level::Bar => &[("←→", "tabs"), ("↓", "states"), ("esc", "close")],
                    Level::Group => &[("←→", "state"), ("↑", "tabs"), ("↓", "list"), ("esc", "close")],
                    Level::Body => &[("←→", "state"), ("↑↓", "move"), ("enter", "open: its thread, else the merge tab"), ("tab", "tabs"), ("esc", "close")],
                }
            }
            Tab::Search => {
                self.draw_search(f, body);
                &[("type", "search"), ("↑↓", "move"), ("enter", "jump to it"), ("←→", "tabs"), ("esc", "close")]
            }
            Tab::Tokens => {
                self.draw_tokens(f, body);
                &[("↑↓", "scroll"), ("r", "reload"), ("←→", "tabs"), ("esc", "close")]
            }
        };
        let mut spans = Vec::new();
        for (k, l) in hints {
            spans.push(Span::styled(format!("{k} "), theme::accent()));
            spans.push(Span::styled(format!("{l}   "), theme::muted()));
        }
        f.render_widget(Paragraph::new(Line::from(spans)), foot);
    }

    fn row_style(hl: bool) -> (Style, Style) {
        if hl {
            (Style::default().fg(theme::pal().on_accent).add_modifier(Modifier::BOLD), Style::default().fg(theme::pal().on_accent))
        } else {
            (theme::accent(), theme::text())
        }
    }

    fn draw_quick(&mut self, f: &mut Frame, body: Rect) {
        let kw = self.items.iter().map(|i| i.key.chars().count()).max().unwrap_or(0);
        // per section: a blank row (after the 1st), its heading, its items
        let mut y = body.y;
        let mut last: Option<&str> = None;
        for (i, it) in self.items.iter().enumerate() {
            if last != Some(it.section) {
                if last.is_some() {
                    y += 1;
                }
                last = Some(it.section);
                if y < body.bottom() {
                    f.render_widget(Paragraph::new(Span::styled(it.section.to_string(), theme::muted().add_modifier(Modifier::BOLD))), Rect { y, height: 1, ..body });
                }
                y += 1;
            }
            if y >= body.bottom() {
                break;
            }
            let row_y = y;
            y += 1;
            let y = row_y;
            let r = Rect { y, height: 1, ..body };
            let hl = i == self.qcursor;
            let (ks, ls) = Self::row_style(hl);
            let label = nav::fit(&it.label, (r.width as usize).saturating_sub(kw + 4));
            let line = Line::from(vec![Span::styled(format!(" {:<kw$}  ", it.key), ks), Span::styled(label, ls)]);
            f.render_widget(Paragraph::new(line).style(if hl { theme::pill() } else { Style::default() }), r);
            self.rows.push((r, i));
        }
    }

    fn draw_threads(&mut self, f: &mut Frame, body: Rect) {
        // the name filter, the repo tabs below it
        let fr = Rect { height: 1, ..body };
        f.render_widget(Paragraph::new(Span::styled("› ", theme::accent())), Rect { width: 2, ..fr });
        self.filter.render(f, Rect { x: fr.x + 2, width: fr.width.saturating_sub(2), ..fr }, true);
        let tabs = self.repo_tabs();
        let cur = self.repo_tab();
        self.repos.i = tabs.iter().position(|t| *t == cur).unwrap_or(0);
        let labels: Vec<String> = tabs.iter().map(RepoTab::label).collect();
        self.repos.draw(f, Rect { y: body.y + 1, height: 1, ..body }, "repo", &labels, self.level == Level::Group);
        let list = Rect { y: body.y + 3, height: body.height.saturating_sub(3), ..body };
        let vis = self.visible();
        if vis.is_empty() {
            let msg = if self.loading.is_some() && self.cards.is_empty() { "reading threads…" } else if cur != RepoTab::All { "0 threads in this repo" } else { "0 threads" };
            f.render_widget(Paragraph::new(Span::styled(msg, theme::muted())), Rect { height: 1, ..list });
            return;
        }
        self.tcursor = self.tcursor.min(vis.len() - 1);
        let fits = (list.height / nav::CARD_H).max(1) as usize;
        if self.tcursor < self.toffset {
            self.toffset = self.tcursor;
        }
        if self.tcursor >= self.toffset + fits {
            self.toffset = self.tcursor + 1 - fits;
        }
        let tick = (self.started.elapsed().as_millis() / nav::WAVE_MS) as usize;
        let notes = crate::notify::unseen(self.thread.as_deref());
        for (slot, &i) in vis.iter().enumerate().skip(self.toffset).take(fits) {
            let y = list.y + (slot - self.toffset) as u16 * nav::CARD_H;
            let r = Rect { y, height: nav::CARD_H, ..list };
            let c = &self.cards[i];
            nav::draw_card(f.buffer_mut(), r, c, slot == self.tcursor && self.level == Level::Body, self.thread.as_deref() == Some(c.id.as_str()), true, tick);
            crate::notify::card_badge(f.buffer_mut(), r, &notes, &c.id);
            self.rows.push((r, i));
        }
        self.cards_fx.apply(f, list, self.cards_version);
        let more = vis.len().saturating_sub(self.toffset + fits);
        if self.toffset > 0 || more > 0 {
            let text = format!("{}↑ · {more}↓", self.toffset);
            f.render_widget(Paragraph::new(Span::styled(text, theme::muted())).right_aligned(), fr);
        }
    }

    /// Rows per search hit: who · thread · time, the snippet, a gap.
    const HIT_H: u16 = 3;

    fn draw_search(&mut self, f: &mut Frame, body: Rect) {
        let fr = Rect { height: 1, ..body };
        f.render_widget(Paragraph::new(Span::styled("› ", theme::accent())), Rect { width: 2, ..fr });
        self.search.render(f, Rect { x: fr.x + 2, width: fr.width.saturating_sub(2), ..fr }, true);
        let list = Rect { y: body.y + 2, height: body.height.saturating_sub(2), ..body };
        let q = self.search.text();
        let Some(msgs) = self.msgs.as_ref() else {
            f.render_widget(Paragraph::new(Span::styled("reading messages…", theme::muted())), Rect { height: 1, ..list });
            return;
        };
        if self.hits.is_empty() {
            let msg = if q.trim().is_empty() { format!("{} messages · type to search", msgs.len()) } else { "0 messages match".into() };
            f.render_widget(Paragraph::new(Span::styled(msg, theme::muted())), Rect { height: 1, ..list });
            return;
        }
        let n = self.hits.len();
        self.scursor = self.scursor.min(n - 1);
        let fits = (list.height / Self::HIT_H).max(1) as usize;
        if self.scursor < self.soffset {
            self.soffset = self.scursor;
        }
        if self.scursor >= self.soffset + fits {
            self.soffset = self.scursor + 1 - fits;
        }
        let w = list.width as usize;
        let mut rows: Vec<(Rect, usize)> = Vec::new();
        for (slot, &mi) in self.hits.iter().enumerate().skip(self.soffset).take(fits) {
            let m = &msgs[mi];
            let y = list.y + (slot - self.soffset) as u16 * Self::HIT_H;
            let r = Rect { y, height: 2, ..list };
            let hl = slot == self.scursor;
            let bg = if hl { Style::default().bg(theme::pal().surface0) } else { Style::default() };
            let who = if m.from == "owner" { "you".to_string() } else { m.from.clone() };
            let here = self.thread.as_deref() == Some(m.thread.as_str());
            let place = if here { "this thread".to_string() } else { m.title.clone() };
            let when = format!("{} · {}", crate::stream::clock(m.ts), nav::ago(m.ts));
            let room = w.saturating_sub(who.chars().count() + when.chars().count() + 8);
            let mark = if hl { "▌ " } else { "  " };
            let head = Line::from(vec![
                Span::styled(mark, theme::accent()),
                Span::styled(who, if m.from == "owner" { theme::text().add_modifier(Modifier::BOLD) } else { Style::default().fg(crate::agents::color(self.roster.iter().find(|a| a.name == m.from).and_then(|a| a.color.as_deref()))).add_modifier(Modifier::BOLD) }),
                Span::styled(format!(" · {} · ", nav::fit(&place, room)), theme::muted()),
                Span::styled(when, theme::muted()),
            ]);
            let (snip, marks) = crate::search::snippet(m, &q, w.saturating_sub(2));
            let mut body_spans = vec![Span::styled(mark, theme::accent())];
            body_spans.extend(crate::fuzzy::spans(&snip, &marks, theme::text(), theme::accent_bold().add_modifier(Modifier::UNDERLINED)));
            f.render_widget(Paragraph::new(vec![head, Line::from(body_spans)]).style(bg), r);
            rows.push((r, slot));
        }
        self.rows = rows;
        let more = n.saturating_sub(self.soffset + fits);
        let cap = if n >= crate::search::MAX { "+" } else { "" };
        let text = format!("{n}{cap} · {}↑ · {more}↓", self.soffset);
        f.render_widget(Paragraph::new(Span::styled(text, theme::muted())).right_aligned(), Rect { y: body.y + 1, height: 1, ..body });
    }

    fn draw_tokens(&mut self, f: &mut Frame, body: Rect) {
        let Some(d) = &self.detail else {
            f.render_widget(Paragraph::new(Span::styled("reading usage…", theme::muted())), Rect { height: 1, ..body });
            return;
        };
        let names = self.roster.iter().map(|a| (a.name.clone(), (a.name.clone(), a.color.clone()))).collect();
        let titles = self.cards.iter().map(|c| (c.id.clone(), c.name())).collect();
        let lines = crate::cost::detail_lines(d, self.thread.as_deref(), &names, &titles, body.width as usize);
        let max = (lines.len() as u16).saturating_sub(body.height);
        self.dscroll = self.dscroll.min(max);
        f.render_widget(Paragraph::new(lines).scroll((self.dscroll, 0)), body);
        if max > 0 {
            let text = format!("{}↑ · {}↓", self.dscroll, max - self.dscroll);
            f.render_widget(Paragraph::new(Span::styled(text, theme::muted())).right_aligned(), Rect { y: body.y.saturating_sub(2), height: 1, ..body });
        }
    }

}

/// A repo tab of the threads tab.
#[derive(Clone, Debug, PartialEq)]
enum RepoTab {
    Repo(std::path::PathBuf),
    Outside,
    All,
}

impl RepoTab {
    fn keeps(&self, repo: Option<&std::path::Path>) -> bool {
        match self {
            RepoTab::Repo(p) => repo == Some(p.as_path()),
            RepoTab::Outside => repo.is_none(),
            RepoTab::All => true,
        }
    }

    fn label(&self) -> String {
        match self {
            RepoTab::Repo(p) => p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| p.display().to_string()),
            RepoTab::Outside => "outside git".into(),
            RepoTab::All => "all".into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::crossterm::event::KeyModifiers;

    fn k(c: KeyCode) -> KeyEvent {
        KeyEvent::new(c, KeyModifiers::NONE)
    }

    fn card(id: &str, title: &str) -> Card {
        Card { id: id.into(), title: title.into(), state: "open".into(), ..Default::default() }
    }

    fn agent(name: &str) -> Agent {
        Agent { name: name.into(), harness: "claude".into(), model: None, color: None, mode: String::new(), prompt: None, last_seen: None }
    }

    fn menu() -> Menu {
        let items = vec![Item { label: "keys".into(), key: "?".into(), id: 7, section: "help" }];
        Menu::new(items, Some("t1".into()), vec![agent("amon"), agent("lilith")], vec![card("t1", "first"), card("t2", "deploy fix")])
    }

    #[test]
    fn tabs_cycle_and_esc_closes() {
        let mut m = menu();
        assert_eq!(m.tab, Tab::Quick);
        m.key(k(KeyCode::Right));
        assert_eq!((m.tab, m.level), (Tab::Threads, Level::Bar));
        // the tab bar keeps the keys: ←→ walk past the tabs with a tab group
        m.key(k(KeyCode::Right));
        assert_eq!(m.tab, Tab::Merge);
        m.key(k(KeyCode::Left));
        assert_eq!(m.tab, Tab::Threads);
        // ↓: the repo tabs (←→ there), ↓: the list; ↑ at its top: the repo tabs, ↑: the tab bar
        m.key(k(KeyCode::Down));
        assert_eq!(m.level, Level::Group);
        m.key(k(KeyCode::Down));
        assert_eq!(m.level, Level::Body);
        m.key(k(KeyCode::Right));
        assert_eq!(m.tab, Tab::Threads, "the list: ←→ walk the repos");
        m.key(k(KeyCode::Up));
        assert_eq!(m.level, Level::Group);
        m.key(k(KeyCode::Up));
        assert_eq!(m.level, Level::Bar);
        m.key(k(KeyCode::Right));
        assert_eq!(m.tab, Tab::Merge);
        // merge: the same 3 levels; tab switches the tabs from any level
        m.key(k(KeyCode::Down));
        m.key(k(KeyCode::Down));
        assert_eq!(m.level, Level::Body);
        m.key(k(KeyCode::Up));
        m.key(k(KeyCode::Up));
        assert_eq!(m.level, Level::Bar);
        m.key(k(KeyCode::Down));
        m.key(k(KeyCode::Tab));
        assert_eq!((m.tab, m.level), (Tab::Search, Level::Bar));
        m.key(k(KeyCode::Right));
        assert_eq!(m.tab, Tab::Tokens);
        // 0 agents tab (the agents box of the thread view, ctrl+q, has their actions)
        m.key(k(KeyCode::Right));
        assert_eq!(m.tab, Tab::Quick);
        m.key(k(KeyCode::Left));
        assert_eq!(m.tab, Tab::Tokens);
        assert_eq!(m.key(k(KeyCode::Esc)), Action::Close);
        // the menu key closes it too; tab: the next menu tab (0 close)
        assert_eq!(m.key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL)), Action::Close);
        assert_ne!(m.key(k(KeyCode::Tab)), Action::Close);
    }

    #[test]
    fn a_command_palette_below_the_header_centered_on_wide_screens() {
        let mut m = menu();
        for (w, h) in [(240u16, 60u16), (90, 30)] {
            let mut t = ratatui::Terminal::new(ratatui::backend::TestBackend::new(w, h)).unwrap();
            t.draw(|f| m.render(f, f.area())).unwrap();
            let r = m.frame;
            assert_eq!(r.y, 1, "below the header row");
            let (left, right) = (r.x, w - r.right());
            assert!(left.abs_diff(right) <= 1, "centered: {r:?} in {w}");
            assert!(r.width <= 100 && r.width >= 76.min(w - 4), "{r:?}");
        }
    }

    #[test]
    fn quick_commands_run() {
        let mut m = menu();
        assert_eq!(m.key(k(KeyCode::Enter)), Action::Home(Home::New));
        m.key(k(KeyCode::Down));
        assert_eq!(m.key(k(KeyCode::Enter)), Action::Home(Home::Settings));
        m.key(k(KeyCode::Down));
        assert_eq!(m.key(k(KeyCode::Enter)), Action::None);
        assert_eq!(m.tab, Tab::Search);
        m.tab = Tab::Quick;
        m.key(k(KeyCode::Down));
        assert_eq!(m.key(k(KeyCode::Enter)), Action::Command(7));
        // the last row: ↓ stays; ↑ at the 1st row: the tab bar
        m.key(k(KeyCode::Down));
        assert_eq!(m.key(k(KeyCode::Enter)), Action::Command(7));
        for _ in 0..4 {
            m.key(k(KeyCode::Up));
        }
        assert_eq!(m.level, Level::Bar);
        m.key(k(KeyCode::Down));
        assert_eq!(m.key(k(KeyCode::Enter)), Action::Home(Home::New));
    }

    #[test]
    fn search_finds_messages_moves_and_jumps() {
        use crate::search::Msg;
        let mut m = menu();
        // f: the search tab, its filter takes every letter (f too)
        m.key(k(KeyCode::Char('f')));
        assert_eq!(m.tab, Tab::Search);
        m.set_msgs(vec![Msg::new("t1", "first", "owner", 10, "fix the deploy"), Msg::new("t2", "deploy fix", "ada", 20, "deploy runs, the deploy is fixed")]);
        for c in "deploy".chars() {
            m.key(k(KeyCode::Char(c)));
        }
        // 2 hits for the 2nd message: it ranks first
        assert!(matches!(m.key(k(KeyCode::Enter)), Action::Jump(c, 20) if c.id == "t2"));
        m.key(k(KeyCode::Down));
        assert!(matches!(m.key(k(KeyCode::Enter)), Action::Jump(c, 10) if c.id == "t1"));
        let s = {
            let mut t = ratatui::Terminal::new(ratatui::backend::TestBackend::new(90, 30)).unwrap();
            t.draw(|f| m.render(f, f.area())).unwrap();
            let b = t.backend().buffer().clone();
            (0..30).map(|y| (0..90).map(|x| b[(x, y)].symbol().to_string()).collect::<String>()).collect::<Vec<_>>().join("\n")
        };
        assert!(s.contains(" search ") && s.contains("you · this thread") && s.contains("ada · deploy fix") && s.contains("fix the deploy"), "{s}");
        m.key(k(KeyCode::Char('x')));
        assert!(matches!(m.key(k(KeyCode::Enter)), Action::None));
    }

    #[test]
    fn live_cards_keep_the_cursor_on_its_thread() {
        let mut m = Menu::new(Vec::new(), None, Vec::new(), vec![card("t1", "a"), card("t2", "b"), card("t3", "c")]);
        m.tab = Tab::Threads;
        m.key(k(KeyCode::Down));
        // a read: t3 works now, it moves to the top; the cursor stays on t2
        let mut busy = card("t3", "c");
        busy.busy = true;
        m.set_cards(vec![busy, card("t1", "a"), card("t2", "b")]);
        assert!(m.cards[0].busy);
        assert!(matches!(m.key(k(KeyCode::Enter)), Action::Open(c) if c.id == "t2"));
    }

    #[test]
    fn live_messages_keep_the_cursor_on_its_message() {
        use crate::search::Msg;
        let mut m = menu();
        m.key(k(KeyCode::Char('f')));
        m.set_msgs(vec![Msg::new("t1", "a", "owner", 10, "deploy one"), Msg::new("t2", "b", "ada", 20, "deploy two")]);
        for c in "deploy".chars() {
            m.key(k(KeyCode::Char(c)));
        }
        m.key(k(KeyCode::Down));
        let Action::Jump(c, ts) = m.key(k(KeyCode::Enter)) else { panic!("0 hit") };
        // a read with a new message: the cursor stays on the same message
        m.set_msgs(vec![Msg::new("t1", "a", "owner", 10, "deploy one"), Msg::new("t2", "b", "ada", 20, "deploy two"), Msg::new("t3", "c", "ada", 30, "deploy three")]);
        assert_eq!(m.hits.len(), 3);
        assert!(matches!(m.key(k(KeyCode::Enter)), Action::Jump(d, t) if d.id == c.id && t == ts));
    }

    #[test]
    fn threads_filter_and_open() {
        let mut m = menu();
        m.key(k(KeyCode::Right));
        m.key(k(KeyCode::Down));
        m.key(k(KeyCode::Down));
        m.key(k(KeyCode::Down));
        assert!(matches!(m.key(k(KeyCode::Enter)), Action::Open(c) if c.id == "t2"));
        for c in "first".chars() {
            m.key(k(KeyCode::Char(c)));
        }
        assert!(matches!(m.key(k(KeyCode::Enter)), Action::Open(c) if c.id == "t1"));
    }

    #[test]
    fn threads_tab_has_the_current_thread_on_top() {
        let cards = vec![card("t1", "newest"), card("t2", "older"), card("t3", "this one")];
        let mut m = Menu::new(Vec::new(), Some("t3".into()), Vec::new(), cards);
        m.key(k(KeyCode::Right));
        // a letter on the tab bar: the filter takes it (here: enter dives into the list)
        m.key(k(KeyCode::Enter));
        m.key(k(KeyCode::Enter));
        let ids = |m: &Menu| m.visible().iter().map(|&i| m.cards[i].id.clone()).collect::<Vec<_>>();
        assert_eq!(ids(&m), ["t3", "t1", "t2"]);
        assert!(matches!(m.key(k(KeyCode::Enter)), Action::Open(c) if c.id == "t3"));
        // a filter that keeps it: on top too; one that drops it: the rest in order
        for c in "o".chars() {
            m.key(k(KeyCode::Char(c)));
        }
        assert_eq!(ids(&m)[0], "t3");
        for c in "lder".chars() {
            m.key(k(KeyCode::Char(c)));
        }
        assert_eq!(ids(&m), ["t2"]);
    }

    #[test]
    fn threads_repo_tabs_start_on_the_current_threads_repo() {
        let repo = |c: &str, r: &str| Card { repo: Some(r.into()), ..card(c, c) };
        let cards = vec![repo("t1", "/x/alpha"), repo("b1", "/x/beta"), repo("b2", "/x/beta"), repo("g1", "/x/gamma"), card("o1", "o1")];
        let mut m = Menu::new(Vec::new(), Some("t1".into()), Vec::new(), cards);
        m.key(k(KeyCode::Right));
        assert_eq!(m.tab, Tab::Threads);
        m.key(k(KeyCode::Down));
        let draw = |m: &mut Menu| {
            let mut t = ratatui::Terminal::new(ratatui::backend::TestBackend::new(90, 30)).unwrap();
            t.draw(|f| m.render(f, f.area())).unwrap();
            let b = t.backend().buffer().clone();
            (0..30).map(|y| (0..90).map(|x| b[(x, y)].symbol()).collect::<String>()).collect::<Vec<_>>()
        };
        // the current thread's repo 1st, then the others by thread count, outside git, all
        let s = draw(&mut m);
        let row = s.iter().find(|l| l.contains("repo ")).expect("repo tabs");
        let at = |t: &str| row.find(t).unwrap_or_else(|| panic!("{t}: {row}"));
        assert!(at(" alpha ") < at(" beta ") && at(" beta ") < at(" gamma ") && at(" gamma ") < at(" outside git ") && at(" outside git ") < at(" all "), "{row}");
        assert!(!s.join("\n").contains("ctrl+r"));
        let ids = |m: &Menu| m.visible().iter().map(|&i| m.cards[i].id.clone()).collect::<Vec<_>>();
        assert_eq!(ids(&m), ["t1"], "the current thread's repo by default");
        // →: beta (2 threads); the name filter applies on top
        m.key(k(KeyCode::Right));
        assert_eq!(ids(&m), ["b1", "b2"]);
        m.key(k(KeyCode::Char('2')));
        assert!(matches!(m.key(k(KeyCode::Enter)), Action::Open(c) if c.id == "b2"));
        m.key(k(KeyCode::Backspace));
        // ←← wraps: all
        m.key(k(KeyCode::Left));
        m.key(k(KeyCode::Left));
        assert_eq!(ids(&m).len(), 5);
        // a click on a tab picks it
        draw(&mut m);
        let r = m.repos.rect_of(1).expect("tab 1 drawn");
        m.mouse(MouseEvent { kind: MouseEventKind::Down(MouseButton::Left), column: r.x, row: r.y, modifiers: KeyModifiers::NONE });
        assert_eq!(ids(&m), ["b1", "b2"]);
    }

    #[test]
    #[ignore = "reads the real threads"]
    fn live_threads_tab() {
        let cards = nav::load();
        let mut m = Menu::new(Vec::new(), cards.first().map(|c| c.id.clone()), crate::agents::cached(), cards);
        m.key(k(KeyCode::Right));
        let mut t = ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 34)).unwrap();
        t.draw(|f| m.render(f, f.area())).unwrap();
        let b = t.backend().buffer().clone();
        let s: Vec<String> = (0..34).map(|y| (0..100).map(|x| b[(x, y)].symbol().to_string()).collect()).collect();
        println!("{}", s.join("\n"));
        m.key(k(KeyCode::Right));
        m.detail = Some(crate::cost::detail(m.thread.as_deref()));
        t.draw(|f| m.render(f, f.area())).unwrap();
        let b = t.backend().buffer().clone();
        let s: Vec<String> = (0..34).map(|y| (0..100).map(|x| b[(x, y)].symbol().to_string()).collect()).collect();
        println!("{}", s.join("\n"));
    }
}

#[cfg(test)]
mod render_tests {
    use super::*;
    use ratatui::crossterm::event::KeyModifiers;

    fn agent(name: &str, harness: &str) -> Agent {
        Agent { name: name.into(), harness: harness.into(), model: None, color: None, mode: String::new(), prompt: None, last_seen: None }
    }

    fn screen(m: &mut Menu) -> String {
        let mut t = ratatui::Terminal::new(ratatui::backend::TestBackend::new(90, 34)).unwrap();
        t.draw(|f| m.render(f, f.area())).unwrap();
        let b = t.backend().buffer().clone();
        let s = (0..34).map(|y| (0..90).map(|x| b[(x, y)].symbol().to_string()).collect::<String>()).collect::<Vec<_>>().join("\n");
        if std::env::var_os("SHOW").is_some() {
            eprintln!("{s}");
        }
        s
    }

    /// 0 agents tab (the thread view's agents box, ctrl+q, has their actions); n starts a thread.
    #[test]
    fn the_menu_has_0_agents_tab_and_n_starts_a_thread() {
        let roster = vec![agent("amon", "claude"), agent("nova", "codex")];
        let mut m = Menu::new(Vec::new(), Some("t1".into()), roster, vec![Card { id: "t1".into(), ..Default::default() }]);
        let s = screen(&mut m);
        let bar = s.lines().find(|l| l.contains("tokens")).unwrap_or_default();
        assert!(bar.contains("quick") && !bar.contains("agents"), "{s}");
        assert_eq!(m.key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE)), Action::Home(Home::New));
    }

}
