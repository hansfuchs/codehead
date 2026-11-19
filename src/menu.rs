//! F2 menu: 1 modal for the start screen and the thread view. Tabs: quick (commands),
//! threads (filter + thread cards), search (the messages of every thread), tokens, agents (the agents of the thread in view: remove, add; edit →
//! the agents tab of the start screen). ↑↓ move, ←→: next / previous tab, enter: run,
//! a single esc or tab closes.

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
    Search,
    Tokens,
    Agents,
}

const TABS: [(Tab, &str); 5] = [(Tab::Quick, "quick"), (Tab::Threads, "threads"), (Tab::Search, "search"), (Tab::Tokens, "tokens"), (Tab::Agents, "agents")];

/// A tab of the start screen the menu opens.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Home {
    New,
    Settings,
    Agents,
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
    Remove(String),
    Add(String),
}

/// Quick command of the host: label, hotkey hint, id, section (a heading row over each section).
pub struct Item {
    pub label: String,
    pub key: String,
    pub id: usize,
    pub section: &'static str,
}

/// Agents tab rows: the edit button, the agents of the thread, the add button.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Row {
    Edit,
    Member(usize),
    Add,
}

pub struct Menu {
    pub tab: Tab,
    items: Vec<Item>,
    qcursor: usize,
    filter: TextArea,
    cards: Vec<Card>,
    loading: Option<Receiver<Vec<Card>>>,
    tcursor: usize,
    toffset: usize,
    /// Thread in view (None: start screen).
    thread: Option<String>,
    members: Vec<String>,
    roster: Vec<Agent>,
    acursor: usize,
    /// Agents tab: 1st virtual row of the card grid in view.
    aoffset: usize,
    /// Effort of claude agents (the effort row of the cards).
    effort: String,
    /// Remove armed for this agent: a 2nd enter removes.
    armed: Option<String>,
    /// Add picker open: its cursor over the roster agents off the thread.
    adding: Option<usize>,
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
    hits: Vec<usize>,
    hits_for: String,
    scursor: usize,
    soffset: usize,
    /// Last draw: tab labels, rows (clicks).
    tab_rects: Vec<(Rect, Tab)>,
    rows: Vec<(Rect, usize)>,
    frame: Rect,
}

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
    pub fn new(items: Vec<Item>, thread: Option<String>, members: Vec<String>, roster: Vec<Agent>, cards: Vec<Card>) -> Menu {
        // tests: 0 thread reads (they write the seen file)
        let loading = (cards.is_empty() && !cfg!(test)).then(|| {
            let (tx, rx) = mpsc::channel();
            std::thread::spawn(move || {
                let _ = tx.send(nav::load());
            });
            rx
        });
        let mut all = builtins();
        all.extend(items);
        let mut filter = TextArea::new(true, "filter threads");
        filter.style = Some(theme::text());
        let mut search = TextArea::new(true, "search messages: you + agents, every thread");
        search.style = Some(theme::text());
        Menu {
            tab: Tab::Quick,
            items: all,
            qcursor: 0,
            filter,
            cards,
            loading,
            tcursor: 0,
            toffset: 0,
            thread,
            members,
            roster,
            acursor: 0,
            aoffset: 0,
            effort: crate::agents::effort_name(),
            armed: None,
            adding: None,
            started: Instant::now(),
            detail: None,
            detail_rx: None,
            detail_at: None,
            dscroll: 0,
            search,
            msgs: None,
            msgs_rx: None,
            hits: Vec::new(),
            hits_for: String::new(),
            scursor: 0,
            soffset: 0,
            tab_rects: Vec::new(),
            rows: Vec::new(),
            frame: Rect::default(),
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

    fn poll(&mut self) {
        if let Some(rx) = &self.loading
            && let Ok(c) = rx.try_recv()
        {
            self.cards = c;
            self.loading = None;
        }
        if let Some(rx) = &self.msgs_rx
            && let Ok(m) = rx.try_recv()
        {
            self.msgs = Some(m);
            self.msgs_rx = None;
            self.hits_for = String::from("\0");
        }
        // the search tab: the messages read once, on its 1st show
        if self.tab == Tab::Search && self.msgs.is_none() && self.msgs_rx.is_none() && !cfg!(test) {
            let (tx, rx) = mpsc::channel();
            std::thread::spawn(move || {
                let _ = tx.send(crate::search::load());
            });
            self.msgs_rx = Some(rx);
        }
        self.refresh_hits();
    }

    /// Tests: messages to search (0 read of the real threads).
    #[cfg(test)]
    pub fn set_msgs(&mut self, m: Vec<crate::search::Msg>) {
        self.msgs = Some(m);
        self.hits_for = String::from("\0");
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
            KeyCode::Up if n > 0 => self.scursor = (self.scursor + n - 1) % n,
            KeyCode::Down if n > 0 => self.scursor = (self.scursor + 1) % n,
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

    fn visible(&self) -> Vec<usize> {
        nav::filter(&self.cards, &self.filter.text())
    }

    /// The agents of the thread (an agent the roster lacks: 0 harness), index = index in `members`.
    fn member_agents(&self) -> Vec<Agent> {
        self.members
            .iter()
            .map(|n| self.roster.iter().find(|a| a.name == *n).cloned().unwrap_or_else(|| Agent { name: n.clone(), harness: "?".into(), model: None, color: None, mode: String::new(), prompt: None, last_seen: None }))
            .collect()
    }

    /// Edit, the agents of the thread (by harness, A→Z, as the grid shows them), add.
    fn agent_rows(&self) -> Vec<Row> {
        let mut v = vec![Row::Edit];
        if self.thread.is_some() {
            let groups = crate::cards::groups(&self.member_agents(), |_| true);
            v.extend(groups.iter().flat_map(|(_, g)| g.iter().map(|&i| Row::Member(i))));
            v.push(Row::Add);
        }
        v
    }

    /// Roster agents off the thread (the add picker), by harness, A→Z.
    fn addable(&self) -> Vec<Agent> {
        let off: Vec<Agent> = self.roster.iter().filter(|a| !self.members.contains(&a.name)).cloned().collect();
        let groups = crate::cards::groups(&off, |_| true);
        groups.iter().flat_map(|(_, g)| g.iter().map(|&i| off[i].clone())).collect()
    }

    /// Card width of the agents tab: as the agents tab of the start screen (text of every agent).
    fn card_width(&self) -> u16 {
        let rows: Vec<String> = self.roster.iter().chain(self.member_agents().iter()).flat_map(|a| [a.name.clone(), crate::cards::tech(a), crate::cards::effort(a, &self.effort)]).collect();
        crate::cards::card_width(crate::cards::text_width(rows.iter()), true)
    }

    /// AGENTS (in their order) as a card grid by harness in AREA, CUR: the agent index with the
    /// cursor; CARD draws 1 (frame, area, agent index, cursor on it). Returns the drawn cards.
    fn draw_grid(&mut self, f: &mut Frame, area: Rect, agents: &[Agent], cur: Option<usize>, mut card: impl FnMut(&mut Frame, Rect, usize, bool)) -> Vec<(Rect, usize)> {
        let groups = crate::cards::groups(agents, |_| true);
        let g = crate::cards::tight_grid(&groups, self.card_width(), crate::cards::CARD_H, area.width);
        let view = area.height as usize;
        let top = cur.and_then(|c| g.places.iter().find(|p| p.0 == c)).map_or(0, |p| p.2);
        self.aoffset = if cur.is_some() { crate::cards::scroll(&g, top, crate::cards::CARD_H, self.aoffset, view) } else { 0 };
        let off = self.aoffset;
        let drawn = crate::cards::draw_grid(f, area, &g, crate::cards::CARD_H, off, |f, r, i| card(f, r, i, Some(i) == cur));
        let below = g.height.saturating_sub(off + view);
        if off > 0 || below > 0 {
            let text = format!("{off}↑ · {below}↓");
            f.render_widget(Paragraph::new(Span::styled(text, theme::muted())).right_aligned(), Rect { height: 1, ..area });
        }
        drawn
    }

    fn switch(&mut self, d: isize) {
        let i = TABS.iter().position(|(t, _)| *t == self.tab).unwrap_or(0) as isize;
        self.tab = TABS[(i + d).rem_euclid(TABS.len() as isize) as usize].0;
        self.armed = None;
        self.adding = None;
    }

    pub fn key(&mut self, k: KeyEvent) -> Action {
        self.poll();
        match k.code {
            // tab: closes (tab opens the menu in the thread view); ←/→ only switch the tabs
            KeyCode::Esc | KeyCode::Tab | KeyCode::BackTab => return Action::Close,
            _ if crate::config::menu_key().matches(&k) => return Action::Close,
            // ←/→: tabs only (0 run, 0 cursor in the threads filter)
            KeyCode::Right => {
                self.switch(1);
                return Action::None;
            }
            KeyCode::Left => {
                self.switch(-1);
                return Action::None;
            }
            _ => {}
        }
        match self.tab {
            Tab::Quick => self.quick_key(k),
            Tab::Threads => self.threads_key(k),
            Tab::Search => self.search_key(k),
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
            Tab::Agents => self.agents_key(k),
        }
    }

    fn quick_key(&mut self, k: KeyEvent) -> Action {
        let n = self.items.len();
        match k.code {
            KeyCode::Up | KeyCode::Char('k') => self.qcursor = (self.qcursor + n - 1) % n,
            KeyCode::Down | KeyCode::Char('j') => self.qcursor = (self.qcursor + 1) % n,
            KeyCode::Char('n') => return Action::Home(Home::New),
            KeyCode::Char('f') => self.tab = Tab::Search,
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
            KeyCode::Up if n > 0 => self.tcursor = (self.tcursor + n - 1) % n,
            KeyCode::Down if n > 0 => self.tcursor = (self.tcursor + 1) % n,
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

    fn agents_key(&mut self, k: KeyEvent) -> Action {
        if let Some(c) = self.adding {
            let names: Vec<String> = self.addable().iter().map(|a| a.name.clone()).collect();
            let n = names.len();
            match k.code {
                KeyCode::Up if n > 0 => self.adding = Some((c + n - 1) % n),
                KeyCode::Down if n > 0 => self.adding = Some((c + 1) % n),
                KeyCode::Backspace => self.adding = None,
                KeyCode::Enter => {
                    if let Some(name) = names.get(c) {
                        self.adding = None;
                        return Action::Add(name.clone());
                    }
                }
                _ => {}
            }
            return Action::None;
        }
        let rows = self.agent_rows();
        let n = rows.len();
        if !matches!(k.code, KeyCode::Enter | KeyCode::Char('d') | KeyCode::Delete) {
            self.armed = None;
        }
        match k.code {
            KeyCode::Up | KeyCode::Char('k') => self.acursor = (self.acursor + n - 1) % n,
            KeyCode::Down | KeyCode::Char('j') => self.acursor = (self.acursor + 1) % n,
            KeyCode::Enter | KeyCode::Char('d') | KeyCode::Delete => return self.activate(rows[self.acursor.min(n - 1)]),
            _ => {}
        }
        Action::None
    }

    fn activate(&mut self, row: Row) -> Action {
        match row {
            Row::Edit => Action::Home(Home::Agents),
            Row::Add => {
                self.adding = Some(0);
                Action::None
            }
            Row::Member(i) => {
                let Some(name) = self.members.get(i).cloned() else { return Action::None };
                if self.armed.as_ref() == Some(&name) {
                    self.armed = None;
                    return Action::Remove(name);
                }
                self.armed = Some(name);
                Action::None
            }
        }
    }

    /// A click: a tab label switches, a row runs (agents: selects; remove needs enter). Outside: close.
    pub fn mouse(&mut self, m: MouseEvent) -> Action {
        self.poll();
        let at = Position { x: m.column, y: m.row };
        match m.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                if !self.frame.contains(at) {
                    return Action::Close;
                }
                if let Some((_, t)) = self.tab_rects.iter().find(|(r, _)| r.contains(at)) {
                    self.tab = *t;
                    self.armed = None;
                    self.adding = None;
                    return Action::None;
                }
                let Some(&(_, i)) = self.rows.iter().find(|(r, _)| r.contains(at)) else { return Action::None };
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
                    Tab::Tokens => {}
                    Tab::Agents => {
                        if let Some(c) = self.adding {
                            let name = self.addable().get(i).map(|a| a.name.clone());
                            let _ = c;
                            self.adding = None;
                            return name.map(Action::Add).unwrap_or(Action::None);
                        }
                        self.acursor = i;
                        let row = self.agent_rows()[i];
                        if !matches!(row, Row::Member(_)) {
                            return self.activate(row);
                        }
                    }
                }
            }
            MouseEventKind::ScrollDown if self.tab == Tab::Threads => {
                let n = self.visible().len();
                if n > 0 {
                    self.tcursor = (self.tcursor + 1).min(n - 1);
                }
            }
            MouseEventKind::ScrollUp if self.tab == Tab::Threads => self.tcursor = self.tcursor.saturating_sub(1),
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

    /// The menu in AREA, centered on CENTER (the chat history; AREA keeps it on screen).
    pub fn render(&mut self, f: &mut Frame, area: Rect, center: Rect) {
        self.poll();
        self.poll_detail();
        let w = area.width.saturating_sub(4).min(76);
        let h = area.height.saturating_sub(2).min(32).max(8.min(area.height));
        let center = if center.width == 0 || center.height == 0 { area } else { center };
        let mid = |c: u16, len: u16, n: u16, lo: u16, hi: u16| (c + len / 2).saturating_sub(n / 2).max(lo).min(hi.saturating_sub(n));
        let r = Rect { x: mid(center.x, center.width, w, area.x, area.right()), y: mid(center.y, center.height, h, area.y, area.bottom()), width: w, height: h };
        self.frame = r;
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
            let st = if t == self.tab { Style::default().bg(theme::pal().accent).fg(theme::pal().on_accent).add_modifier(Modifier::BOLD) } else { theme::muted() };
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
                &[("↑↓", "move"), ("enter", "run"), ("←→", "tabs"), ("esc tab", "close")]
            }
            Tab::Threads => {
                self.draw_threads(f, body);
                &[("type", "filter"), ("↑↓", "move"), ("enter", "open"), ("←→", "tabs"), ("esc tab", "close")]
            }
            Tab::Search => {
                self.draw_search(f, body);
                &[("type", "search"), ("↑↓", "move"), ("enter", "jump to it"), ("←→", "tabs"), ("esc tab", "close")]
            }
            Tab::Tokens => {
                self.draw_tokens(f, body);
                &[("↑↓", "scroll"), ("r", "reload"), ("←→", "tabs"), ("esc tab", "close")]
            }
            Tab::Agents if self.adding.is_some() => {
                self.draw_agents(f, body);
                &[("↑↓", "move"), ("enter", "add"), ("⌫", "back"), ("esc", "close")]
            }
            Tab::Agents => {
                self.draw_agents(f, body);
                &[("↑↓", "move"), ("enter", "run · remove (2×)"), ("←→", "tabs"), ("esc tab", "close")]
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
        let fr = Rect { height: 1, ..body };
        f.render_widget(Paragraph::new(Span::styled("› ", theme::accent())), Rect { width: 2, ..fr });
        self.filter.render(f, Rect { x: fr.x + 2, width: fr.width.saturating_sub(2), ..fr }, true);
        let list = Rect { y: body.y + 2, height: body.height.saturating_sub(2), ..body };
        let vis = self.visible();
        if vis.is_empty() {
            let msg = if self.loading.is_some() { "reading threads…" } else { "0 threads" };
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
        for (slot, &i) in vis.iter().enumerate().skip(self.toffset).take(fits) {
            let y = list.y + (slot - self.toffset) as u16 * nav::CARD_H;
            let r = Rect { y, height: nav::CARD_H, ..list };
            let c = &self.cards[i];
            nav::draw_card(f.buffer_mut(), r, c, slot == self.tcursor, self.thread.as_deref() == Some(c.id.as_str()), true, tick);
            self.rows.push((r, i));
        }
        let more = vis.len().saturating_sub(self.toffset + fits);
        if self.toffset > 0 || more > 0 {
            let text = format!("{}↑ · {more}↓", self.toffset);
            f.render_widget(Paragraph::new(Span::styled(text, theme::muted())).right_aligned(), Rect { y: body.y, height: 1, ..body });
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
            let bg = if hl { Style::default().bg(theme::pal().panel) } else { Style::default() };
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

    fn draw_agents(&mut self, f: &mut Frame, body: Rect) {
        if let Some(c) = self.adding {
            let list = self.addable();
            f.render_widget(Paragraph::new(Span::styled("add an agent to the thread", theme::muted())), Rect { height: 1, ..body });
            if list.is_empty() {
                f.render_widget(Paragraph::new(Span::styled("every agent is on the thread", theme::muted())), Rect { y: body.y + 2, height: 1, ..body });
                return;
            }
            let area = Rect { y: body.y + 2, height: body.height.saturating_sub(2), ..body };
            let c = c.min(list.len() - 1);
            let effort = self.effort.clone();
            self.rows = self.draw_grid(f, area, &list, Some(c), |f, r, i, cur| {
                let a = &list[i];
                let title = cur.then(|| Span::styled(" enter: add ", theme::accent()));
                let look = crate::cards::Look { cur, lit: false, border: Some(crate::cards::border(cur, false, theme::pal().accent)), title, bg: None };
                crate::cards::draw(f, r, a, &[a.name.clone(), crate::cards::tech(a), crate::cards::effort(a, &effort)], look);
            });
            return;
        }
        let rows = self.agent_rows();
        self.acursor = self.acursor.min(rows.len() - 1);
        let cur_row = rows[self.acursor];
        let button = |f: &mut Frame, r: Rect, label: &str, hl: bool| {
            let st = if hl { Style::default().bg(theme::pal().accent).fg(theme::pal().on_accent).add_modifier(Modifier::BOLD) } else { Style::default().bg(theme::pal().panel).fg(theme::pal().text) };
            let br = Rect { width: (label.chars().count() as u16).min(r.width), height: 1, ..r };
            f.render_widget(Paragraph::new(label.to_string()).style(st), br);
            br
        };
        let edit = button(f, body, " ✎ edit agents: start screen, agents tab ", cur_row == Row::Edit);
        self.rows.push((edit, 0));
        if self.thread.is_none() {
            let note = Rect { y: body.y + 2, height: 1, ..body };
            if note.bottom() <= body.bottom() {
                f.render_widget(Paragraph::new(Span::styled("0 thread in view: open a thread for its agents", theme::muted())), note);
            }
            return;
        }
        // the agents of the thread: the grid of the start screen; add below it
        let members = self.member_agents();
        let groups = crate::cards::groups(&members, |_| true);
        let height = crate::cards::tight_grid(&groups, self.card_width(), crate::cards::CARD_H, body.width).height as u16;
        let area = Rect { y: body.y + 2, height: height.min(body.height.saturating_sub(4)), ..body };
        let cur = match cur_row {
            Row::Member(i) => Some(i),
            _ => None,
        };
        let armed = self.armed.clone();
        let effort = self.effort.clone();
        let drawn = self.draw_grid(f, area, &members, cur, |f, r, i, hl| {
            let a = &members[i];
            let title = if armed.as_deref() == Some(a.name.as_str()) {
                Some(Span::styled(" enter again: remove · ↑↓: keep ", Style::default().fg(theme::pal().danger).add_modifier(Modifier::BOLD)))
            } else {
                hl.then(|| Span::styled(" enter: remove ", theme::muted()))
            };
            let look = crate::cards::Look { cur: hl, lit: true, border: Some(crate::cards::border(hl, false, theme::pal().accent)), title, bg: None };
            crate::cards::draw(f, r, a, &[a.name.clone(), crate::cards::tech(a), crate::cards::effort(a, &effort)], look);
        });
        for (r, i) in drawn {
            if let Some(at) = rows.iter().position(|row| *row == Row::Member(i)) {
                self.rows.push((r, at));
            }
        }
        let add = Rect { y: area.bottom() + 1, height: 1, ..body };
        if add.bottom() <= body.bottom() {
            let r = button(f, add, " + add an agent ", cur_row == Row::Add);
            self.rows.push((r, rows.len() - 1));
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
        Menu::new(items, Some("t1".into()), vec!["amon".into()], vec![agent("amon"), agent("lilith")], vec![card("t1", "first"), card("t2", "deploy fix")])
    }

    #[test]
    fn tabs_cycle_and_esc_closes() {
        let mut m = menu();
        assert_eq!(m.tab, Tab::Quick);
        m.key(k(KeyCode::Right));
        assert_eq!(m.tab, Tab::Threads);
        m.key(k(KeyCode::Right));
        assert_eq!(m.tab, Tab::Search);
        m.key(k(KeyCode::Right));
        assert_eq!(m.tab, Tab::Tokens);
        m.key(k(KeyCode::Right));
        assert_eq!(m.tab, Tab::Agents);
        m.key(k(KeyCode::Right));
        assert_eq!(m.tab, Tab::Quick);
        m.key(k(KeyCode::Left));
        assert_eq!(m.tab, Tab::Agents);
        assert_eq!(m.key(k(KeyCode::Esc)), Action::Close);
        assert_eq!(m.key(k(KeyCode::Tab)), Action::Close);
        assert_eq!(m.key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT)), Action::Close);
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
            t.draw(|f| m.render(f, f.area(), f.area())).unwrap();
            let b = t.backend().buffer().clone();
            (0..30).map(|y| (0..90).map(|x| b[(x, y)].symbol().to_string()).collect::<String>()).collect::<Vec<_>>().join("\n")
        };
        assert!(s.contains(" search ") && s.contains("you · this thread") && s.contains("ada · deploy fix") && s.contains("fix the deploy"), "{s}");
        m.key(k(KeyCode::Char('x')));
        assert!(matches!(m.key(k(KeyCode::Enter)), Action::None));
    }

    #[test]
    fn threads_filter_and_open() {
        let mut m = menu();
        m.key(k(KeyCode::Right));
        m.key(k(KeyCode::Down));
        assert!(matches!(m.key(k(KeyCode::Enter)), Action::Open(c) if c.id == "t2"));
        for c in "first".chars() {
            m.key(k(KeyCode::Char(c)));
        }
        assert!(matches!(m.key(k(KeyCode::Enter)), Action::Open(c) if c.id == "t1"));
    }

    #[test]
    fn agents_remove_needs_2_enters_and_add_picks_off_thread_agents() {
        let mut m = menu();
        m.key(k(KeyCode::Left));
        assert_eq!(m.tab, Tab::Agents);
        assert_eq!(m.key(k(KeyCode::Enter)), Action::Home(Home::Agents));
        m.key(k(KeyCode::Down));
        assert_eq!(m.key(k(KeyCode::Enter)), Action::None);
        assert_eq!(m.key(k(KeyCode::Enter)), Action::Remove("amon".into()));
        // a move disarms
        m.key(k(KeyCode::Enter));
        m.key(k(KeyCode::Down));
        m.key(k(KeyCode::Up));
        assert_eq!(m.key(k(KeyCode::Enter)), Action::None);
        m.key(k(KeyCode::Down));
        assert_eq!(m.key(k(KeyCode::Enter)), Action::None);
        assert_eq!(m.key(k(KeyCode::Enter)), Action::Add("lilith".into()));
    }

    #[test]
    fn start_screen_agents_tab_has_the_edit_button_only() {
        let mut m = Menu::new(Vec::new(), None, Vec::new(), vec![agent("amon")], vec![card("t1", "x")]);
        m.key(k(KeyCode::Left));
        m.key(k(KeyCode::Down));
        assert_eq!(m.key(k(KeyCode::Enter)), Action::Home(Home::Agents));
    }

    #[test]
    #[ignore = "reads the real threads"]
    fn live_threads_tab() {
        let cards = nav::load();
        let mut m = Menu::new(Vec::new(), cards.first().map(|c| c.id.clone()), vec!["amon".into()], crate::agents::cached(), cards);
        m.key(k(KeyCode::Right));
        let mut t = ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 34)).unwrap();
        t.draw(|f| m.render(f, f.area(), f.area())).unwrap();
        let b = t.backend().buffer().clone();
        let s: Vec<String> = (0..34).map(|y| (0..100).map(|x| b[(x, y)].symbol().to_string()).collect()).collect();
        println!("{}", s.join("\n"));
        m.key(k(KeyCode::Right));
        m.detail = Some(crate::cost::detail(m.thread.as_deref()));
        t.draw(|f| m.render(f, f.area(), f.area())).unwrap();
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
        t.draw(|f| m.render(f, f.area(), f.area())).unwrap();
        let b = t.backend().buffer().clone();
        let s = (0..34).map(|y| (0..90).map(|x| b[(x, y)].symbol().to_string()).collect::<String>()).collect::<Vec<_>>().join("\n");
        if std::env::var_os("SHOW").is_some() {
            eprintln!("{s}");
        }
        s
    }

    #[test]
    fn agents_tab_shows_cards_by_harness_and_n_starts_a_thread() {
        let roster = vec![agent("amon", "claude"), agent("nova", "codex"), agent("lilith", "claude"), agent("regi", "codex")];
        let mut m = Menu::new(Vec::new(), Some("t1".into()), vec!["nova".into(), "amon".into(), "lilith".into()], roster, vec![Card { id: "t1".into(), ..Default::default() }]);
        assert_eq!(m.key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE)), Action::Home(Home::New));
        m.key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
        let s = screen(&mut m);
        assert!(s.contains("claude · 2") && s.contains("codex · 1") && s.contains("+ add an agent"), "{s}");
        // 0 blank rows between the harness groups: the heading right below the cards above
        let lines: Vec<&str> = s.lines().collect();
        let codex = lines.iter().position(|l| l.contains("codex · 1")).unwrap();
        assert!(lines[codex - 1].contains('└'), "{s}");
        // ↓: the cards in grid order (claude: amon, lilith; codex: nova)
        m.key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        m.key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        m.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(m.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)), Action::Remove("lilith".into()));
        // add: the agents off the thread as cards
        m.key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        m.key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        m.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        let s = screen(&mut m);
        assert!(s.contains("codex · 1") && s.contains("regi") && s.contains("enter: add"), "{s}");
        assert_eq!(m.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)), Action::Add("regi".into()));
    }
}
