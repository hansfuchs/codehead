use super::{
    App, Constraint, Frame, Layout, Line, Paragraph, Rect, Span, Style, Tab, UnicodeWidthStr, filter_input, theme,
};

pub(super) fn draw_note_filter(app: &mut App, frame: &mut Frame<'_>, area: &mut Rect) {
    if app.tab == Tab::Notes && app.note_list.filter.is_some() {
        draw_note_filter_label(app, frame, area);
    }
}

pub(super) fn draw_note_filter_label(app: &mut App, frame: &mut Frame<'_>, area: &mut Rect) {
    let Some(query) = app.note_list.filter.as_ref() else {
        return;
    };

    let [row, _, rest] =
        Layout::vertical([Constraint::Length(1), Constraint::Length(1), Constraint::Min(1)]).areas(*area);
    let label = Style::default().fg(if app.note_list.filter_editing {
        theme::pal().accent
    } else {
        theme::pal().subtext0
    });
    let count = format!("{} of {} ", app.note_list.rows.len(), app.note_list.notes.files.len());

    if app.note_list.filter_editing {
        frame.render_widget(
            Paragraph::new(Span::styled("filter › ", label)).style(theme::panel()),
            row,
        );
        app.note_list
            .filter_ta
            .render(frame, filter_input(row, count.width()), true);
    } else {
        let line = Line::from(vec![
            Span::styled("filter › ", label),
            Span::styled(query.clone(), theme::text()),
        ]);

        frame.render_widget(Paragraph::new(line).style(theme::panel()), row);
    }

    frame.render_widget(
        Paragraph::new(Line::styled(count, theme::panel().fg(theme::pal().subtext0)).right_aligned()),
        row,
    );
    *area = rest;
}

pub(super) fn draw_agent_filter(app: &mut App, frame: &mut Frame<'_>, area: &mut Rect) {
    if matches!(app.tab, Tab::Agents | Tab::New) {
        draw_agent_filter_label(app, frame, area);
    }
}

pub(super) fn draw_agent_filter_label(app: &mut App, frame: &mut Frame<'_>, area: &mut Rect) {
    let [row, _, rest] =
        Layout::vertical([Constraint::Length(1), Constraint::Length(1), Constraint::Min(1)]).areas(*area);
    let label = Style::default().fg(if app.agent_list.afilter_editing {
        theme::pal().accent
    } else {
        theme::pal().subtext0
    });
    let query = app.agent_list.afilter.clone().unwrap_or_default();
    let shown: usize = app.avisible().iter().map(|(_, group)| group.len()).sum();

    let count = if query.is_empty() {
        String::new()
    } else {
        format!("{shown} of {} ", app.agent_list.agents.len())
    };

    if app.agent_list.afilter_editing {
        frame.render_widget(
            Paragraph::new(Span::styled("filter › ", label)).style(theme::panel()),
            row,
        );
        app.agent_list
            .afilter_ta
            .render(frame, filter_input(row, count.width()), true);
    } else {
        let mut spans = vec![
            Span::styled("filter › ", label),
            Span::styled(query.clone(), theme::text()),
        ];

        if query.is_empty() {
            spans.push(Span::styled(
                "f: fuzzy search name, harness, model",
                theme::panel().fg(theme::pal().overlay0),
            ));
        }

        frame.render_widget(Paragraph::new(Line::from(spans)).style(theme::panel()), row);
    }

    if !count.is_empty() {
        frame.render_widget(
            Paragraph::new(Line::styled(count, theme::panel().fg(theme::pal().subtext0)).right_aligned()),
            row,
        );
    }

    *area = rest;
}
