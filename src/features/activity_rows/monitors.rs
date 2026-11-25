//! Live harness watches, independent of the agent's foreground turn state.
use std::collections::HashSet;

use crate::{infra::background::Reads, ui::theme};
use ratatui::crossterm::event::{KeyCode, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use ratatui::{
    Frame,
    layout::{Position, Rect},
    text::Line,
    widgets::{Clear, Paragraph},
};

#[derive(Default)]
struct MonitorReads {
    /// None: the host did not answer in time (busy, e.g. a message to deliver): 0 change, 1 retry.
    reads: Reads<String, Option<Vec<Watch>>>,
    retry: bool,
    /// Bus topics: a file of the thread or of a kid changed → 1 read (0 timer).
    topics: Option<std::sync::mpsc::Receiver<crate::infra::bus::Topic>>,
    read_once: bool,
}

#[derive(Default)]
pub(crate) struct Monitors {
    pub watches: Vec<Watch>,
    reader: MonitorReads,
    open: bool,
    selected: usize,
    scroll: usize,
    bar: Rect,
    popup: Rect,
    hover: bool,
    /// Watches ended here (`d`): hidden until their transcript says so too.
    ended: HashSet<String>,
    /// A line for the view's toast (`d`: what ended, or why not).
    note: Option<String>,
    /// A 1st d: a 2nd d ends the selected watch.
    armed: bool,
}

impl Monitors {
    pub(crate) fn new(thread: &str) -> Self {
        Self {
            watches: crate::infra::background::snapshot(&format!("monitors-{thread}")).unwrap_or_default(),
            ..Default::default()
        }
    }
    pub(crate) fn tick(&mut self, thread: &str, kids: &[String]) -> bool {
        tick_monitors(self, thread, kids)
    }
    pub(crate) fn key(&mut self, key: KeyEvent) -> bool {
        key_monitors(self, key)
    }

    /// The last `d` result, once.
    pub(crate) fn take_note(&mut self) -> Option<String> {
        self.note.take()
    }

    /// Monitors shown: their row animates.
    pub(crate) fn active(&self) -> bool {
        !self.watches.is_empty()
    }

    /// Rows for the monitors area at WIDTH: 0 (0 watches), 1 (the row), or the row + the open
    /// box (its lines + 2 border rows, at most MAX in all).
    pub(crate) fn rows(&self, width: u16, max: u16) -> u16 {
        if self.watches.is_empty() {
            return 0;
        }

        if !self.open {
            return 1;
        }

        (1 + 2 + self.lines(width.saturating_sub(2).max(1) as usize).len() as u16).min(max.max(1))
    }

    /// The open box's text, wrapped to WIDTH: 1 row per watch, then the selected one's details.
    fn lines(&self, width: usize) -> Vec<Line<'static>> {
        lines_monitors(self, width)
    }

    pub(crate) fn mouse(&mut self, event: MouseEvent) -> bool {
        mouse_monitors(self, event)
    }

    /// AREA: the monitors area (`rows`): the open box on top, the monitors row at the bottom.
    pub(crate) fn render(&mut self, frame: &mut Frame, area: Rect, tick: usize) {
        render_monitors(self, frame, area, tick);
    }
}

#[cfg(test)]
#[path = "activity_rows_monitors_tests.rs"]
mod tests;

pub(crate) use crate::domain::monitors::*;

#[cfg(test)]
use serde_json::Value;
#[cfg(test)]
use std::process::Command;

/// `d`: end the selected watch (its process), then hide it.
fn end_selected(monitors: &mut Monitors) {
    let Some(watch) = monitors.watches.get(monitors.selected).cloned() else {
        return;
    };

    let killed = kill(&watch.command);

    monitors.note = Some(match killed {
        0 => format!(
            "monitor {}: 0 process found (it ended already?) · hidden",
            watch.description
        ),
        count => format!(
            "monitor ended: {} ({count} process{})",
            watch.description,
            if count == 1 { "" } else { "es" }
        ),
    });
    monitors.ended.insert(watch.key());
    monitors.watches.retain(|other| other.key() != watch.key());
    monitors.selected = monitors.selected.min(monitors.watches.len().saturating_sub(1));
    monitors.scroll = 0;

    if monitors.watches.is_empty() {
        monitors.open = false;
    }
}

fn select_clicked_monitor(event: MouseEvent, monitors: &mut Monitors) {
    let row = usize::from(event.row.saturating_sub(monitors.popup.y + 1)) + monitors.scroll;

    if row < monitors.watches.len() {
        monitors.selected = row;
        monitors.scroll = 0;
    }
}

fn monitor_style(i: usize, monitors: &Monitors) -> ratatui::prelude::Style {
    if i == monitors.selected {
        theme::accent_bold()
    } else {
        theme::text()
    }
}

fn tick_monitors(monitors: &mut Monitors, thread: &str, kids: &[String]) -> bool {
    let id = thread.to_string();

    // the 1st read, then 1 per change of the files of the thread or of a kid (KIDS)
    let topics = monitors.reader.topics.get_or_insert_with(crate::infra::bus::subscribe);
    let mut due = !std::mem::replace(&mut monitors.reader.read_once, true) | std::mem::take(&mut monitors.reader.retry);

    for topic in topics.try_iter() {
        if let crate::infra::bus::Topic::Thread(
            tid,
            crate::infra::bus::Part::Meta | crate::infra::bus::Part::Log | crate::infra::bus::Part::Other,
        ) = topic
        {
            due |= tid == id || kids.contains(&tid);
        }
    }

    // an expiry: the next draw drops it (below); a change: 1 read at once
    if due {
        monitors.reader.reads.forget(&id);
        monitors.reader.reads.request(id.clone(), move || load(&id));
    }

    let mut changed = false;

    for (_, watches) in monitors.reader.reads.poll() {
        match watches {
            Some(fresh) => {
                monitors.watches = fresh;
                changed = true;
            }
            None => monitors.reader.retry = true,
        }
    }

    monitors.watches.retain(|watch| !monitors.ended.contains(&watch.key()));
    let now = crate::engine::now_ms();
    let before = monitors.watches.len();

    monitors
        .watches
        .retain(|watch| watch.expires.is_none_or(|end| end > now));
    changed |= before != monitors.watches.len();
    monitors.selected = monitors.selected.min(monitors.watches.len().saturating_sub(1));

    if monitors.watches.is_empty() {
        monitors.open = false;
    }

    changed
}

fn key_monitors(monitors: &mut Monitors, key: KeyEvent) -> bool {
    if !monitors.open {
        return false;
    }

    let armed = std::mem::take(&mut monitors.armed);

    match key.code {
        KeyCode::Esc => monitors.open = false,
        KeyCode::Up => {
            monitors.selected = monitors.selected.saturating_sub(1);
            monitors.scroll = 0;
        }
        KeyCode::Down => {
            monitors.selected = (monitors.selected + 1).min(monitors.watches.len().saturating_sub(1));
            monitors.scroll = 0;
        }
        KeyCode::PageDown => monitors.scroll += 5,
        KeyCode::PageUp => monitors.scroll = monitors.scroll.saturating_sub(5),
        // d d: end the selected watch
        KeyCode::Char('d') if key.modifiers.is_empty() && armed => end_selected(monitors),
        KeyCode::Char('d') if key.modifiers.is_empty() => monitors.armed = true,
        // another key: the box closes, the key goes on (0 swallowed key)
        _ => {
            monitors.open = false;
            return false;
        }
    }

    true
}

fn lines_monitors(monitors: &Monitors, width: usize) -> Vec<Line<'static>> {
    let Some(watch) = monitors.watches.get(monitors.selected) else {
        return Vec::new();
    };

    let mut out: Vec<Line<'static>> = monitors
        .watches
        .iter()
        .enumerate()
        .map(|(i, watch_entry)| {
            let chars: Vec<_> = format!(
                "{} {} · {} · {}",
                if i == monitors.selected { "›" } else { " " },
                watch_entry.agent,
                watch_entry.harness,
                watch_entry.description
            )
            .chars()
            .collect();
            let (_, end) = crate::ui::textarea::wrap(&chars, width)[0];

            let style = monitor_style(i, monitors);

            Line::styled(chars[..end].iter().collect::<String>(), style)
        })
        .collect();

    let mut sections = vec![
        String::new(),
        format!("watch: {}", watch.description),
        format!("task: {}", watch.id),
        format!("method: {}", watch.command),
    ];

    if let Some(expires) = watch.expires {
        sections.push(format!(
            "expires in: {}s",
            expires.saturating_sub(crate::engine::now_ms()) / 1000
        ));
    }

    for section in sections {
        let chars: Vec<_> = section.chars().map(printable_character).collect();

        append_wrapped_lines(width, &mut out, &chars);
    }

    out
}

fn append_wrapped_lines(width: usize, out: &mut Vec<Line<'static>>, chars: &[char]) {
    for line in chars.split(|&character| character == '\n') {
        for (start, end) in crate::ui::textarea::wrap(line, width) {
            out.push(Line::styled(
                line[start..end].iter().collect::<String>(),
                theme::muted(),
            ));
        }
    }
}

fn printable_character(character: char) -> char {
    if character.is_control() && character != '\n' {
        ' '
    } else {
        character
    }
}

fn mouse_monitors(monitors: &mut Monitors, event: MouseEvent) -> bool {
    let at = Position {
        x: event.column,
        y: event.row,
    };

    monitors.hover = monitors.bar.contains(at);

    if matches!(event.kind, MouseEventKind::Down(MouseButton::Left)) && monitors.bar.contains(at) {
        monitors.open = !monitors.open;
        return true;
    }

    if monitors.open && monitors.popup.contains(at) {
        match event.kind {
            MouseEventKind::ScrollDown => monitors.scroll += 1,
            MouseEventKind::ScrollUp => monitors.scroll = monitors.scroll.saturating_sub(1),
            MouseEventKind::Down(MouseButton::Left) => {
                self::select_clicked_monitor(event, monitors);
            }
            _ => {}
        }

        return true;
    }

    false
}

fn render_monitors(monitors: &mut Monitors, frame: &mut Frame, area: Rect, tick: usize) {
    monitors.bar = Rect::default();
    monitors.popup = Rect::default();

    if monitors.watches.is_empty() || area.is_empty() {
        return;
    }

    let anchor = Rect {
        y: area.bottom() - 1,
        height: 1,
        ..area
    };

    monitors.bar = anchor;
    frame.render_widget(Clear, anchor);
    let descriptions = monitors
        .watches
        .iter()
        .map(|watch| format!("{}: {}", watch.agent, watch.description))
        .collect::<Vec<_>>()
        .join(" · ");
    let wave = ["◐", "◓", "◑", "◒"][tick % 4];

    let style = if monitors.hover {
        theme::accent().bg(theme::pal().surface1)
    } else {
        theme::accent()
    };

    let hint = if monitors.open {
        "click: close"
    } else {
        "click: details"
    };

    frame.render_widget(
        Paragraph::new(format!(
            "{wave} monitors ({}) · {descriptions} · {hint}",
            monitors.watches.len()
        ))
        .style(style),
        anchor,
    );

    if !monitors.open || area.height < 4 {
        return;
    }

    let popup_area = Rect {
        height: area.height - 1,
        ..area
    };

    monitors.popup = popup_area;

    let hint = if monitors.armed {
        " d again: end the watch · another key: no "
    } else {
        " ↑↓ select · d d end · pgup/pgdn scroll · esc close "
    };

    let inner = super::popup::draw_frame(frame, popup_area, " monitors ", hint);
    let lines = monitors.lines(inner.width.max(1) as usize);

    super::popup::draw_lines(frame, inner, lines, &mut monitors.scroll, theme::text());
}
