//! Notifications: what needs the owner on a thread they do not look at.
//!
//! A background poll reads the tail of each open thread's log (only logs that changed) and the
//! remembered PRs of each thread (every 3 min), and keeps the notes that hold now: a resolved
//! state (an answered ask, a blocked agent at work again) drops its note. A note shows while its
//! time is after the thread's seen mark; the thread view moves the mark when the owner has the
//! event on screen for 1 s. The header shows the counts, ctrl+n the list.

use std::time::{Duration, Instant};

use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph};

use crate::ui::theme;

pub(crate) use crate::domain::notifications::*;

pub(crate) fn is_key(key: &KeyEvent) -> bool {
    key.code == KeyCode::Char('n') && key.modifiers == KeyModifiers::CONTROL
}

#[derive(Default)]
pub(crate) struct Panel {
    pub notes: Vec<Note>,
    cursor: usize,
    rows: Vec<(Rect, usize)>,
}

pub(crate) enum Pick {
    /// Keep the panel open.
    Stay,
    Close,
    /// Enter: jump to the note's event in its thread.
    Jump(Note),
}

impl Panel {
    pub(crate) fn open(except: Option<&str>) -> Panel {
        Panel {
            notes: unseen(except),
            ..Default::default()
        }
    }

    pub(crate) fn key(&mut self, key_value: KeyEvent) -> Pick {
        let count = self.notes.len();

        match key_value.code {
            KeyCode::Esc => Pick::Close,
            _ if is_key(&key_value) => Pick::Close,
            KeyCode::Up | KeyCode::Char('k') => {
                self.cursor = self.cursor.saturating_sub(1);
                Pick::Stay
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.cursor = (self.cursor + 1).min(count.saturating_sub(1));
                Pick::Stay
            }
            KeyCode::Enter => self.notes.get(self.cursor).cloned().map_or(Pick::Close, Pick::Jump),
            // x: mark seen without a jump
            KeyCode::Char('x') if count > 0 => mark_selected_seen(self, count),
            _ => Pick::Stay,
        }
    }

    /// A click at AT: the note's jump.
    pub(crate) fn click(&mut self, at: ratatui::layout::Position) -> Pick {
        match self.rows.iter().find(|(result, _)| result.contains(at)) {
            Some(&(_, i)) => self.notes.get(i).cloned().map_or(Pick::Close, Pick::Jump),
            None => Pick::Close,
        }
    }

    /// The panel at the top right of AREA, below its 1st row.
    pub(crate) fn draw(&mut self, frame: &mut Frame, area: Rect) {
        let width_value = area.width.saturating_sub(4).min(76);
        let rows = (self.notes.len().max(1) as u16 * 2).min(area.height.saturating_sub(5));
        let area_value = Rect {
            x: area.right().saturating_sub(width_value + 1),
            y: area.y + 1,
            width: width_value,
            height: rows + 3,
        };

        frame.render_widget(Clear, area_value);
        let block = Block::new()
            .borders(Borders::ALL)
            .border_type(BorderType::Plain)
            .border_style(theme::accent())
            .title(Span::styled(
                format!(" notifications {} ", self.notes.len()),
                theme::accent_bold(),
            ))
            .title_bottom(Span::styled(
                " ↑↓ move · enter jump · x seen · esc close ",
                theme::muted(),
            ))
            .style(theme::text());
        let inner = block.inner(area_value);

        frame.render_widget(block, area_value);
        self.rows.clear();

        if self.notes.is_empty() {
            frame.render_widget(
                Paragraph::new(Span::styled(" 0 notifications", theme::muted())),
                Rect { height: 1, ..inner },
            );
            return;
        }

        let fits = (inner.height / 2).max(1) as usize;

        self.cursor = self.cursor.min(self.notes.len() - 1);
        let top = self.cursor.saturating_sub(fits - 1);
        let tw = inner.width.saturating_sub(2) as usize;

        for (slot, (i, count_value)) in self.notes.iter().enumerate().skip(top).take(fits).enumerate() {
            let y = inner.y + slot as u16 * 2;
            let row = Rect {
                x: inner.x,
                y,
                width: inner.width,
                height: 2,
            };
            let cur = i == self.cursor;

            draw_note(frame, row, count_value, cur, tw);
            self.rows.push((row, i));
        }
    }
}

/// The thread view's seen tracker: the newest event ts on screen counts once it stayed 1 s.
#[derive(Default)]
pub(crate) struct Watch {
    pending: Option<(u64, Instant)>,
}

impl Watch {
    /// THREAD shows events up to TS now (None: the bottom, every event). True: a mark moved.
    pub(crate) fn on_screen(&mut self, thread: &str, ts: Option<u64>) {
        let ts = ts.unwrap_or(u64::MAX);

        let Some((pending, at)) = self.pending.filter(|(pending, _)| *pending == ts) else {
            self.pending = Some((ts, Instant::now()));
            return;
        };

        if at.elapsed() < Duration::from_secs(1) {
            return;
        }

        if ts == u64::MAX {
            mark_all_seen(thread);
        } else {
            mark_seen(thread, ts);
        }

        self.pending = Some((pending, at));
    }
}

#[cfg(test)]
#[path = "notifications_tests.rs"]
mod tests;

pub(crate) use crate::ui::notification_badge::{badge, width};

#[cfg(test)]
use std::path::PathBuf;

fn draw_note(frame: &mut Frame, row: Rect, count_value: &Note, cur: bool, tw: usize) {
    if cur {
        frame.render_widget(
            Paragraph::new("").style(Style::default().bg(theme::pal().surface0)),
            row,
        );
    }

    let when = crate::ui::thread_card::ago(count_value.ts);
    let head = format!("{} {} ", count_value.kind.icon(), count_value.kind.label());
    let room = tw.saturating_sub(head.chars().count() + when.chars().count() + 3);
    let l1 = Line::from(vec![
        Span::styled(
            format!(" {head}"),
            Style::default()
                .fg(count_value.kind.color())
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            crate::ui::thread_card::fit(&count_value.title, room),
            if cur { theme::bold() } else { theme::text() },
        ),
        Span::styled(format!(" · {when}"), theme::muted()),
    ]);
    let l2 = Line::from(Span::styled(
        format!(
            "   {}",
            crate::ui::thread_card::fit(&count_value.text, tw.saturating_sub(2))
        ),
        theme::muted(),
    ));

    frame.render_widget(Paragraph::new(vec![l1, l2]), row);
}

fn mark_selected_seen(panel: &mut Panel, count: usize) -> Pick {
    let note = panel.notes.remove(panel.cursor.min(count - 1));

    mark_seen(&note.thread, note.ts);
    panel.cursor = panel.cursor.min(panel.notes.len().saturating_sub(1));

    if panel.notes.is_empty() {
        Pick::Close
    } else {
        Pick::Stay
    }
}
