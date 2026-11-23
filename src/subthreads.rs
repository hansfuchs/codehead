//! The sub-threads row of the thread view (above the subagents row): the running sub-threads of
//! the thread (`>sub`); a click opens a box with each one's agents, time and its last steps. `d`
//! stops the selected one. The row and its box go by themselves once 0 sub-thread runs: a
//! sub-thread reports its end to the thread and deletes itself.

use std::time::{Duration, Instant};

use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph};

use crate::engine::store::{self, Kind};
use crate::theme;

/// Steps of a sub-thread in the box.
const STEPS: usize = 4;

/// A running sub-thread as the row shows it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Kid {
    pub id: String,
    pub title: String,
    pub agents: Vec<String>,
    pub start: u64,
    pub busy: bool,
    /// An agent of it asks (permission, question): enter in the box opens it to answer.
    pub asking: bool,
    /// Its running watches (monitors, background commands): it waits for them.
    pub watches: usize,
    /// Its last steps, oldest first: agent texts (1st line), tool calls.
    pub steps: Vec<String>,
}

/// What a key asks the view to do.
#[derive(Clone, Debug, PartialEq)]
pub enum Act {
    None,
    /// Stop sub-thread ID.
    Stop(String),
    /// Open sub-thread ID (its view: answer its asks, follow it closely).
    Open(String),
}

#[derive(Default)]
pub struct SubThreads {
    pub kids: Vec<Kid>,
    open: bool,
    selected: Option<String>,
    scroll: usize,
    bar: Rect,
    popup: Rect,
    hover: bool,
    read: Option<std::sync::mpsc::Receiver<Vec<Kid>>>,
    read_at: Option<Instant>,
}

/// The 1st non-empty line of S, cut to N chars.
fn line(s: &str, n: usize) -> String {
    let l = s.lines().find(|l| !l.trim().is_empty()).unwrap_or_default().trim();
    if l.chars().count() > n { l.chars().take(n.saturating_sub(1)).collect::<String>() + "…" } else { l.to_string() }
}

/// The last steps of events EVENTS (texts, tool calls), oldest first.
pub fn steps(events: &[store::Event]) -> Vec<String> {
    let mut out: Vec<String> = events
        .iter()
        .filter_map(|e| {
            let who = e.agent.clone().unwrap_or_default();
            match &e.kind {
                Kind::Text { text } if !text.trim().is_empty() => Some(format!("{who}: {}", line(text, 200))),
                Kind::Tool { name, input, .. } => Some(format!("{who}: {}", crate::handoff::tool_line(name, input))),
                Kind::Error { message } => Some(format!("{who}: ✗ {}", line(message, 200))),
                _ => None,
            }
        })
        .collect();
    let cut = out.len().saturating_sub(STEPS);
    out.drain(..cut);
    out
}

/// The running sub-threads of thread MAIN, read from disk. A sub-thread whose host is gone a
/// while after its start: its end reported for it (lost), the thread deleted.
fn read(main: &str) -> Vec<Kid> {
    let Some(t) = store::load(main) else { return Vec::new() };
    let mut out = Vec::new();
    for id in &t.kids {
        // gone (it ended a moment ago): the thread's host drops it (`kids_check`, lost ones too)
        let Some(k) = store::load(id) else { continue };
        let alive = crate::engine::client::alive(id);
        let busy = alive && !crate::engine::client::state(id).1.is_empty();
        let events = store::events_tail(id, 1 << 16);
        out.push(Kid { id: id.clone(), title: k.title.clone(), agents: k.agents.clone(), start: k.created, busy, asking: asking(&events), watches: if alive { crate::monitors::watches(id).len() } else { 0 }, steps: steps(&events) });
    }
    out
}

/// An ask (permission, question) of EVENTS waits for its answer.
pub fn asking(events: &[store::Event]) -> bool {
    let mut open: Vec<&str> = Vec::new();
    for e in events {
        match &e.kind {
            Kind::Ask { id, .. } => open.push(id),
            Kind::Answer { id, .. } => open.retain(|x| x != id),
            Kind::TurnEnd { .. } => open.clear(),
            _ => {}
        }
    }
    !open.is_empty()
}

fn took(start: u64) -> String {
    let secs = crate::engine::now_ms().saturating_sub(start) / 1000;
    if secs >= 60 { format!("{}m{:02}s", secs / 60, secs % 60) } else { format!("{secs}s") }
}

impl SubThreads {
    pub fn active(&self) -> bool {
        !self.kids.is_empty()
    }

    /// Each second: the sub-threads of thread MAIN (in the background). True: news.
    pub fn tick(&mut self, main: &str) -> bool {
        let due = self.read.is_none() && self.read_at.is_none_or(|t| t.elapsed() >= Duration::from_secs(1));
        if due && !cfg!(test) {
            let (tx, rx) = std::sync::mpsc::channel();
            let main = main.to_string();
            std::thread::spawn(move || {
                let _ = tx.send(read(&main));
            });
            self.read = Some(rx);
            self.read_at = Some(Instant::now());
        }
        let mut changed = false;
        if let Some(kids) = self.read.as_ref().and_then(|r| r.try_recv().ok()) {
            self.read = None;
            changed = kids != self.kids;
            self.kids = kids;
        }
        // 0 running: the box closes, the row goes
        if self.kids.is_empty() && self.open {
            self.open = false;
            changed = true;
        }
        changed
    }

    /// A sub-thread just started (shown at once, before the next read).
    pub fn started(&mut self, k: Kid) {
        if !self.kids.iter().any(|x| x.id == k.id) {
            self.kids.push(k);
        }
    }

    /// A sub-thread stopped by the owner: out of the row at once.
    pub fn gone(&mut self, id: &str) {
        self.kids.retain(|k| k.id != id);
        if self.kids.is_empty() {
            self.open = false;
        }
    }

    fn current(&self) -> Option<&Kid> {
        self.kids.iter().find(|k| Some(&k.id) == self.selected.as_ref()).or(self.kids.first())
    }

    pub fn key(&mut self, k: KeyEvent) -> Option<Act> {
        if !self.open {
            return None;
        }
        let at = self.current().and_then(|c| self.kids.iter().position(|k| k.id == c.id)).unwrap_or(0);
        match k.code {
            KeyCode::Esc => self.open = false,
            KeyCode::Up => self.selected = self.kids.get(at.saturating_sub(1)).map(|k| k.id.clone()),
            KeyCode::Down => self.selected = self.kids.get((at + 1).min(self.kids.len().saturating_sub(1))).map(|k| k.id.clone()),
            KeyCode::PageDown => self.scroll += 5,
            KeyCode::PageUp => self.scroll = self.scroll.saturating_sub(5),
            KeyCode::Char('d') if k.modifiers.is_empty() => return Some(self.current().map_or(Act::None, |c| Act::Stop(c.id.clone()))),
            KeyCode::Enter => return Some(self.current().map_or(Act::None, |c| Act::Open(c.id.clone()))),
            _ => self.open = false,
        }
        Some(Act::None)
    }

    pub fn mouse(&mut self, m: MouseEvent) -> bool {
        let at = Position { x: m.column, y: m.row };
        self.hover = self.bar.contains(at);
        if matches!(m.kind, MouseEventKind::Down(MouseButton::Left)) && self.bar.contains(at) {
            self.open = !self.open;
            return true;
        }
        if self.open && self.popup.contains(at) {
            match m.kind {
                MouseEventKind::ScrollDown => self.scroll += 1,
                MouseEventKind::ScrollUp => self.scroll = self.scroll.saturating_sub(1),
                _ => {}
            }
            return true;
        }
        false
    }

    /// The box text at WIDTH: per sub-thread its title row (agents, time, state) and its last
    /// steps, a blank row between them.
    fn lines(&self, width: usize) -> Vec<Line<'static>> {
        let cur = self.current().map(|k| k.id.clone());
        let fit = |s: String, w: usize| -> String { if s.chars().count() > w { s.chars().take(w.saturating_sub(1)).collect::<String>() + "…" } else { s } };
        let mut out: Vec<Line<'static>> = Vec::new();
        for (i, k) in self.kids.iter().enumerate() {
            if i > 0 {
                out.push(Line::raw(""));
            }
            let sel = cur.as_deref() == Some(k.id.as_str());
            let state = if k.asking {
                "asks you · enter: open".to_string()
            } else if k.busy {
                "working".to_string()
            } else if k.watches > 0 {
                format!("waits on {} monitor{}", k.watches, if k.watches == 1 { "" } else { "s" })
            } else {
                "starting".to_string()
            };
            let tail = format!(" · @{} · {} · {state}", k.agents.join(" @"), took(k.start));
            let what = fit(format!("{}{}", if sel { "› " } else { "  " }, k.title), width.saturating_sub(tail.chars().count()));
            out.push(Line::from(vec![Span::styled(what, if sel { theme::accent_bold() } else { theme::text() }), Span::styled(tail, theme::muted())]));
            if k.steps.is_empty() {
                out.push(Line::styled("    ↳ starting…", theme::muted().add_modifier(Modifier::ITALIC)));
            }
            for s in &k.steps {
                out.push(Line::styled(fit(format!("    ↳ {s}"), width), theme::muted().add_modifier(Modifier::ITALIC)));
            }
        }
        out
    }

    /// Rows at WIDTH: 0 (0 running), 1 (the row) or the row + the open box (at most MAX).
    pub fn rows(&self, width: u16, max: u16) -> u16 {
        if !self.active() {
            return 0;
        }
        if !self.open {
            return 1;
        }
        (1 + 2 + self.lines(width.saturating_sub(4).max(1) as usize).len() as u16).min(max.max(1))
    }

    /// AREA (`rows`): the open box on top, the row at the bottom.
    pub fn render(&mut self, f: &mut Frame, area: Rect, tick: usize) {
        self.bar = Rect::default();
        self.popup = Rect::default();
        if !self.active() || area.is_empty() {
            return;
        }
        let anchor = Rect { y: area.bottom() - 1, height: 1, ..area };
        self.bar = anchor;
        f.render_widget(Clear, anchor);
        let wave = ["◐", "◓", "◑", "◒"][tick % 4];
        let style = if self.hover { theme::accent().bg(theme::pal().surface1) } else { theme::accent() };
        let what = self.kids.iter().map(|k| line(&k.title, 40)).collect::<Vec<_>>().join(" · ");
        let hint = if self.open { "click: close" } else { "click: details" };
        // the thread's messages to its agents wait for the end (same checkout): said here, 0 chat row
        f.render_widget(Paragraph::new(format!("{wave} sub-threads ({}) · {what} · messages wait · {hint}", self.kids.len())).style(style), anchor);
        if !self.open || area.height < 4 {
            return;
        }
        let r = Rect { height: area.height - 1, ..area };
        self.popup = r;
        f.render_widget(Clear, r);
        let block = Block::new()
            .borders(Borders::ALL)
            .border_type(BorderType::Plain)
            .border_style(theme::accent())
            .title(Span::styled(" sub-threads ", theme::accent_bold()))
            .title_bottom(Line::styled(" ↑↓ select · enter open · d stop · pgup/pgdn scroll · esc close ", theme::muted()).right_aligned());
        let inner = block.inner(r);
        f.render_widget(block, r);
        let inner = Rect { x: inner.x + 1, width: inner.width.saturating_sub(2), ..inner };
        let lines = self.lines(inner.width.max(1) as usize);
        self.scroll = self.scroll.min(lines.len().saturating_sub(inner.height as usize));
        f.render_widget(Paragraph::new(lines).scroll((self.scroll as u16, 0)).style(Style::default().fg(theme::pal().text)), inner);
    }
}

/// `>sub TASK` at the start of TEXT: the task (trimmed). None: 0 sub-thread.
pub fn task_of(text: &str) -> Option<String> {
    let rest = text.trim_start().strip_prefix(">sub")?;
    if !rest.is_empty() && !rest.starts_with(char::is_whitespace) {
        return None;
    }
    Some(rest.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::crossterm::event::KeyModifiers;

    fn kid(id: &str) -> Kid {
        Kid { id: id.into(), title: format!("job {id}"), agents: vec!["ADA".into()], start: crate::engine::now_ms(), busy: true, asking: false, watches: 0, steps: vec!["ADA: Bash git status".into()] }
    }

    #[test]
    fn task_of_reads_the_sub_prefix_only() {
        assert_eq!(task_of(">sub @ADA #commit").as_deref(), Some("@ADA #commit"));
        assert_eq!(task_of("  >sub  split the diff\nmore").as_deref(), Some("split the diff\nmore"));
        assert_eq!(task_of(">sub").as_deref(), Some(""));
        assert_eq!(task_of(">subway"), None);
        assert_eq!(task_of("go >sub x"), None);
    }

    #[test]
    fn the_row_shows_running_ones_and_goes_with_the_last() {
        let mut s = SubThreads::default();
        assert_eq!(s.rows(80, 20), 0);
        s.started(kid("a"));
        s.started(kid("b"));
        assert_eq!(s.rows(80, 20), 1);
        // a click on the row opens the box: title rows + steps, a blank row between
        s.bar = Rect::new(0, 5, 80, 1);
        assert!(s.mouse(MouseEvent { kind: MouseEventKind::Down(MouseButton::Left), column: 3, row: 5, modifiers: KeyModifiers::NONE }));
        let text: Vec<String> = s.lines(76).iter().map(|l| l.spans.iter().map(|x| x.content.to_string()).collect()).collect();
        assert!(text[0].starts_with("› job a") && text[0].contains("@ADA") && text[1].contains("↳ ADA: Bash git status"), "{text:?}");
        assert_eq!(s.rows(80, 20), 1 + 2 + 5);
        assert_eq!(s.key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)), Some(Act::None));
        assert_eq!(s.key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE)), Some(Act::Stop("b".into())));
        // the last one ends: the box closes, the row goes
        s.gone("a");
        s.gone("b");
        assert!(!s.open && s.rows(80, 20) == 0);
    }

    #[test]
    fn an_open_ask_shows_and_enter_opens_the_sub_thread() {
        let ev = |kind: Kind| store::Event { ts: 0, agent: Some("ADA".into()), kind };
        let ask = || ev(Kind::Ask { id: "q1".into(), tool: "Bash".into(), input: serde_json::Value::Null });
        assert!(asking(&[ask()]));
        assert!(!asking(&[ask(), ev(Kind::Answer { id: "q1".into(), allow: true, label: String::new() })]));

        let mut s = SubThreads::default();
        s.started(Kid { asking: true, ..kid("a") });
        s.open = true;
        let text: Vec<String> = s.lines(90).iter().map(|l| l.spans.iter().map(|x| x.content.to_string()).collect()).collect();
        assert!(text[0].contains("asks you · enter: open"), "{text:?}");
        assert_eq!(s.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)), Some(Act::Open("a".into())));
    }

    #[test]
    fn a_sub_thread_that_waits_on_a_monitor_says_so() {
        let mut s = SubThreads::default();
        s.started(Kid { busy: false, watches: 1, ..kid("a") });
        s.open = true;
        let text: Vec<String> = s.lines(90).iter().map(|l| l.spans.iter().map(|x| x.content.to_string()).collect()).collect();
        assert!(text[0].contains("waits on 1 monitor"), "{text:?}");
    }

    #[test]
    fn steps_keep_the_last_texts_and_tools() {
        let ev = |agent: &str, kind: Kind| store::Event { ts: 0, agent: Some(agent.into()), kind };
        let events: Vec<store::Event> = (0..6).map(|i| ev("ADA", Kind::Text { text: format!("step {i}\nmore") })).chain([ev("ADA", Kind::TurnStart)]).collect();
        let s = steps(&events);
        assert_eq!(s.len(), STEPS);
        assert_eq!(s.last().map(String::as_str), Some("ADA: step 5"));
    }
}
