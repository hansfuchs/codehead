//! A row of tabs, 1 picked: the repos of the threads tab, the PR states of the merge list, the
//! repos of the changes view. ←→ (the owner's widget routes them) move the pick; a click picks.
//! A row too narrow for every tab: the picked tab stays in view, ‹ › mark the hidden ones.

use ratatui::Frame;
use ratatui::layout::{Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use unicode_width::UnicodeWidthStr;

use crate::theme;

#[derive(Default)]
pub struct TabGroup {
    /// The picked tab.
    pub i: usize,
    /// 1st tab in view (a narrow row).
    first: usize,
    /// Last draw: each tab's area and index.
    rects: Vec<(Rect, usize)>,
}

impl TabGroup {
    /// D steps over N tabs, wrapping. True: the pick changed.
    pub fn step(&mut self, d: isize, n: usize) -> bool {
        if n == 0 {
            return false;
        }
        let before = self.i;
        self.i = (self.i.min(n - 1) as isize + d).rem_euclid(n as isize) as usize;
        self.i != before
    }

    /// Tab I's area in the last draw.
    #[cfg(test)]
    pub fn rect_of(&self, i: usize) -> Option<Rect> {
        self.rects.iter().find(|(_, k)| *k == i).map(|(r, _)| *r)
    }

    /// AT on a tab (last draw).
    pub fn hit(&self, at: Position) -> bool {
        self.rects.iter().any(|(r, _)| r.contains(at))
    }

    /// A click at AT on a tab: it is picked. True: a tab was hit.
    pub fn click(&mut self, at: Position) -> bool {
        match self.rects.iter().find(|(r, _)| r.contains(at)) {
            Some(&(_, i)) => {
                self.i = i;
                true
            }
            None => false,
        }
    }

    /// LABELS as tabs on row AREA (its 1st line), FOCUSED: the picked tab as an accent pill (else
    /// a panel pill). LEAD: muted text before the tabs (`repo`).
    pub fn draw(&mut self, f: &mut Frame, area: Rect, lead: &str, labels: &[String], focused: bool) {
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
        let fits = |first: usize, upto: usize| (first..=upto).map(|k| width(k) + 1).sum::<u16>() <= room;
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
        for (k, label) in labels.iter().enumerate().take(last).skip(self.first) {
            let text = format!(" {label} ");
            let w = width(k);
            let st = match (k == self.i, focused) {
                (true, true) => theme::pill().add_modifier(Modifier::BOLD),
                (true, false) => theme::panel().add_modifier(Modifier::BOLD),
                _ => theme::muted(),
            };
            self.rects.push((Rect { x, y: row.y, width: w.min(row.right().saturating_sub(x)), height: 1 }, k));
            spans.push(Span::styled(text, st));
            spans.push(Span::raw(" "));
            x += w + 1;
        }
        if last < labels.len() {
            spans.push(Span::styled("›", theme::muted()));
        }
        f.render_widget(Paragraph::new(Line::from(spans)).style(Style::default()), row);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(n: usize) -> Vec<String> {
        (0..n).map(|i| format!("repo-{i}")).collect()
    }

    fn row(g: &mut TabGroup, w: u16, l: &[String]) -> String {
        let mut term = ratatui::Terminal::new(ratatui::backend::TestBackend::new(w, 1)).unwrap();
        term.draw(|f| g.draw(f, f.area(), "repo", l, true)).unwrap();
        let buf = term.backend().buffer().clone();
        (0..w).map(|x| buf[(x, 0)].symbol().to_string()).collect()
    }

    #[test]
    fn steps_wrap_clicks_pick_and_a_narrow_row_keeps_the_pick_in_view() {
        let l = labels(6);
        let mut g = TabGroup::default();
        assert!(g.step(-1, 6));
        assert_eq!(g.i, 5);
        assert!(g.step(1, 6));
        assert_eq!(g.i, 0);
        let wide = row(&mut g, 120, &l);
        assert!(wide.contains(" repo-0 ") && wide.contains("repo-5") && !wide.contains('‹'), "{wide}");
        // narrow: the pick (5) in view, ‹ for the hidden ones before it
        g.i = 5;
        let narrow = row(&mut g, 40, &l);
        assert!(narrow.contains("repo-5") && narrow.contains('‹') && !narrow.contains("repo-0"), "{narrow}");
        // a click on a drawn tab picks it
        let (r, k) = g.rects[0];
        assert!(g.click(Position { x: r.x, y: r.y }));
        assert_eq!(g.i, k);
        assert!(!g.click(Position { x: 0, y: 5 }));
    }
}
