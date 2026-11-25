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
use ratatui::widgets::{Clear, Paragraph};

use crate::domain::chat::Sub;
use crate::ui::theme;

/// What a key / click asks the view to do.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Act {
    None,
    /// Stop subagent ID (the host's `sub_stop`).
    Stop(String),
}

#[derive(Default)]
struct TranscriptReads {
    /// What each running subagent does now (id → its last step), read in the background.
    now: HashMap<String, String>,
    read: Option<std::sync::mpsc::Receiver<HashMap<String, String>>>,
    /// The transcript dirs of the running subagents, watched while they run (canonical dir,
    /// its guard).
    watched: Vec<(PathBuf, crate::infra::watch::Shown)>,
    topics: Option<std::sync::mpsc::Receiver<crate::infra::bus::Topic>>,
    /// A transcript changed (or a new subagent runs): 1 read.
    due: bool,
}

#[derive(Default)]
pub(crate) struct Subagents {
    open: bool,
    /// The selected subagent (its id).
    selected: Option<String>,
    scroll: usize,
    bar: Rect,
    popup: Rect,
    hover: bool,
    reader: TranscriptReads,
    /// Box rows of the last draw: (row in the box text, subagent id).
    rows: Vec<(usize, String)>,
    /// A 1st d: a 2nd d stops the selected subagent.
    armed: bool,
}

/// The running subagents of SUBS in box order: by master (first start first), each master's
/// tree depth first. (subagent, depth).
pub(crate) fn order(subs: &[Sub]) -> Vec<(&Sub, usize)> {
    let running: Vec<&Sub> = subs.iter().filter(|sub| sub.running()).collect();
    let mut masters: Vec<&str> = Vec::new();

    for subagent in &running {
        if !masters.contains(&subagent.master.as_str()) {
            masters.push(&subagent.master);
        }
    }

    let mut out = Vec::new();

    for master_name in masters {
        append_subtree(&running, None, master_name, 0, &mut out);

        // a subagent whose parent ended (or is unknown): at the top level of its master
        append_orphans(&running, &mut out, master_name);
    }

    out
}

fn append_orphans<'subs>(running: &[&'subs Sub], out: &mut Vec<(&'subs Sub, usize)>, master_name: &str) {
    for subagent in running.iter().filter(|candidate| {
        candidate.master == master_name
            && candidate
                .parent
                .as_deref()
                .is_some_and(|parent_id| !running.iter().any(|other| other.id == parent_id))
    }) {
        if !out.iter().any(|(placed, _)| placed.id == subagent.id) {
            out.push((subagent, 0));
            append_subtree(running, Some(&subagent.id), master_name, 1, out);
        }
    }
}

fn took(start: u64) -> String {
    let secs = crate::engine::now_ms().saturating_sub(start) / 1000;

    if secs >= 60 {
        format!("{}m{:02}s", secs / 60, secs % 60)
    } else {
        format!("{secs}s")
    }
}

impl Subagents {
    /// Running subagents shown: their row animates.
    pub(crate) fn active(subs: &[Sub]) -> bool {
        subs.iter().any(Sub::running)
    }

    /// What the running subagents of SUBS do now (their transcripts): read when a transcript
    /// changed (its dir is watched while it runs), 1 read at a time. True: news.
    pub(crate) fn tick(&mut self, subs: &[Sub]) -> bool {
        tick_subagents(self, subs)
    }

    #[cfg(test)]
    pub(crate) fn set_now(&mut self, id: &str, text: &str) {
        self.reader.now.insert(id.into(), text.into());
    }

    /// The selected subagent: the chosen one while it runs, else the 1st.
    fn current<'a>(&self, list: &[(&'a Sub, usize)]) -> Option<&'a Sub> {
        list.iter()
            .find(|(sub, _)| Some(&sub.id) == self.selected.as_ref())
            .or(list.first())
            .map(|(sub, _)| *sub)
    }

    pub(crate) fn key(&mut self, key: KeyEvent, subs: &[Sub]) -> Option<Act> {
        key_subagents(self, key, subs)
    }

    pub(crate) fn mouse(&mut self, event: MouseEvent) -> bool {
        mouse_subagents(self, event)
    }

    /// The box text at WIDTH: per master a heading, its subagents (2 rows each: what, now), a
    /// blank row between masters. Fills `rows`.
    fn lines(&mut self, subs: &[Sub], width: usize) -> Vec<Line<'static>> {
        lines_subagents(self, subs, width)
    }

    /// Rows for the subagents area at WIDTH: 0 (0 running), 1 (the row) or the row + the open box
    /// (at most MAX in all).
    pub(crate) fn rows(&mut self, subs: &[Sub], width: u16, max: u16) -> u16 {
        if !Self::active(subs) {
            return 0;
        }

        if !self.open {
            return 1;
        }

        (1 + 2 + self.lines(subs, width.saturating_sub(4).max(1) as usize).len() as u16).min(max.max(1))
    }

    /// AREA (`rows`): the open box on top, the subagents row at the bottom.
    pub(crate) fn render(&mut self, frame: &mut Frame, area: Rect, tick: usize, subs: &[Sub]) {
        render_subagents(self, frame, area, tick, subs);
    }
}

fn append_agent_type_tag(subagent: &Sub, tags: &mut Vec<String>) {
    if !subagent.agent_type.is_empty() {
        tags.push(subagent.agent_type.clone());
    }
}

#[cfg(test)]
#[path = "activity_rows_subagents_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "activity_rows_subagents_key_tests.rs"]
mod key_tests;

fn select_clicked_subagent(event: MouseEvent, subagents: &mut Subagents) {
    let row = usize::from(event.row.saturating_sub(subagents.popup.y + 1)) + subagents.scroll;

    if let Some((_, id)) = subagents.rows.iter().rev().find(|(start, _)| *start <= row) {
        subagents.selected = Some(id.clone());
    }
}

fn tick_subagents(subagents: &mut Subagents, subs: &[Sub]) -> bool {
    let TranscriptFiles {
        files,
        directories: dirs,
    } = watch_transcripts(&mut subagents.reader, subs);
    let topics = subagents.reader.topics.get_or_insert_with(crate::infra::bus::subscribe);

    for topic in topics.try_iter() {
        if let crate::infra::bus::Topic::Work(path) | crate::infra::bus::Topic::Rescan(path) = topic {
            subagents.reader.due |= dirs.iter().any(|dir| dir.starts_with(&path));
        }
    }

    if !files.is_empty() && subagents.reader.due && subagents.reader.read.is_none() && !cfg!(test) {
        subagents.reader.due = false;
        let (tx, rx) = crate::infra::bus::channel();

        std::thread::spawn(move || {
            let _ = tx.send(
                files
                    .into_iter()
                    .filter_map(|(id, transcript)| Some((id, crate::engine::subagents::activity_of(&transcript)?)))
                    .collect(),
            );

            crate::infra::bus::wake();
        });

        subagents.reader.read = Some(rx);
    }

    let mut changed = false;

    if let Some(now) = subagents.reader.read.as_ref().and_then(|read| read.try_recv().ok()) {
        subagents.reader.read = None;
        changed = now != subagents.reader.now;
        subagents.reader.now = now;
    }

    if !Subagents::active(subs) && subagents.open {
        subagents.open = false;
        changed = true;
    }

    changed
}

fn watch_transcripts(reader: &mut TranscriptReads, subs: &[Sub]) -> TranscriptFiles {
    let files: Vec<(String, PathBuf)> = subs
        .iter()
        .filter(|subagent| subagent.running() && !subagent.transcript.is_empty())
        .map(|sub| (sub.id.clone(), PathBuf::from(&sub.transcript)))
        .collect();
    let dirs: Vec<PathBuf> = files
        .iter()
        .filter_map(|(_, transcript)| transcript.parent().map(crate::infra::watch::canon))
        .collect();
    let before = reader.watched.len();

    reader.watched.retain(|(dir, _)| dirs.contains(dir));

    for dir in &dirs {
        if !reader.watched.iter().any(|(watched, _)| watched == dir) {
            reader.watched.push((dir.clone(), crate::infra::watch::show(dir)));
            reader.due = true;
        }
    }

    reader.due |= reader.watched.len() != before;

    TranscriptFiles {
        files,
        directories: dirs,
    }
}

fn key_subagents(subagents: &mut Subagents, key: KeyEvent, subs: &[Sub]) -> Option<Act> {
    if !subagents.open {
        return None;
    }

    let list = order(subs);
    let at = subagents
        .current(&list)
        .and_then(|current| list.iter().position(|(sub, _)| sub.id == current.id))
        .unwrap_or(0);
    let armed = std::mem::take(&mut subagents.armed);

    match key.code {
        KeyCode::Esc => subagents.open = false,
        KeyCode::Up => {
            subagents.selected = list.get(at.saturating_sub(1)).map(|(sub, _)| sub.id.clone());
        }
        KeyCode::Down => {
            subagents.selected = list
                .get((at + 1).min(list.len().saturating_sub(1)))
                .map(|(sub, _)| sub.id.clone());
        }
        KeyCode::PageDown => subagents.scroll += 5,
        KeyCode::PageUp => subagents.scroll = subagents.scroll.saturating_sub(5),
        // d d: stop the selected subagent
        KeyCode::Char('d') if key.modifiers.is_empty() && armed => {
            return Some(
                subagents
                    .current(&list)
                    .map_or(Act::None, |sub| Act::Stop(sub.id.clone())),
            );
        }
        KeyCode::Char('d') if key.modifiers.is_empty() => subagents.armed = true,
        // another key: the box closes, the key goes on (0 swallowed key)
        _ => {
            subagents.open = false;
            return None;
        }
    }

    Some(Act::None)
}

fn mouse_subagents(subagents: &mut Subagents, event: MouseEvent) -> bool {
    let at = Position {
        x: event.column,
        y: event.row,
    };

    subagents.hover = subagents.bar.contains(at);

    if matches!(event.kind, MouseEventKind::Down(MouseButton::Left)) && subagents.bar.contains(at) {
        subagents.open = !subagents.open;
        return true;
    }

    if subagents.open && subagents.popup.contains(at) {
        match event.kind {
            MouseEventKind::ScrollDown => subagents.scroll += 1,
            MouseEventKind::ScrollUp => subagents.scroll = subagents.scroll.saturating_sub(1),
            MouseEventKind::Down(MouseButton::Left) => {
                self::select_clicked_subagent(event, subagents);
            }
            _ => {}
        }

        return true;
    }

    false
}

fn lines_subagents(subagents: &mut Subagents, subs: &[Sub], width: usize) -> Vec<Line<'static>> {
    let list = order(subs);
    let selected = subagents
        .current(&list)
        .map(|current_subagent| current_subagent.id.clone());
    let mut out: Vec<Line<'static>> = Vec::new();

    subagents.rows.clear();
    let mut master_name: Option<&str> = None;

    for (subagent, depth) in &list {
        if master_name != Some(subagent.master.as_str()) {
            append_group_gap(&mut out, master_name);

            master_name = Some(&subagent.master);
            let group_count = list.iter().filter(|(sub, _)| sub.master == subagent.master).count();

            out.push(Line::from(vec![
                Span::styled(subagent.master.clone(), theme::bold()),
                Span::styled(
                    format!(" · {group_count} subagent{}", if group_count == 1 { "" } else { "s" }),
                    theme::muted(),
                ),
            ]));
        }

        let selected_row = selected.as_deref() == Some(subagent.id.as_str());
        let indentation = "  ".repeat(*depth);
        let mark = if selected_row { "› " } else { "  " };
        let mut description = format!("{mark}{indentation}{}", subagent.description);
        let mut tags: Vec<String> = Vec::new();

        append_agent_type_tag(subagent, &mut tags);

        if subagent.background {
            tags.push("background".into());
        }

        tags.push(took(subagent.start));
        let tail = format!(" · {}", tags.join(" · "));

        description = super::popup::truncate(description, width.saturating_sub(tail.chars().count()));
        subagents.rows.push((out.len(), subagent.id.clone()));

        let style = if selected_row {
            theme::accent_bold()
        } else {
            theme::text()
        };

        out.push(Line::from(vec![
            Span::styled(description, style),
            Span::styled(tail, theme::muted()),
        ]));

        let activity = activity_text(&subagents.reader, subagent);

        out.push(Line::styled(
            super::popup::truncate(format!("  {indentation}  ↳ {activity}"), width),
            theme::muted().add_modifier(Modifier::ITALIC),
        ));
    }

    out
}

fn append_group_gap(out: &mut Vec<Line<'static>>, master: Option<&str>) {
    if master.is_some() {
        out.push(Line::raw(""));
    }
}

fn render_subagents(subagents: &mut Subagents, frame: &mut Frame, area: Rect, tick: usize, subs: &[Sub]) {
    subagents.bar = Rect::default();
    subagents.popup = Rect::default();

    if !Subagents::active(subs) || area.is_empty() {
        return;
    }

    let list = order(subs);
    let anchor = Rect {
        y: area.bottom() - 1,
        height: 1,
        ..area
    };

    subagents.bar = anchor;
    frame.render_widget(Clear, anchor);
    let wave = ["◐", "◓", "◑", "◒"][tick % 4];

    let style = if subagents.hover {
        theme::accent().bg(theme::pal().surface1)
    } else {
        theme::accent()
    };

    let what = list
        .iter()
        .filter(|(_, depth)| *depth == 0)
        .map(|(sub, _)| format!("{}: {}", sub.master, sub.description))
        .collect::<Vec<_>>()
        .join(" · ");

    let hint = if subagents.open {
        "click: close"
    } else {
        "click: details"
    };

    frame.render_widget(
        Paragraph::new(format!("{wave} subagents ({}) · {what} · {hint}", list.len())).style(style),
        anchor,
    );

    if !subagents.open || area.height < 4 {
        return;
    }

    let popup_area = Rect {
        height: area.height - 1,
        ..area
    };

    subagents.popup = popup_area;

    let hint = if subagents.armed {
        " d again: stop the subagent · another key: no "
    } else {
        " ↑↓ select · d d stop · pgup/pgdn scroll · esc close "
    };

    let inner = super::popup::draw_frame(frame, popup_area, " subagents ", hint);
    let lines = subagents.lines(subs, inner.width.max(1) as usize);

    super::popup::draw_lines(
        frame,
        inner,
        lines,
        &mut subagents.scroll,
        Style::default().fg(theme::pal().text),
    );
}

fn append_subtree<'a>(
    running: &[&'a Sub],
    parent: Option<&str>,
    master: &str,
    depth: usize,
    out: &mut Vec<(&'a Sub, usize)>,
) {
    for subagent in running
        .iter()
        .filter(|sub| sub.master == master && sub.parent.as_deref() == parent)
    {
        out.push((subagent, depth));
        append_subtree(running, Some(&subagent.id), master, depth + 1, out);
    }
}

fn activity_text(reader: &TranscriptReads, sub: &Sub) -> String {
    reader.now.get(&sub.id).cloned().unwrap_or_else(|| {
        if sub.transcript.is_empty() {
            "running (0 steps reported by this harness)".into()
        } else {
            "starting…".into()
        }
    })
}

struct TranscriptFiles {
    files: Vec<(String, PathBuf)>,
    directories: Vec<PathBuf>,
}
