use std::collections::HashSet;

use super::app_state::NoteListState;
use super::{App, Frame, Line, Paragraph, Rect, Row, Span, Style, theme};

impl App {
    pub(super) fn draw_notes(&mut self, frame: &mut Frame, area: Rect) {
        self.note_list
            .draw(frame, area, self.tabs_focus, self.thread_list.tlist.filtering);
    }
}

impl NoteListState {
    fn draw(&mut self, frame: &mut Frame, area: Rect, tabs_focus: bool, thread_filtering: bool) {
        let height = usize::from(area.height);

        if self.rows.is_empty() {
            self.draw_message(frame, area);
            return;
        }

        if self.cursor < self.offset {
            self.offset = self.cursor;
        } else if self.cursor >= self.offset + height {
            self.offset = self.cursor + 1 - height;
        }

        self.offset = self.offset.min(self.rows.len().saturating_sub(height));
        let selected: HashSet<&String> = self.selected.iter().collect();

        for (index, row) in self.rows.iter().enumerate().skip(self.offset).take(height) {
            let highlighted = index == self.cursor;
            let pill = highlighted && !(tabs_focus || self.filter_editing);
            let style = row_style(highlighted, tabs_focus || self.filter_editing || thread_filtering);
            let spans = note_spans(self, row, &selected, pill);
            let area = Rect {
                y: area.y + (index - self.offset) as u16,
                height: 1,
                ..area
            };

            frame.render_widget(Paragraph::new(Line::from(spans)).style(style), area);
        }
    }

    fn draw_message(&mut self, frame: &mut Frame<'_>, area: Rect) {
        let message = if self.filter.is_some() { "0 matches" } else { "0 notes" };

        frame.render_widget(Paragraph::new(Line::styled(message, theme::muted())), area);
    }
}

fn note_spans(notes: &NoteListState, row: &Row, selected: &HashSet<&String>, pill: bool) -> Vec<Span<'static>> {
    let muted = row_foreground(theme::muted(), pill);
    let mut spans = vec![Span::raw(" ")];

    match row {
        Row::Dir {
            path,
            name,
            depth,
            count,
            collapsed,
        } => {
            let count_selected = notes
                .selected
                .iter()
                .filter(|selected| selected.starts_with(path.as_str()))
                .count();

            spans.push(Span::raw("  ".repeat(*depth)));
            spans.push(Span::styled(if *collapsed { "▸ " } else { "▾ " }, muted));
            spans.push(Span::styled(format!("{name}/"), row_foreground(theme::bold(), pill)));
            spans.push(Span::styled(format!(" {count}"), muted));

            if count_selected > 0 {
                spans.push(Span::raw(" "));
                spans.push(Span::styled(format!(" ◼ {count_selected} "), theme::badge(pill)));
            }
        }
        Row::File { path, name, depth } => {
            spans.push(Span::raw("  ".repeat(*depth)));
            spans.push(note_mark(selected.contains(path), pill));
            spans.push(Span::styled(name.clone(), row_foreground(theme::text(), pill)));
            spans.push(Span::styled(format!(" {}", notes.notes.age(path)), muted));
        }
        Row::Hit { path, snippet } => {
            spans.push(note_mark(selected.contains(path), pill));
            spans.push(Span::styled(path.clone(), row_foreground(theme::text(), pill)));

            let extra = if snippet.is_empty() {
                notes.notes.age(path)
            } else {
                snippet.clone()
            };

            spans.push(Span::styled(format!("  {extra}"), muted));
        }
    }

    spans
}

fn note_mark(selected: bool, pill: bool) -> Span<'static> {
    if selected {
        Span::styled("◼ ", row_foreground(theme::accent(), pill))
    } else {
        Span::styled("◻ ", row_foreground(theme::line(), pill))
    }
}

pub(super) fn row_style(highlighted: bool, inactive: bool) -> Style {
    match (highlighted, inactive) {
        (true, false) => theme::pill(),
        (true, true) => theme::panel(),
        _ => Style::default(),
    }
}

pub(super) fn row_foreground(style: Style, pill: bool) -> Style {
    if pill { style.fg(theme::pal().on_accent) } else { style }
}
