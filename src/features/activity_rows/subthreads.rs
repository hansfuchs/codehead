//! The sub-threads row of the thread view (above the subagents row): the running sub-threads of
//! the thread (`>sub`); a click opens a box with each one's agents, time and its last steps. `d`
//! stops the selected one. The row and its box go by themselves once 0 sub-thread runs: a
//! sub-thread reports its end to the thread and deletes itself.

use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph};

use crate::engine::store::{self, Kind};
use crate::ui::theme;

/// Steps of a sub-thread in the box.
const STEPS: usize = 4;

/// A running sub-thread as the row shows it.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Kid {
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
pub(crate) enum Act {
    None,
    /// Stop sub-thread ID.
    Stop(String),
    /// Open sub-thread ID (its view: answer its asks, follow it closely).
    Open(String),
}

#[derive(Default)]
struct SubthreadReads {
    read: Option<std::sync::mpsc::Receiver<Vec<Kid>>>,
    /// Bus topics: a file of the main thread or of a kid changed → 1 read (0 timer).
    topics: Option<std::sync::mpsc::Receiver<crate::infra::bus::Topic>>,
    /// A read is due (the 1st one, a topic during a read).
    due: bool,
    read_once: bool,
}

#[derive(Default)]
pub(crate) struct SubThreads {
    pub kids: Vec<Kid>,
    open: bool,
    selected: Option<String>,
    scroll: usize,
    bar: Rect,
    popup: Rect,
    hover: bool,
    reader: SubthreadReads,
}

/// The 1st non-empty line of TEXT, cut to MAX chars.
fn line(text: &str, max: usize) -> String {
    let first = text
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or_default()
        .trim();

    if first.chars().count() > max {
        first.chars().take(max.saturating_sub(1)).collect::<String>() + "…"
    } else {
        first.to_string()
    }
}

/// The last steps of events EVENTS (texts, tool calls), oldest first.
pub(crate) fn steps(events: &[store::Event]) -> Vec<String> {
    let mut out: Vec<String> = events
        .iter()
        .filter_map(|event| {
            let who = event.agent.clone().unwrap_or_default();

            match &event.kind {
                Kind::Text { text } if !text.trim().is_empty() => Some(format!("{who}: {}", line(text, 200))),
                Kind::Tool { name, input, .. } => {
                    Some(format!("{who}: {}", crate::domain::handoff::tool_line(name, input)))
                }
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
    let Some(thread) = store::load(main) else {
        return Vec::new();
    };

    let mut out = Vec::new();

    for id in &thread.kids {
        // gone (it ended a moment ago): the thread's host drops it (`kids_check`, lost ones too)
        let Some(meta) = store::load(id) else { continue };
        let alive = crate::engine::client::alive(id);
        let busy = alive && !crate::engine::client::state(id).1.is_empty();
        let events = store::events_tail(id, 1 << 16);

        out.push(Kid {
            id: id.clone(),
            title: meta.title.clone(),
            agents: meta.agents.clone(),
            start: meta.created,
            busy,
            asking: asking(&events),
            watches: if alive {
                crate::features::activity_rows::monitors::watches(id).len()
            } else {
                0
            },
            steps: steps(&events),
        });
    }

    out
}

/// An ask (permission, question) of EVENTS waits for its answer.
pub(crate) fn asking(events: &[store::Event]) -> bool {
    let mut open: Vec<&str> = Vec::new();

    for event in events {
        match &event.kind {
            Kind::Ask { id, .. } => open.push(id),
            Kind::Answer { id, .. } => open.retain(|open_id| open_id != id),
            Kind::TurnEnd { .. } => open.clear(),
            _ => {}
        }
    }

    !open.is_empty()
}

fn took(start: u64) -> String {
    let secs = crate::engine::now_ms().saturating_sub(start) / 1000;

    if secs >= 60 {
        format!("{}m{:02}s", secs / 60, secs % 60)
    } else {
        format!("{secs}s")
    }
}

impl SubThreads {
    pub(crate) fn active(&self) -> bool {
        !self.kids.is_empty()
    }

    /// The sub-threads of thread MAIN (in the background): read once, then after each change of
    /// MAIN's or a kid's files (bus topics). True: news.
    pub(crate) fn tick(&mut self, main: &str) -> bool {
        tick_sub_threads(self, main)
    }

    /// A sub-thread just started (shown at once, before the next read).
    pub(crate) fn started(&mut self, kid: Kid) {
        if !self.kids.iter().any(|known| known.id == kid.id) {
            self.kids.push(kid);
        }
    }

    /// A sub-thread stopped by the owner: out of the row at once.
    pub(crate) fn gone(&mut self, id: &str) {
        self.kids.retain(|kid| kid.id != id);

        if self.kids.is_empty() {
            self.open = false;
        }
    }

    fn current(&self) -> Option<&Kid> {
        self.kids
            .iter()
            .find(|kid| Some(&kid.id) == self.selected.as_ref())
            .or(self.kids.first())
    }

    pub(crate) fn key(&mut self, key: KeyEvent) -> Option<Act> {
        key_sub_threads(self, key)
    }

    pub(crate) fn mouse(&mut self, event: MouseEvent) -> bool {
        mouse_sub_threads(self, event)
    }

    /// The box text at WIDTH: per sub-thread its title row (agents, time, state) and its last
    /// steps, a blank row between them.
    fn lines(&self, width: usize) -> Vec<Line<'static>> {
        lines_sub_threads(self, width)
    }

    /// Rows at WIDTH: 0 (0 running), 1 (the row) or the row + the open box (at most MAX).
    pub(crate) fn rows(&self, width: u16, max: u16) -> u16 {
        if !self.active() {
            return 0;
        }

        if !self.open {
            return 1;
        }

        (1 + 2 + self.lines(width.saturating_sub(4).max(1) as usize).len() as u16).min(max.max(1))
    }

    /// AREA (`rows`): the open box on top, the row at the bottom.
    pub(crate) fn render(&mut self, frame: &mut Frame, area: Rect, tick: usize) {
        render_sub_threads(self, frame, area, tick);
    }
}

/// `>sub TASK` at the start of TEXT: the task (trimmed). None: 0 sub-thread.
pub(crate) fn task_of(text: &str) -> Option<String> {
    let rest = text.trim_start().strip_prefix(">sub")?;

    if !rest.is_empty() && !rest.starts_with(char::is_whitespace) {
        return None;
    }

    Some(rest.trim().to_string())
}

#[cfg(test)]
#[path = "activity_rows_subthreads_tests.rs"]
mod tests;

fn topic_is_relevant(main: &str, topic: crate::infra::bus::Topic, kids: &[Kid]) -> bool {
    let crate::infra::bus::Topic::Thread(id, _) = topic else {
        return false;
    };

    id == main || kids.iter().any(|kid| kid.id == id)
}

fn tick_sub_threads(subthreads: &mut SubThreads, main: &str) -> bool {
    let topics = subthreads
        .reader
        .topics
        .get_or_insert_with(crate::infra::bus::subscribe);

    for topic in topics.try_iter() {
        subthreads.reader.due |= topic_is_relevant(main, topic, &subthreads.kids);
    }

    if (subthreads.reader.due || !subthreads.reader.read_once) && subthreads.reader.read.is_none() && !cfg!(test) {
        subthreads.reader.due = false;
        subthreads.reader.read_once = true;
        let (tx, rx) = crate::infra::bus::channel();
        let main = main.to_string();

        std::thread::spawn(move || {
            let _ = tx.send(read(&main));

            crate::infra::bus::wake();
        });

        subthreads.reader.read = Some(rx);
    }

    let mut changed = false;

    if let Some(kids) = subthreads.reader.read.as_ref().and_then(|read| read.try_recv().ok()) {
        subthreads.reader.read = None;
        changed = kids != subthreads.kids;
        subthreads.kids = kids;
    }

    // 0 running: the box closes, the row goes
    if subthreads.kids.is_empty() && subthreads.open {
        subthreads.open = false;
        changed = true;
    }

    changed
}

fn key_sub_threads(subthreads: &mut SubThreads, key: KeyEvent) -> Option<Act> {
    if !subthreads.open {
        return None;
    }

    let at = subthreads
        .current()
        .and_then(|current| subthreads.kids.iter().position(|kid| kid.id == current.id))
        .unwrap_or(0);

    match key.code {
        KeyCode::Up => {
            subthreads.selected = subthreads.kids.get(at.saturating_sub(1)).map(|kid| kid.id.clone());
        }
        KeyCode::Down => {
            subthreads.selected = subthreads
                .kids
                .get((at + 1).min(subthreads.kids.len().saturating_sub(1)))
                .map(|kid| kid.id.clone());
        }
        KeyCode::PageDown => subthreads.scroll += 5,
        KeyCode::PageUp => subthreads.scroll = subthreads.scroll.saturating_sub(5),
        KeyCode::Char('d') if key.modifiers.is_empty() => {
            return Some(subthreads.current().map_or(Act::None, |kid| Act::Stop(kid.id.clone())));
        }
        KeyCode::Enter => {
            return Some(subthreads.current().map_or(Act::None, |kid| Act::Open(kid.id.clone())));
        }
        _ => subthreads.open = false,
    }

    Some(Act::None)
}

fn mouse_sub_threads(subthreads: &mut SubThreads, event: MouseEvent) -> bool {
    let at = Position {
        x: event.column,
        y: event.row,
    };

    subthreads.hover = subthreads.bar.contains(at);

    if matches!(event.kind, MouseEventKind::Down(MouseButton::Left)) && subthreads.bar.contains(at) {
        subthreads.open = !subthreads.open;
        return true;
    }

    if subthreads.open && subthreads.popup.contains(at) {
        match event.kind {
            MouseEventKind::ScrollDown => subthreads.scroll += 1,
            MouseEventKind::ScrollUp => subthreads.scroll = subthreads.scroll.saturating_sub(1),
            _ => {}
        }

        return true;
    }

    false
}

fn lines_sub_threads(subthreads: &SubThreads, width: usize) -> Vec<Line<'static>> {
    let selected = subthreads.current().map(|kid| kid.id.clone());

    let mut out: Vec<Line<'static>> = Vec::new();

    for (i, kid) in subthreads.kids.iter().enumerate() {
        if i > 0 {
            out.push(Line::raw(""));
        }

        let selected_row = selected.as_deref() == Some(kid.id.as_str());

        let state = if kid.asking {
            "asks you · enter: open".to_string()
        } else if kid.busy {
            "working".to_string()
        } else if kid.watches > 0 {
            format!(
                "waits on {} monitor{}",
                kid.watches,
                if kid.watches == 1 { "" } else { "s" }
            )
        } else {
            "starting".to_string()
        };

        let tail = format!(" · @{} · {} · {state}", kid.agents.join(" @"), took(kid.start));
        let title = super::popup::truncate(
            format!("{}{}", if selected_row { "› " } else { "  " }, kid.title),
            width.saturating_sub(tail.chars().count()),
        );

        out.push(Line::from(vec![
            Span::styled(
                title,
                if selected_row {
                    theme::accent_bold()
                } else {
                    theme::text()
                },
            ),
            Span::styled(tail, theme::muted()),
        ]));

        if kid.steps.is_empty() {
            out.push(Line::styled(
                "    ↳ starting…",
                theme::muted().add_modifier(Modifier::ITALIC),
            ));
        }

        for step in &kid.steps {
            out.push(Line::styled(
                super::popup::truncate(format!("    ↳ {step}"), width),
                theme::muted().add_modifier(Modifier::ITALIC),
            ));
        }
    }

    out
}

fn render_sub_threads(subthreads: &mut SubThreads, frame: &mut Frame, area: Rect, tick: usize) {
    subthreads.bar = Rect::default();
    subthreads.popup = Rect::default();

    if !subthreads.active() || area.is_empty() {
        return;
    }

    let anchor = Rect {
        y: area.bottom() - 1,
        height: 1,
        ..area
    };

    subthreads.bar = anchor;
    frame.render_widget(Clear, anchor);
    let wave = ["◐", "◓", "◑", "◒"][tick % 4];

    let style = if subthreads.hover {
        theme::accent().bg(theme::pal().surface1)
    } else {
        theme::accent()
    };

    let what = subthreads
        .kids
        .iter()
        .map(|kid| line(&kid.title, 40))
        .collect::<Vec<_>>()
        .join(" · ");

    let hint = if subthreads.open {
        "click: close"
    } else {
        "click: details"
    };

    // the thread's messages to its agents wait for the end (same checkout): said here, 0 chat row
    frame.render_widget(
        Paragraph::new(format!(
            "{wave} sub-threads ({}) · {what} · messages wait · {hint}",
            subthreads.kids.len()
        ))
        .style(style),
        anchor,
    );

    if !subthreads.open || area.height < 4 {
        return;
    }

    let popup_area = Rect {
        height: area.height - 1,
        ..area
    };

    subthreads.popup = popup_area;
    let hint = " ↑↓ select · enter open · d stop · pgup/pgdn scroll · esc close ";
    let inner = super::popup::draw_frame(frame, popup_area, " sub-threads ", hint);
    let lines = subthreads.lines(inner.width.max(1) as usize);

    super::popup::draw_lines(
        frame,
        inner,
        lines,
        &mut subthreads.scroll,
        Style::default().fg(theme::pal().text),
    );
}
