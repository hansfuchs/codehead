//! F2 menu: 1 modal for the start screen and the thread view. Tabs: quick (commands),
//! threads (filter + thread cards), agents (the agents of the thread in view: remove, add; edit →
//! the agents tab of the start screen). ↑↓ move, tab / shift+tab, ←→: next / previous tab, enter: run,
//! a single esc closes.

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
    Tokens,
    Agents,
}

const TABS: [(Tab, &str); 4] = [(Tab::Quick, "quick"), (Tab::Threads, "threads"), (Tab::Tokens, "tokens"), (Tab::Agents, "agents")];

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
    /// Quick command of the host (its id).
    Command(usize),
    Remove(String),
    Add(String),
}

/// Quick command of the host: label, hotkey hint, id.
pub struct Item {
    pub label: String,
    pub key: String,
    pub id: usize,
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
    /// Last draw: tab labels, rows (clicks).
    tab_rects: Vec<(Rect, Tab)>,
    rows: Vec<(Rect, usize)>,
    frame: Rect,
}

fn builtins() -> Vec<Item> {
    vec![
        Item { label: "new: start screen, new tab".into(), key: String::new(), id: usize::MAX },
        Item { label: "settings: start screen, settings tab".into(), key: String::new(), id: usize::MAX - 1 },
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
            armed: None,
            adding: None,
            started: Instant::now(),
            detail: None,
            detail_rx: None,
            detail_at: None,
            dscroll: 0,
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
    }

    fn visible(&self) -> Vec<usize> {
        nav::filter(&self.cards, &self.filter.text())
    }

    fn agent_rows(&self) -> Vec<Row> {
        let mut v = vec![Row::Edit];
        if self.thread.is_some() {
            v.extend((0..self.members.len()).map(Row::Member));
            v.push(Row::Add);
        }
        v
    }

    /// Roster agents off the thread (the add picker).
    fn addable(&self) -> Vec<&Agent> {
        self.roster.iter().filter(|a| !self.members.contains(&a.name)).collect()
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
            KeyCode::Esc => return Action::Close,
            KeyCode::F(2) => return Action::Close,
            // ←/→ too: tabs only (0 run, 0 cursor in the threads filter)
            KeyCode::Tab | KeyCode::Right => {
                self.switch(1);
                return Action::None;
            }
            KeyCode::BackTab | KeyCode::Left => {
                self.switch(-1);
                return Action::None;
            }
            _ => {}
        }
        match self.tab {
            Tab::Quick => self.quick_key(k),
            Tab::Threads => self.threads_key(k),
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
            KeyCode::Enter => return self.run_item(self.qcursor),
            _ => {}
        }
        Action::None
    }

    fn run_item(&self, i: usize) -> Action {
        match self.items.get(i).map(|it| it.id) {
            Some(usize::MAX) => Action::Home(Home::New),
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

    pub fn render(&mut self, f: &mut Frame, area: Rect) {
        self.poll();
        self.poll_detail();
        let w = area.width.saturating_sub(4).min(76);
        let h = area.height.saturating_sub(2).min(30).max(8.min(area.height));
        let r = Rect { x: area.x + (area.width - w) / 2, y: area.y + (area.height - h) / 2, width: w, height: h };
        self.frame = r;
        f.render_widget(Clear, r);
        let block = Block::new()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
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
            let st = if t == self.tab { Style::default().bg(theme::ACCENT).fg(theme::ON_ACCENT).add_modifier(Modifier::BOLD) } else { theme::muted() };
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
                &[("↑↓", "move"), ("enter", "run"), ("tab ←→", "tabs"), ("esc", "close")]
            }
            Tab::Threads => {
                self.draw_threads(f, body);
                &[("type", "filter"), ("↑↓", "move"), ("enter", "open"), ("tab ←→", "tabs"), ("esc", "close")]
            }
            Tab::Tokens => {
                self.draw_tokens(f, body);
                &[("↑↓", "scroll"), ("r", "reload"), ("tab ←→", "tabs"), ("esc", "close")]
            }
            Tab::Agents if self.adding.is_some() => {
                self.draw_agents(f, body);
                &[("↑↓", "move"), ("enter", "add"), ("⌫", "back"), ("esc", "close")]
            }
            Tab::Agents => {
                self.draw_agents(f, body);
                &[("↑↓", "move"), ("enter", "run · remove (2×)"), ("tab ←→", "tabs"), ("esc", "close")]
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
            (Style::default().fg(theme::ON_ACCENT).add_modifier(Modifier::BOLD), Style::default().fg(theme::ON_ACCENT))
        } else {
            (theme::accent(), theme::text())
        }
    }

    fn draw_quick(&mut self, f: &mut Frame, body: Rect) {
        let kw = self.items.iter().map(|i| i.key.chars().count()).max().unwrap_or(0);
        for (i, it) in self.items.iter().enumerate() {
            let y = body.y + i as u16;
            if y >= body.bottom() {
                break;
            }
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
            let rows: Vec<(Rect, usize)> = list
                .iter()
                .enumerate()
                .take(body.height.saturating_sub(2) as usize)
                .map(|(i, a)| {
                    let r = Rect { y: body.y + 2 + i as u16, height: 1, ..body };
                    let hl = i == c;
                    let color = crate::agents::color(a.color.as_deref());
                    let name_st = if hl { Style::default().fg(theme::ON_ACCENT).add_modifier(Modifier::BOLD) } else { Style::default().fg(color).add_modifier(Modifier::BOLD) };
                    let (_, ls) = Self::row_style(hl);
                    let line = Line::from(vec![Span::styled(format!(" + {}", a.name), name_st), Span::styled(format!("  {}", a.harness_model()), ls)]);
                    f.render_widget(Paragraph::new(line).style(if hl { theme::pill() } else { Style::default() }), r);
                    (r, i)
                })
                .collect();
            self.rows = rows;
            return;
        }
        let rows = self.agent_rows();
        self.acursor = self.acursor.min(rows.len() - 1);
        let card_h = crate::sprites::H as u16 + 2;
        let mut y = body.y;
        for (i, row) in rows.iter().enumerate() {
            let hl = i == self.acursor;
            let h = if matches!(row, Row::Member(_)) { card_h } else { 1 };
            if y + h > body.bottom() {
                break;
            }
            let r = Rect { y, height: h, ..body };
            match row {
                Row::Edit | Row::Add => {
                    let label = if *row == Row::Edit { " ✎ edit agents: start screen, agents tab " } else { " + add an agent " };
                    let st = if hl { Style::default().bg(theme::ACCENT).fg(theme::ON_ACCENT).add_modifier(Modifier::BOLD) } else { Style::default().bg(theme::PANEL).fg(theme::TEXT) };
                    let br = Rect { width: (label.chars().count() as u16).min(r.width), ..r };
                    f.render_widget(Paragraph::new(label).style(st), br);
                    self.rows.push((br, i));
                    if *row == Row::Edit && self.thread.is_none() {
                        let note = Rect { y: y + 2, height: 1, ..body };
                        if note.bottom() <= body.bottom() {
                            f.render_widget(Paragraph::new(Span::styled("0 thread in view: open a thread for its agents", theme::muted())), note);
                        }
                    }
                }
                Row::Member(m) => {
                    let name = &self.members[*m];
                    self.draw_member(f, r, name, hl);
                    self.rows.push((r, i));
                }
            }
            y += h + 1;
        }
    }

    fn draw_member(&self, f: &mut Frame, r: Rect, name: &str, hl: bool) {
        let a = self.roster.iter().find(|a| a.name == name);
        let color = crate::agents::color(a.and_then(|a| a.color.as_deref()));
        let armed = self.armed.as_deref() == Some(name);
        let border = if hl { theme::accent_bold() } else { theme::line() };
        let title = if armed {
            Span::styled(" enter again: remove · ↑↓: keep ", Style::default().fg(theme::DANGER).add_modifier(Modifier::BOLD))
        } else if hl {
            Span::styled(" enter: remove ", theme::muted())
        } else {
            Span::raw("")
        };
        let block = Block::new().borders(Borders::ALL).border_type(BorderType::Rounded).border_style(border).title(title);
        let inner = block.inner(r);
        f.render_widget(block, r);
        let tech = a.map(|a| {
            let m = crate::agents::model_of(a);
            if m.is_empty() { a.harness.clone() } else { format!("{} · {m}", a.harness) }
        });
        let side = vec![
            vec![Span::styled(format!("  {name}"), Style::default().fg(color).add_modifier(Modifier::BOLD))],
            vec![Span::styled(format!("  {}", tech.unwrap_or_default()), theme::muted())],
            vec![],
            vec![],
        ];
        let sprite = a.map(crate::agents::sprite).unwrap_or_else(|| crate::sprites::pool()[0].clone());
        let inner = Rect { x: inner.x + 1, width: inner.width.saturating_sub(1), ..inner };
        f.render_widget(Paragraph::new(crate::sprites::lines(&sprite, color, side)), inner);
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
        let items = vec![Item { label: "keys".into(), key: "?".into(), id: 7 }];
        Menu::new(items, Some("t1".into()), vec!["amon".into()], vec![agent("amon"), agent("lilith")], vec![card("t1", "first"), card("t2", "deploy fix")])
    }

    #[test]
    fn tabs_cycle_and_esc_closes() {
        let mut m = menu();
        assert_eq!(m.tab, Tab::Quick);
        m.key(k(KeyCode::Tab));
        assert_eq!(m.tab, Tab::Threads);
        m.key(k(KeyCode::Tab));
        assert_eq!(m.tab, Tab::Tokens);
        m.key(k(KeyCode::Tab));
        assert_eq!(m.tab, Tab::Agents);
        m.key(k(KeyCode::Tab));
        assert_eq!(m.tab, Tab::Quick);
        m.key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT));
        assert_eq!(m.tab, Tab::Agents);
        assert_eq!(m.key(k(KeyCode::Esc)), Action::Close);
    }

    #[test]
    fn quick_commands_run() {
        let mut m = menu();
        assert_eq!(m.key(k(KeyCode::Enter)), Action::Home(Home::New));
        m.key(k(KeyCode::Down));
        assert_eq!(m.key(k(KeyCode::Enter)), Action::Home(Home::Settings));
        m.key(k(KeyCode::Down));
        assert_eq!(m.key(k(KeyCode::Enter)), Action::Command(7));
        m.key(k(KeyCode::Down));
        assert_eq!(m.key(k(KeyCode::Enter)), Action::Home(Home::New));
    }

    #[test]
    fn threads_filter_and_open() {
        let mut m = menu();
        m.key(k(KeyCode::Tab));
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
        m.key(k(KeyCode::BackTab));
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
        m.key(k(KeyCode::BackTab));
        m.key(k(KeyCode::Down));
        assert_eq!(m.key(k(KeyCode::Enter)), Action::Home(Home::Agents));
    }

    #[test]
    #[ignore = "reads the real threads"]
    fn live_threads_tab() {
        let cards = nav::load();
        let mut m = Menu::new(Vec::new(), cards.first().map(|c| c.id.clone()), vec!["amon".into()], crate::agents::cached(), cards);
        m.key(k(KeyCode::Tab));
        let mut t = ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 34)).unwrap();
        t.draw(|f| m.render(f, f.area())).unwrap();
        let b = t.backend().buffer().clone();
        let s: Vec<String> = (0..34).map(|y| (0..100).map(|x| b[(x, y)].symbol().to_string()).collect()).collect();
        println!("{}", s.join("\n"));
        m.key(k(KeyCode::Tab));
        m.detail = Some(crate::cost::detail(m.thread.as_deref()));
        t.draw(|f| m.render(f, f.area())).unwrap();
        let b = t.backend().buffer().clone();
        let s: Vec<String> = (0..34).map(|y| (0..100).map(|x| b[(x, y)].symbol().to_string()).collect()).collect();
        println!("{}", s.join("\n"));
    }
}
