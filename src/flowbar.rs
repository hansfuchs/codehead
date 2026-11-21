//! The flow row of the thread view (above the subagents row): the thread's flow, its current
//! step, a wait (gate, loop max, open role), a pending / queued switch, warnings. ctrl+f (or a
//! click, or `> flow`) opens its box:
//! - steps: the rail (marks, arcs), enter: the step's text; s skip, j jump, a answer the wait,
//!   @ the role of a step, c change the flow, e edit it in the flows screen, x x remove it.
//! - change flow: the flows (typed text filters) → `switch now` / `after current flow` / cancel
//!   (asked at once in the same box: 0 frame where the chat waits on a hidden choice) → the start
//!   step. `now`: the host stops the running turn at its next tool boundary.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph, Wrap};
use serde_json::{Value, json};

use crate::engine::flows::run::{FlowState, Wait};
use crate::engine::flows::{self, Library, Resolved};
use crate::flowrail;
use crate::theme;

/// What a key asks the view to do.
#[derive(Clone, Debug, PartialEq)]
pub enum Act {
    None,
    /// A host command (op, command).
    Host(&'static str, Value),
    /// Answer the wait with CHOICE: the prompt's text goes along as the note.
    Answer(String),
    /// Open the flows screen on flow SLUG.
    Edit(String),
}

#[derive(Clone, Debug, PartialEq)]
enum Mode {
    Steps,
    /// The flows to switch to; typed filter.
    Pick(String),
    /// A flow runs: now, after, cancel.
    How(String),
    /// The start step of the new flow (MODE `now` / `set`).
    Start(String),
    /// The agent for ROLE.
    Role(String),
    /// The choices of the wait.
    Answer,
}

pub struct FlowBar {
    open: bool,
    mode: Mode,
    cur: usize,
    offset: usize,
    /// enter on a step: its text below the rail.
    detail: bool,
    /// x once: armed, x again: the flow goes.
    armed: bool,
    // the thread's flow, read from thread.json
    pub state: Option<FlowState>,
    nodes: Vec<Resolved>,
    name: String,
    lead: Option<String>,
    agents: Vec<String>,
    dir: PathBuf,
    lib: Library,
    warnings: Vec<(usize, String)>,
    updated: u64,
    read_at: Option<Instant>,
    lib_at: Option<Instant>,
    // rects of the last draw
    bar: Rect,
    popup: Rect,
    rows: Vec<(Rect, usize)>,
    hover: bool,
}

impl Default for FlowBar {
    fn default() -> Self {
        FlowBar {
            open: false,
            mode: Mode::Steps,
            cur: 0,
            offset: 0,
            detail: false,
            armed: false,
            state: None,
            nodes: Vec::new(),
            name: String::new(),
            lead: None,
            agents: Vec::new(),
            dir: PathBuf::new(),
            lib: Library::default(),
            warnings: Vec::new(),
            updated: 0,
            read_at: None,
            lib_at: None,
            bar: Rect::default(),
            popup: Rect::default(),
            rows: Vec::new(),
            hover: false,
        }
    }
}

/// Choices of wait W at node N: (choice sent, label).
fn choices(w: &Wait, n: Option<&Resolved>) -> Vec<(String, String)> {
    match w {
        Wait::Gate => {
            let mut v = vec![("approve".to_string(), "approve · go on".to_string())];
            if let Some(n) = n {
                for (k, _) in &n.edges {
                    v.push((k.clone(), k.clone()));
                }
            }
            v
        }
        Wait::LoopMax => vec![("again".into(), "1 more round".into()), ("next".into(), "move on".into())],
        Wait::Role { .. } => Vec::new(),
    }
}

impl FlowBar {
    /// Read thread ID's flow again: every second, at once with FORCE (a flow event came).
    pub fn tick(&mut self, id: &str, force: bool) -> bool {
        if !force && self.read_at.is_some_and(|t| t.elapsed() < Duration::from_secs(1)) {
            return false;
        }
        self.read_at = Some(Instant::now());
        let Some(t) = crate::engine::store::load(id) else { return false };
        if t.updated == self.updated && !force {
            return false;
        }
        self.updated = t.updated;
        let changed = self.state != t.flow;
        self.state = t.flow.clone();
        self.lead = t.lead().map(String::from);
        self.agents = t.agents.clone();
        self.dir = t.work_dir();
        self.reload_lib(force);
        changed
    }

    fn reload_lib(&mut self, force: bool) {
        if force || self.lib_at.is_none_or(|t| t.elapsed() > Duration::from_secs(5)) {
            self.lib = Library::load(Some(&self.dir));
            self.lib_at = Some(Instant::now());
        }
        self.nodes.clear();
        self.warnings.clear();
        self.name.clear();
        if let Some(st) = &self.state {
            self.name = st.title.clone();
            if let Some(f) = self.lib.flow(&st.flow) {
                self.name = f.name.clone();
                self.nodes = flows::resolve(f, &self.lib);
                let roster = crate::engine::roster::active();
                self.warnings = flows::run::warnings(&self.nodes, st, self.lead.as_deref(), &roster, &self.dir);
            }
        }
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Open on the steps (the current one under the cursor), or on the wait's choices.
    pub fn open(&mut self) {
        self.open = true;
        self.detail = false;
        self.armed = false;
        self.lib_at = None;
        let dir = self.dir.clone();
        self.lib = Library::load(Some(&dir));
        self.reload_lib(false);
        match &self.state {
            None => self.pick(),
            Some(st) => {
                self.cur = st.node;
                self.mode = match &st.wait {
                    Some(Wait::Role { role }) => Mode::Role(role.clone()),
                    Some(_) => Mode::Answer,
                    None => Mode::Steps,
                };
                if matches!(self.mode, Mode::Role(_) | Mode::Answer) {
                    self.cur = 0;
                }
            }
        }
    }

    /// Open on the flows to switch to (`> flow`).
    pub fn pick(&mut self) {
        self.open = true;
        self.mode = Mode::Pick(String::new());
        self.cur = self.state.as_ref().and_then(|s| self.lib.flows.iter().position(|f| f.slug == s.flow)).unwrap_or(0);
        self.offset = 0;
    }

    fn picks(&self, filter: &str) -> Vec<usize> {
        let q = filter.to_lowercase();
        (0..self.lib.flows.len()).filter(|&i| q.is_empty() || self.lib.flows[i].name.to_lowercase().contains(&q) || self.lib.flows[i].slug.contains(&q)).collect()
    }

    fn running(&self) -> bool {
        self.state.as_ref().is_some_and(|s| !s.finished)
    }

    fn len(&self) -> usize {
        match &self.mode {
            Mode::Steps => self.nodes.len(),
            Mode::Pick(q) => self.picks(q).len(),
            Mode::How(_) => 3,
            Mode::Start(slug) => self.lib.flow(slug).map_or(0, |f| f.steps.len()),
            Mode::Role(_) => self.role_agents().len(),
            Mode::Answer => self.state.as_ref().and_then(|s| s.wait.as_ref()).map_or(0, |w| choices(w, self.nodes.get(self.state.as_ref().map_or(0, |s| s.node))).len()),
        }
    }

    /// Agents for a role: the thread's first, then the rest of the roster.
    fn role_agents(&self) -> Vec<(String, bool)> {
        let mut v: Vec<(String, bool)> = self.agents.iter().map(|a| (a.clone(), true)).collect();
        for a in crate::engine::roster::active() {
            if !self.agents.contains(&a.name) {
                v.push((a.name, false));
            }
        }
        v
    }

    pub fn key(&mut self, k: KeyEvent) -> Option<Act> {
        if !self.open {
            return None;
        }
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        let n = self.len();
        if !matches!(k.code, KeyCode::Char('x')) {
            self.armed = false;
        }
        match k.code {
            KeyCode::Esc => {
                match &self.mode {
                    Mode::Steps if self.detail => self.detail = false,
                    Mode::Steps => self.open = false,
                    Mode::Pick(_) | Mode::Answer | Mode::Role(_) if self.state.is_some() => {
                        self.mode = Mode::Steps;
                        self.cur = self.state.as_ref().map_or(0, |s| s.node);
                    }
                    Mode::How(_) | Mode::Start(_) => self.pick(),
                    _ => self.open = false,
                }
                return Some(Act::None);
            }
            KeyCode::Char('f') if ctrl => {
                self.open = false;
                return Some(Act::None);
            }
            KeyCode::Up => {
                self.cur = self.cur.saturating_sub(1);
                return Some(Act::None);
            }
            KeyCode::Down => {
                self.cur = (self.cur + 1).min(n.saturating_sub(1));
                return Some(Act::None);
            }
            _ => {}
        }
        let act = match self.mode.clone() {
            Mode::Pick(mut q) => match k.code {
                KeyCode::Enter | KeyCode::Tab => {
                    let Some(&i) = self.picks(&q).get(self.cur) else { return Some(Act::None) };
                    let slug = self.lib.flows[i].slug.clone();
                    self.cur = 0;
                    self.mode = if self.running() { Mode::How(slug) } else { Mode::Start(slug) };
                    Act::None
                }
                KeyCode::Backspace => {
                    q.pop();
                    self.cur = 0;
                    self.mode = Mode::Pick(q);
                    Act::None
                }
                KeyCode::Char(c) if !ctrl => {
                    q.push(c);
                    self.cur = 0;
                    self.mode = Mode::Pick(q);
                    Act::None
                }
                _ => Act::None,
            },
            Mode::How(slug) => match k.code {
                KeyCode::Enter => match self.cur {
                    0 => {
                        self.mode = Mode::Start(slug);
                        self.cur = 0;
                        Act::None
                    }
                    1 => {
                        let name = self.lib.flow(&slug).map(|f| f.name.clone()).unwrap_or_default();
                        if let Some(st) = self.state.as_mut() {
                            st.queued = Some(flows::run::Switch { flow: slug.clone(), start: 0 });
                        }
                        self.mode = Mode::Steps;
                        self.open = false;
                        let _ = name;
                        Act::Host("flow_set", json!({"op": "flow_set", "flow": slug, "mode": "after"}))
                    }
                    _ => {
                        self.pick();
                        Act::None
                    }
                },
                _ => Act::None,
            },
            Mode::Start(slug) => match k.code {
                KeyCode::Enter => {
                    let start = self.cur;
                    // the chip shows the switch at once (the host confirms with its log line)
                    if let Some(st) = self.state.as_mut().filter(|s| !s.finished) {
                        st.pending = Some(flows::run::Switch { flow: slug.clone(), start });
                    }
                    self.mode = Mode::Steps;
                    self.open = false;
                    Act::Host("flow_set", json!({"op": "flow_set", "flow": slug, "start": start, "mode": "now"}))
                }
                _ => Act::None,
            },
            Mode::Role(role) => match k.code {
                KeyCode::Enter => {
                    let Some((agent, _)) = self.role_agents().get(self.cur).cloned() else { return Some(Act::None) };
                    self.mode = Mode::Steps;
                    self.cur = self.state.as_ref().map_or(0, |s| s.node);
                    if let Some(st) = self.state.as_mut() {
                        st.roles.insert(role.clone(), agent.clone());
                        if st.wait == Some(Wait::Role { role: role.clone() }) {
                            st.wait = None;
                        }
                    }
                    Act::Host("flow_role", json!({"op": "flow_role", "role": role, "agent": agent}))
                }
                _ => Act::None,
            },
            Mode::Answer => match k.code {
                KeyCode::Enter => {
                    let Some(st) = self.state.as_ref() else { return Some(Act::None) };
                    let Some(w) = st.wait.clone() else { return Some(Act::None) };
                    let Some((choice, _)) = choices(&w, self.nodes.get(st.node)).get(self.cur).cloned() else { return Some(Act::None) };
                    if let Some(st) = self.state.as_mut() {
                        st.wait = None;
                    }
                    self.mode = Mode::Steps;
                    self.open = false;
                    Act::Answer(choice)
                }
                _ => Act::None,
            },
            Mode::Steps => match k.code {
                KeyCode::Enter => {
                    self.detail = !self.detail;
                    Act::None
                }
                KeyCode::Char('a') if self.state.as_ref().is_some_and(|s| s.wait.is_some()) => {
                    self.open();
                    Act::None
                }
                KeyCode::Char('s') if self.running() => {
                    if let Some(st) = self.state.as_mut() {
                        st.wait = None;
                    }
                    Act::Host("flow_skip", json!({"op": "flow_skip"}))
                }
                KeyCode::Char('j') if self.state.is_some() && self.cur < self.nodes.len() => Act::Host("flow_jump", json!({"op": "flow_jump", "node": self.cur})),
                KeyCode::Char('@') => {
                    let role = self.nodes.get(self.cur).map(|n| n.role.clone()).unwrap_or_else(|| flows::LEAD.into());
                    self.mode = Mode::Role(role);
                    self.cur = 0;
                    Act::None
                }
                KeyCode::Char('c') => {
                    self.pick();
                    Act::None
                }
                KeyCode::Char('u') if self.state.as_ref().is_some_and(|s| s.pending.is_some() || s.queued.is_some()) => {
                    if let Some(st) = self.state.as_mut() {
                        st.pending = None;
                        st.queued = None;
                    }
                    Act::Host("flow_cancel", json!({"op": "flow_cancel"}))
                }
                KeyCode::Char('e') => {
                    let slug = self.state.as_ref().map(|s| s.flow.clone()).unwrap_or_default();
                    self.open = false;
                    Act::Edit(slug)
                }
                KeyCode::Char('x') if self.state.is_some() => {
                    if self.armed {
                        self.armed = false;
                        self.state = None;
                        self.open = false;
                        Act::Host("flow_clear", json!({"op": "flow_clear"}))
                    } else {
                        self.armed = true;
                        Act::None
                    }
                }
                _ => Act::None,
            },
        };
        Some(act)
    }

    pub fn mouse(&mut self, m: MouseEvent) -> bool {
        let at = Position { x: m.column, y: m.row };
        self.hover = self.bar.contains(at);
        if matches!(m.kind, MouseEventKind::Down(MouseButton::Left)) && self.bar.contains(at) {
            if self.open {
                self.open = false;
            } else {
                self.open();
            }
            return true;
        }
        if self.open && self.popup.contains(at) {
            match m.kind {
                MouseEventKind::ScrollDown => self.cur = (self.cur + 1).min(self.len().saturating_sub(1)),
                MouseEventKind::ScrollUp => self.cur = self.cur.saturating_sub(1),
                MouseEventKind::Down(MouseButton::Left) => {
                    if let Some(&(_, i)) = self.rows.iter().find(|(r, _)| r.contains(at)) {
                        self.cur = i;
                    }
                }
                _ => {}
            }
            return true;
        }
        false
    }

    /// The row's text and style.
    fn bar_line(&self) -> (Vec<Span<'static>>, Style) {
        let Some(st) = &self.state else { return (vec![Span::raw("◆ 0 flow · ctrl+f: pick one")], theme::muted()) };
        let mut spans = Vec::new();
        let mut style = theme::accent();
        let total = self.nodes.len();
        let step = self.nodes.get(st.node).map(|n| n.title.clone()).unwrap_or_default();
        if st.finished {
            spans.push(Span::raw(format!("◆ {} · finished", self.name)));
            style = Style::default().fg(theme::pal().ok);
        } else {
            let mut s = format!("◆ {} · {}/{total} {step}", self.name, st.node + 1);
            if let Some(n) = self.nodes.get(st.node) {
                if let Some((_, _, max)) = &n.loops {
                    let r = st.round(&n.id);
                    if r > 0 {
                        s.push_str(&format!(" · round {}/{max}", r + 1));
                    }
                }
                if let Some(a) = st.agent_of(n, self.lead.as_deref()) {
                    s.push_str(&format!(" · @{a}"));
                }
            }
            spans.push(Span::raw(s));
        }
        match &st.wait {
            Some(Wait::Gate) => spans.push(Span::styled(format!("  ⏸ gate {step}: ctrl+f answer"), Style::default().fg(theme::pal().warn).add_modifier(Modifier::BOLD))),
            Some(Wait::LoopMax) => spans.push(Span::styled("  ⏸ loop max reached: ctrl+f decide".to_string(), Style::default().fg(theme::pal().warn).add_modifier(Modifier::BOLD))),
            Some(Wait::Role { role }) => spans.push(Span::styled(format!("  ⏸ @{role} has 0 agent: ctrl+f pick"), Style::default().fg(theme::pal().warn).add_modifier(Modifier::BOLD))),
            None => {}
        }
        if let Some(p) = &st.pending {
            let name = self.lib.flow(&p.flow).map_or(p.flow.clone(), |f| f.name.clone());
            spans.push(Span::styled(format!("  → {name} at the next tool boundary"), Style::default().fg(theme::pal().warn)));
        }
        if let Some(q) = &st.queued {
            let name = self.lib.flow(&q.flow).map_or(q.flow.clone(), |f| f.name.clone());
            spans.push(Span::styled(format!("  → queued: {name}"), theme::muted()));
        }
        if !self.warnings.is_empty() {
            spans.push(Span::styled(format!("  ! {}", self.warnings.len()), Style::default().fg(theme::pal().danger)));
        }
        (spans, style)
    }

    /// Rows for the flow area: 0 (0 flow, closed), 1 (the row) or the row + the open box.
    pub fn rows(&self, max: u16) -> u16 {
        if !self.open {
            return u16::from(self.state.is_some());
        }
        let body = self.len().max(1) as u16 + 4 + if self.detail { 8 } else { 0 } + self.warnings.len().min(4) as u16;
        (1 + body).min(max.max(1))
    }

    pub fn render(&mut self, f: &mut Frame, area: Rect, focus: bool) {
        self.bar = Rect::default();
        self.popup = Rect::default();
        self.rows.clear();
        if area.is_empty() {
            return;
        }
        let anchor = Rect { y: area.bottom() - 1, height: 1, ..area };
        self.bar = anchor;
        f.render_widget(Clear, anchor);
        let (spans, style) = self.bar_line();
        let style = if self.hover { style.bg(theme::pal().hover) } else { style };
        f.render_widget(Paragraph::new(Line::from(spans)).style(style), anchor);
        if !self.open || area.height < 5 {
            return;
        }
        let r = Rect { height: area.height - 1, ..area };
        self.popup = r;
        f.render_widget(Clear, r);
        let (title, keys) = self.frame_text();
        let block = Block::new()
            .borders(Borders::ALL)
            .border_type(BorderType::Plain)
            .border_style(theme::accent())
            .title(Span::styled(format!(" {title} "), theme::accent_bold()))
            .title_bottom(Line::styled(format!(" {keys} "), theme::muted()).right_aligned());
        let inner = block.inner(r);
        f.render_widget(block, r);
        let inner = Rect { x: inner.x + 1, width: inner.width.saturating_sub(2), ..inner };
        if inner.height == 0 {
            return;
        }
        match self.mode.clone() {
            Mode::Steps => self.draw_steps(f, inner, focus),
            Mode::Pick(q) => {
                let head = Rect { height: 1, ..inner };
                f.render_widget(Paragraph::new(Line::from(vec![Span::styled("flow: ", theme::muted()), Span::styled(format!("{q}▏"), theme::text())])), head);
                let list = Rect { y: inner.y + 2, height: inner.height.saturating_sub(2), ..inner };
                let rows: Vec<(String, String)> = self
                    .picks(&q)
                    .into_iter()
                    .map(|i| {
                        let fl = &self.lib.flows[i];
                        let cur = self.state.as_ref().is_some_and(|s| s.flow == fl.slug);
                        (format!("{}{}", fl.name, if cur { " (current)" } else { "" }), format!("{} steps · {}", fl.steps.len(), fl.description))
                    })
                    .collect();
                self.list(f, list, &rows, focus);
            }
            Mode::How(slug) => {
                let name = self.lib.flow(&slug).map_or(slug.clone(), |f| f.name.clone());
                let cur = self.state.as_ref().map_or(String::new(), |s| s.title.clone());
                let left = self.state.as_ref().map_or(0, |s| self.nodes.len().saturating_sub(s.node));
                f.render_widget(Paragraph::new(Line::styled(format!("change flow: {cur} → {name}"), theme::bold())), Rect { height: 1, ..inner });
                let rows = vec![
                    ("switch now".to_string(), "the running turn stops at its next tool boundary; then the start step".to_string()),
                    ("after current flow".to_string(), format!("{left} steps of {cur} left")),
                    ("cancel".to_string(), String::new()),
                ];
                self.list(f, Rect { y: inner.y + 2, height: inner.height.saturating_sub(2), ..inner }, &rows, focus);
            }
            Mode::Start(slug) => {
                let Some(fl) = self.lib.flow(&slug).cloned() else { return };
                f.render_widget(Paragraph::new(Line::styled(format!("{}: start at", fl.name), theme::bold())), Rect { height: 1, ..inner });
                let nodes = flows::resolve(&fl, &self.lib);
                let rows: Vec<flowrail::Row> = nodes.iter().map(|n| rail_row(n, "○", theme::muted(), None)).collect();
                let arcs = flowrail::arcs(&nodes, |_| 0);
                let list = Rect { y: inner.y + 2, height: inner.height.saturating_sub(2), ..inner };
                self.offset = crate::dropdown::fit(self.offset, self.cur, rows.len(), usize::from(list.height));
                self.rows = flowrail::draw(f.buffer_mut(), list, &rows, &arcs, Some(self.cur), focus, self.offset);
            }
            Mode::Role(role) => {
                f.render_widget(Paragraph::new(Line::styled(format!("@{role}: bring in"), theme::bold())), Rect { height: 1, ..inner });
                let rows: Vec<(String, String)> = self.role_agents().into_iter().map(|(a, on)| (a, if on { "on the thread".into() } else { "joins the thread".into() })).collect();
                self.list(f, Rect { y: inner.y + 2, height: inner.height.saturating_sub(2), ..inner }, &rows, focus);
            }
            Mode::Answer => {
                let Some(st) = self.state.clone() else { return };
                let n = self.nodes.get(st.node).cloned().unwrap_or_default();
                let what = match &st.wait {
                    Some(Wait::LoopMax) => format!("loop of {}: max rounds reached", n.title),
                    _ => format!("gate {}: the agent waits", n.title),
                };
                f.render_widget(Paragraph::new(Line::styled(what, Style::default().fg(theme::pal().warn).add_modifier(Modifier::BOLD))), Rect { height: 1, ..inner });
                f.render_widget(Paragraph::new(Line::styled("text in the prompt goes along as the note", theme::muted())), Rect { y: inner.y + 1, height: 1, ..inner });
                let rows: Vec<(String, String)> = st
                    .wait
                    .as_ref()
                    .map(|w| choices(w, Some(&n)))
                    .unwrap_or_default()
                    .into_iter()
                    .map(|(c, l)| {
                        let to = n.edges.iter().find(|(k, _)| *k == c).and_then(|(_, j)| self.nodes.get(*j)).map(|t| format!("back to {}", t.title)).unwrap_or_default();
                        (l, to)
                    })
                    .collect();
                self.list(f, Rect { y: inner.y + 3, height: inner.height.saturating_sub(3), ..inner }, &rows, focus);
            }
        }
    }

    fn frame_text(&self) -> (String, &'static str) {
        match &self.mode {
            Mode::Steps if self.armed => (format!("flow · {}", self.name), "x again: remove the flow · any key: keep"),
            Mode::Steps if self.state.as_ref().is_some_and(|s| s.wait.is_some()) => (format!("flow · {}", self.name), "a answer · ↑↓ · enter text · s skip · j jump · @ role · c change · e edit · esc"),
            Mode::Steps if self.state.as_ref().is_some_and(|s| s.pending.is_some() || s.queued.is_some()) => (format!("flow · {}", self.name), "↑↓ · enter text · u undo switch · s skip · j jump · @ role · c change · e edit · esc"),
            Mode::Steps => (format!("flow · {}", self.name), "↑↓ · enter text · s skip · j jump here · @ role · c change · e edit · x x remove · esc"),
            Mode::Pick(_) => ("pick a flow".into(), "type: filter · ↑↓ · enter · esc"),
            Mode::How(_) => ("change flow".into(), "↑↓ · enter · esc: back"),
            Mode::Start(_) => ("start step".into(), "↑↓ · enter: switch · esc: back"),
            Mode::Role(_) => ("role".into(), "↑↓ · enter: map · esc: back"),
            Mode::Answer => ("answer".into(), "↑↓ · enter · esc: back"),
        }
    }

    fn list(&mut self, f: &mut Frame, area: Rect, rows: &[(String, String)], focus: bool) {
        self.offset = crate::dropdown::fit(self.offset, self.cur, rows.len(), usize::from(area.height));
        for (k, (label, detail)) in rows.iter().enumerate().skip(self.offset).take(usize::from(area.height)) {
            let r = Rect { y: area.y + (k - self.offset) as u16, height: 1, ..area };
            let on = k == self.cur;
            let st = if on { if focus { theme::pill().add_modifier(Modifier::BOLD) } else { theme::panel().add_modifier(Modifier::BOLD) } } else { theme::text() };
            let lw = 28.min(usize::from(r.width) / 2);
            let line = Line::from(vec![Span::styled(format!(" {:<lw$} ", crate::nav::fit(label, lw)), st), Span::styled(crate::nav::fit(detail, usize::from(r.width).saturating_sub(lw + 3)), if on { st } else { theme::muted() })]);
            f.render_widget(Paragraph::new(line).style(if on { st } else { Style::default() }), r);
            self.rows.push((r, k));
        }
    }

    fn draw_steps(&mut self, f: &mut Frame, inner: Rect, focus: bool) {
        let Some(st) = self.state.clone() else { return };
        // roles
        let roles: Vec<Span> = {
            let mut v = vec![Span::styled("roles ", theme::muted())];
            let f2 = self.lib.flow(&st.flow).cloned().unwrap_or_default();
            for r in flows::roles(&f2, &self.nodes) {
                match st.roles.get(&r).cloned().or_else(|| (r == flows::LEAD).then(|| self.lead.clone()).flatten()) {
                    Some(a) => v.push(Span::styled(format!("@{r} {a}  "), theme::text())),
                    None => v.push(Span::styled(format!("@{r} ?  "), Style::default().fg(theme::pal().warn))),
                }
            }
            v
        };
        f.render_widget(Paragraph::new(Line::from(roles)), Rect { height: 1, ..inner });
        let warn_h = self.warnings.len().min(4) as u16;
        let detail_h = if self.detail { 8.min(inner.height / 2) } else { 0 };
        let list = Rect { y: inner.y + 2, height: inner.height.saturating_sub(2 + warn_h + detail_h), ..inner };
        let rows: Vec<flowrail::Row> = self
            .nodes
            .iter()
            .enumerate()
            .map(|(i, n)| {
                let last = st.history.iter().rev().find(|p| p.node == i);
                let (mark, ms) = if i == st.node && !st.finished {
                    if st.wait.is_some() { ("⏸", theme::accent_bold().fg(theme::pal().warn)) } else { ("●", theme::accent_bold()) }
                } else {
                    match last.map(|p| p.how.as_str()) {
                        Some("skipped") => ("⤼", theme::muted()),
                        Some("left") => ("·", theme::muted()),
                        Some(_) => ("✓", Style::default().fg(theme::pal().ok)),
                        None => ("○", theme::muted()),
                    }
                };
                // the lead's steps: 0 tag (most of them); another role: its agent
                let agent = if n.role == flows::LEAD { None } else { st.agent_of(n, self.lead.as_deref()) };
                let mut row = rail_row(n, mark, ms, agent);
                if let Some(p) = last.filter(|_| i != st.node || st.finished) {
                    row.tags.push((dur(p.ms), theme::muted()));
                }
                if self.warnings.iter().any(|(w, _)| *w == i) {
                    row.tags.push(("!".into(), Style::default().fg(theme::pal().danger)));
                }
                row
            })
            .collect();
        let arcs = flowrail::arcs(&self.nodes, |id| st.round(id));
        self.offset = crate::dropdown::fit(self.offset, self.cur, rows.len(), usize::from(list.height));
        self.rows = flowrail::draw(f.buffer_mut(), list, &rows, &arcs, Some(self.cur), focus, self.offset);
        let mut y = list.bottom();
        if detail_h > 0
            && let Some(n) = self.nodes.get(self.cur)
        {
            let r = Rect { y, height: detail_h, ..inner };
            let mut lines = vec![Line::styled("─".repeat(usize::from(r.width)), theme::line())];
            let head = format!("{} · {} · @{}{}", n.title, n.step.as_deref().map_or("one-off".to_string(), |s| format!("step {s}")), n.role, if n.gate { " · gate" } else { "" });
            lines.push(Line::styled(head, theme::bold()));
            for l in crate::shortcuts::expand(&n.text).lines() {
                lines.push(Line::styled(l.to_string(), theme::text()));
            }
            if !n.done.is_empty() {
                lines.push(Line::styled(format!("done when: {}", n.done), theme::muted()));
            }
            f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), r);
            y += detail_h;
        }
        for (i, (node, w)) in self.warnings.iter().take(4).enumerate() {
            let r = Rect { y: y + i as u16, height: 1, ..inner };
            f.render_widget(Paragraph::new(Line::styled(crate::nav::fit(&format!("! {}: {w}", node + 1), usize::from(r.width)), Style::default().fg(theme::pal().danger))), r);
        }
    }
}

/// A rail row of node N: MARK, its title, its tags (gate, @role → agent, one-off).
pub fn rail_row(n: &Resolved, mark: &str, ms: Style, agent: Option<String>) -> flowrail::Row {
    let mut tags = Vec::new();
    if n.gate {
        tags.push(("⏸".to_string(), Style::default().fg(theme::pal().warn)));
    }
    match agent {
        Some(a) => tags.push((format!("@{a}"), theme::muted())),
        None if n.role != flows::LEAD => tags.push((format!("@{}", n.role), theme::muted())),
        None => {}
    }
    if n.step.is_none() {
        tags.push(("◇".to_string(), theme::muted()));
    }
    flowrail::Row { mark: (mark.to_string(), ms), title: n.title.clone(), title_style: Style::default(), tags }
}

fn dur(ms: u64) -> String {
    let s = ms / 1000;
    if s < 60 { format!("{s}s") } else if s < 3600 { format!("{}m", s / 60) } else { format!("{}h{:02}", s / 3600, s / 60 % 60) }
}
