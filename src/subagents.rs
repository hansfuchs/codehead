//! The subagents row of the thread view (above the monitors row): the running subagents of the
//! agents; a click opens a box, grouped by master (a blank row between masters, nested subagents
//! indented under their parent), with what each does now (its transcript's last step). `d` stops
//! the selected one.

use std::collections::HashMap;
use std::path::PathBuf;

use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph};

use crate::stream::Sub;
use crate::theme;

/// What a key / click asks the view to do.
#[derive(Clone, Debug, PartialEq)]
pub enum Act {
    None,
    /// Stop subagent ID (the host's `sub_stop`).
    Stop(String),
}

#[derive(Default)]
pub struct Subagents {
    open: bool,
    /// The selected subagent (its id).
    selected: Option<String>,
    scroll: usize,
    bar: Rect,
    popup: Rect,
    hover: bool,
    /// What each running subagent does now (id → its last step), read in the background.
    now: HashMap<String, String>,
    read: Option<std::sync::mpsc::Receiver<HashMap<String, String>>>,
    read_at: Option<std::time::Instant>,
    /// Box rows of the last draw: (row in the box text, subagent id).
    rows: Vec<(usize, String)>,
    /// A 1st d: a 2nd d stops the selected subagent.
    armed: bool,
}

/// The running subagents of SUBS in box order: by master (first start first), each master's
/// tree depth first. (subagent, depth).
pub fn order(subs: &[Sub]) -> Vec<(&Sub, usize)> {
    let run: Vec<&Sub> = subs.iter().filter(|s| s.running()).collect();
    let mut masters: Vec<&str> = Vec::new();
    for s in &run {
        if !masters.contains(&s.master.as_str()) {
            masters.push(&s.master);
        }
    }
    fn walk<'a>(run: &[&'a Sub], parent: Option<&str>, master: &str, depth: usize, out: &mut Vec<(&'a Sub, usize)>) {
        for s in run.iter().filter(|s| s.master == master && s.parent.as_deref() == parent) {
            out.push((s, depth));
            walk(run, Some(&s.id), master, depth + 1, out);
        }
    }
    let mut out = Vec::new();
    for m in masters {
        walk(&run, None, m, 0, &mut out);
        // a subagent whose parent ended (or is unknown): at the top level of its master
        for s in run.iter().filter(|s| s.master == m && s.parent.as_deref().is_some_and(|p| !run.iter().any(|x| x.id == p))) {
            if !out.iter().any(|(x, _)| x.id == s.id) {
                out.push((s, 0));
                walk(&run, Some(&s.id), m, 1, &mut out);
            }
        }
    }
    out
}

fn took(start: u64) -> String {
    let secs = crate::engine::now_ms().saturating_sub(start) / 1000;
    if secs >= 60 { format!("{}m{:02}s", secs / 60, secs % 60) } else { format!("{secs}s") }
}

impl Subagents {
    /// Running subagents shown: their row animates.
    pub fn active(subs: &[Sub]) -> bool {
        subs.iter().any(Sub::running)
    }

    /// Each second: what the running subagents of SUBS do now (their transcripts). True: news.
    pub fn tick(&mut self, subs: &[Sub]) -> bool {
        let files: Vec<(String, PathBuf)> = subs.iter().filter(|s| s.running() && !s.transcript.is_empty()).map(|s| (s.id.clone(), PathBuf::from(&s.transcript))).collect();
        // 1 read per second at most, 1 at a time
        let due = self.read.is_none() && self.read_at.is_none_or(|t| t.elapsed() >= std::time::Duration::from_secs(1));
        if !files.is_empty() && due && !cfg!(test) {
            let (tx, rx) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                let _ = tx.send(files.into_iter().filter_map(|(id, p)| Some((id, crate::engine::subagents::activity_of(&p)?))).collect());
            });
            self.read = Some(rx);
            self.read_at = Some(std::time::Instant::now());
        }
        let mut changed = false;
        if let Some(now) = self.read.as_ref().and_then(|r| r.try_recv().ok()) {
            self.read = None;
            changed = now != self.now;
            self.now = now;
        }
        if !Self::active(subs) && self.open {
            self.open = false;
            changed = true;
        }
        changed
    }

    #[cfg(test)]
    pub fn set_now(&mut self, id: &str, text: &str) {
        self.now.insert(id.into(), text.into());
    }

    /// The selected subagent: the chosen one while it runs, else the 1st.
    fn current<'a>(&self, list: &[(&'a Sub, usize)]) -> Option<&'a Sub> {
        list.iter().find(|(s, _)| Some(&s.id) == self.selected.as_ref()).or(list.first()).map(|(s, _)| *s)
    }

    pub fn key(&mut self, k: KeyEvent, subs: &[Sub]) -> Option<Act> {
        if !self.open {
            return None;
        }
        let list = order(subs);
        let at = self.current(&list).and_then(|c| list.iter().position(|(s, _)| s.id == c.id)).unwrap_or(0);
        let armed = std::mem::take(&mut self.armed);
        match k.code {
            KeyCode::Esc => self.open = false,
            KeyCode::Up => self.selected = list.get(at.saturating_sub(1)).map(|(s, _)| s.id.clone()),
            KeyCode::Down => self.selected = list.get((at + 1).min(list.len().saturating_sub(1))).map(|(s, _)| s.id.clone()),
            KeyCode::PageDown => self.scroll += 5,
            KeyCode::PageUp => self.scroll = self.scroll.saturating_sub(5),
            // d d: stop the selected subagent
            KeyCode::Char('d') if k.modifiers.is_empty() && armed => return Some(self.current(&list).map_or(Act::None, |s| Act::Stop(s.id.clone()))),
            KeyCode::Char('d') if k.modifiers.is_empty() => self.armed = true,
            // another key: the box closes, the key goes on (0 swallowed key)
            _ => {
                self.open = false;
                return None;
            }
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
                MouseEventKind::Down(MouseButton::Left) => {
                    let row = usize::from(m.row.saturating_sub(self.popup.y + 1)) + self.scroll;
                    if let Some((_, id)) = self.rows.iter().rev().find(|(r, _)| *r <= row) {
                        self.selected = Some(id.clone());
                    }
                }
                _ => {}
            }
            return true;
        }
        false
    }

    /// The box text at WIDTH: per master a heading, its subagents (2 rows each: what, now), a
    /// blank row between masters. Fills `rows`.
    fn lines(&mut self, subs: &[Sub], width: usize) -> Vec<Line<'static>> {
        let list = order(subs);
        let cur = self.current(&list).map(|s| s.id.clone());
        let mut out: Vec<Line<'static>> = Vec::new();
        self.rows.clear();
        let mut master: Option<&str> = None;
        let fit = |s: String, w: usize| -> String { if s.chars().count() > w { s.chars().take(w.saturating_sub(1)).collect::<String>() + "…" } else { s } };
        for (s, depth) in &list {
            if master != Some(s.master.as_str()) {
                if master.is_some() {
                    out.push(Line::raw(""));
                }
                master = Some(&s.master);
                let n = list.iter().filter(|(x, _)| x.master == s.master).count();
                out.push(Line::from(vec![Span::styled(s.master.clone(), theme::accent_bold()), Span::styled(format!(" · {n} subagent{}", if n == 1 { "" } else { "s" }), theme::muted())]));
            }
            let sel = cur.as_deref() == Some(s.id.as_str());
            let pad = "  ".repeat(*depth);
            let mark = if sel { "› " } else { "  " };
            let mut what = format!("{mark}{pad}{}", s.description);
            let mut tags: Vec<String> = Vec::new();
            if !s.agent_type.is_empty() {
                tags.push(s.agent_type.clone());
            }
            if s.background {
                tags.push("background".into());
            }
            tags.push(took(s.start));
            let tail = format!(" · {}", tags.join(" · "));
            what = fit(what, width.saturating_sub(tail.chars().count()));
            self.rows.push((out.len(), s.id.clone()));
            let st = if sel { theme::accent_bold() } else { theme::text() };
            out.push(Line::from(vec![Span::styled(what, st), Span::styled(tail, theme::muted())]));
            let now = self.now.get(&s.id).cloned().unwrap_or_else(|| if s.transcript.is_empty() { "running (0 steps reported by this harness)".into() } else { "starting…".into() });
            out.push(Line::styled(fit(format!("  {pad}  ↳ {now}"), width), theme::muted().add_modifier(Modifier::ITALIC)));
        }
        out
    }

    /// Rows for the subagents area at WIDTH: 0 (0 running), 1 (the row) or the row + the open box
    /// (at most MAX in all).
    pub fn rows(&mut self, subs: &[Sub], width: u16, max: u16) -> u16 {
        if !Self::active(subs) {
            return 0;
        }
        if !self.open {
            return 1;
        }
        (1 + 2 + self.lines(subs, width.saturating_sub(4).max(1) as usize).len() as u16).min(max.max(1))
    }

    /// AREA (`rows`): the open box on top, the subagents row at the bottom.
    pub fn render(&mut self, f: &mut Frame, area: Rect, tick: usize, subs: &[Sub]) {
        self.bar = Rect::default();
        self.popup = Rect::default();
        if !Self::active(subs) || area.is_empty() {
            return;
        }
        let list = order(subs);
        let anchor = Rect { y: area.bottom() - 1, height: 1, ..area };
        self.bar = anchor;
        f.render_widget(Clear, anchor);
        let wave = ["◐", "◓", "◑", "◒"][tick % 4];
        let style = if self.hover { theme::accent().bg(theme::pal().hover) } else { theme::accent() };
        let what = list.iter().filter(|(_, d)| *d == 0).map(|(s, _)| format!("{}: {}", s.master, s.description)).collect::<Vec<_>>().join(" · ");
        let hint = if self.open { "click: close" } else { "click: details" };
        f.render_widget(Paragraph::new(format!("{wave} subagents ({}) · {what} · {hint}", list.len())).style(style), anchor);
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
            .title(Span::styled(" subagents ", theme::accent_bold()))
            .title_bottom(Line::styled(if self.armed { " d again: stop the subagent · another key: no " } else { " ↑↓ select · d d stop · pgup/pgdn scroll · esc close " }, theme::muted()).right_aligned());
        let inner = block.inner(r);
        f.render_widget(block, r);
        let inner = Rect { x: inner.x + 1, width: inner.width.saturating_sub(2), ..inner };
        let lines = self.lines(subs, inner.width.max(1) as usize);
        self.scroll = self.scroll.min(lines.len().saturating_sub(inner.height as usize));
        f.render_widget(Paragraph::new(lines).scroll((self.scroll as u16, 0)).style(Style::default().fg(theme::pal().text)), inner);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sub(id: &str, master: &str, parent: Option<&str>) -> Sub {
        Sub { id: id.into(), master: master.into(), parent: parent.map(String::from), description: format!("job {id}"), start: crate::engine::now_ms(), ..Default::default() }
    }

    #[test]
    fn order_groups_by_master_with_nested_ones_under_their_parent() {
        let mut done = sub("x", "ada", None);
        done.state = Some("done".into());
        let subs = [sub("a", "ada", None), sub("b", "bob", None), sub("c", "ada", Some("a")), done, sub("d", "ada", None), sub("e", "ada", Some("gone"))];
        let o: Vec<(&str, usize)> = order(&subs).into_iter().map(|(s, d)| (s.id.as_str(), d)).collect();
        assert_eq!(o, [("a", 0), ("c", 1), ("d", 0), ("e", 0), ("b", 0)]);
    }
}

#[cfg(test)]
mod key_tests {
    use super::*;
    use ratatui::crossterm::event::KeyModifiers;

    #[test]
    fn another_key_closes_the_box_and_goes_on_d_asks_first() {
        let mut s = Subagents { open: true, ..Default::default() };
        assert!(s.key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE), &[]).is_none(), "x: not the box's");
        assert!(!s.open);

        s.open = true;
        assert!(matches!(s.key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE), &[]), Some(Act::None)));
        assert!(s.armed && s.open);
    }
}
