//! A row of tabs, 1 picked: the repos of the threads tab, the PR states of the merge list, the
//! repos of the changes view. ←→ (the owner's widget routes them) move the pick; a click picks.
//! A row too narrow for every tab: the picked tab stays in view, ‹ › mark the hidden ones.

use ratatui::Frame;
use ratatui::layout::{Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use unicode_width::UnicodeWidthStr;

use crate::ui::theme;

#[derive(Default)]
pub(crate) struct TabGroup {
    /// The picked tab.
    pub i: usize,
    /// 1st tab in view (a narrow row).
    first: usize,
    /// Last draw: each tab's area and index.
    rects: Vec<(Rect, usize)>,
}

impl TabGroup {
    /// D steps over N tabs, wrapping. True: the pick changed.
    pub(crate) fn step(&mut self, directory: isize, count: usize) -> bool {
        if count == 0 {
            return false;
        }

        let before = self.i;

        self.i = (self.i.min(count - 1) as isize + directory).rem_euclid(count as isize) as usize;
        self.i != before
    }

    /// Tab I's area in the last draw.
    #[cfg(test)]
    pub(crate) fn rect_of(&self, i: usize) -> Option<Rect> {
        self.rects.iter().find(|(_, key)| *key == i).map(|(result, _)| *result)
    }

    /// AT on a tab (last draw).
    pub(crate) fn hit(&self, at: Position) -> bool {
        self.rects.iter().any(|(result, _)| result.contains(at))
    }

    /// The x right of the last tab in view (last draw).
    pub(crate) fn end(&self) -> Option<u16> {
        self.rects.last().map(|(result, _)| result.right())
    }

    /// A click at AT on a tab: it is picked. True: a tab was hit.
    pub(crate) fn click(&mut self, at: Position) -> bool {
        match self.rects.iter().find(|(result, _)| result.contains(at)) {
            Some(&(_, i)) => {
                self.i = i;
                true
            }
            None => false,
        }
    }

    /// LABELS as tabs on row AREA (its 1st line), FOCUSED: the picked tab as an accent pill (else
    /// a panel pill). LEAD: muted text before the tabs (`repo`).
    pub(crate) fn draw(&mut self, frame: &mut Frame, area: Rect, lead: &str, labels: &[String], focused: bool) {
        self.rects.clear();

        if area.width == 0 || area.height == 0 || labels.is_empty() {
            return;
        }

        self.i = self.i.min(labels.len() - 1);
        let row = Rect { height: 1, ..area };
        let lead_w = if lead.is_empty() { 0 } else { lead.width() as u16 + 1 };
        let width = |i: usize| labels[i].width() as u16 + 2;
        let room = row.width.saturating_sub(lead_w + 4);

        // the window [first, last): holds the pick, from FIRST as far as it fits
        self.first = self.first.min(self.i);
        let fits = |first: usize, upto: usize| (first..=upto).map(|key| width(key) + 1).sum::<u16>() <= room;

        while self.first < self.i && !fits(self.first, self.i) {
            self.first += 1;
        }

        let mut last = self.first;
        let mut used = 0;

        while last < labels.len() && used + width(last) < room.max(width(self.first) + 1) {
            used += width(last) + 1;
            last += 1;
        }

        let mut spans = Vec::new();

        if !lead.is_empty() {
            spans.push(Span::styled(format!("{lead} "), theme::muted()));
        }

        spans.push(Span::styled(if self.first > 0 { "‹ " } else { "  " }, theme::muted()));
        let mut x = row.x + lead_w + 2;

        for (key_value, label) in labels.iter().enumerate().take(last).skip(self.first) {
            let text = format!(" {label} ");
            let width_value = width(key_value);

            let st = match (key_value == self.i, focused) {
                (true, true) => theme::pill().add_modifier(Modifier::BOLD),
                (true, false) => theme::panel().add_modifier(Modifier::BOLD),
                _ => theme::muted(),
            };

            self.rects.push((
                Rect {
                    x,
                    y: row.y,
                    width: width_value.min(row.right().saturating_sub(x)),
                    height: 1,
                },
                key_value,
            ));
            spans.push(Span::styled(text, st));
            spans.push(Span::raw(" "));
            x += width_value + 1;
        }

        if last < labels.len() {
            spans.push(Span::styled("›", theme::muted()));
        }

        frame.render_widget(Paragraph::new(Line::from(spans)).style(Style::default()), row);
    }
}

#[cfg(test)]
#[path = "tab_group_tests.rs"]
mod tests;
