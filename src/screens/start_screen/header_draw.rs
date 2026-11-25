use super::{App, BRAND, Frame, Line, Modifier, Paragraph, Rect, Screen, Span, Style, Tab, UnicodeWidthStr, theme};

impl App {
    pub(super) fn draw_header(&mut self, frame: &mut Frame, area: Rect) {
        let mut spans = vec![Span::styled(BRAND, theme::accent_bold()), Span::raw(" ")];
        let mut x = area.x + BRAND.width() as u16 + 1;

        if self.compose.screen == Screen::Pick && self.tab == Tab::Notes {
            spans.push(Span::styled(" new › notes ", theme::pill()));
        } else {
            draw_header_active(self, area, &mut spans, &mut x);
        }

        frame.render_widget(Paragraph::new(Line::from(spans)), area);

        // notifications: kind + agent count (ctrl+n lists them)
        let right = Line::from(crate::features::notifications::badge(
            &crate::domain::notifications::unseen(None),
        ))
        .right_aligned();
        let right_area = Rect {
            x: x.min(area.right()),
            width: area.right().saturating_sub(x),
            ..area
        };

        frame.render_widget(Paragraph::new(right), right_area);
    }
}

pub(super) fn draw_header_active(app_state: &mut App, area: Rect, spans: &mut Vec<Span<'_>>, x: &mut u16) {
    for (tab, label) in [
        (Tab::New, "new"),
        (Tab::Threads, "threads"),
        (Tab::Projects, "projects"),
        (Tab::Merge, "merge"),
        (Tab::Agents, "agents"),
        (Tab::Flows, "flows"),
        (Tab::Settings, "settings"),
    ] {
        let active = app_state.cur_tab() == tab;
        let focused = active && app_state.tabs_focus;

        let (bg, fg) = match (active, focused) {
            (true, true) => (Some(theme::pal().accent), theme::pal().on_accent),
            (true, false) => (theme::pill_dim().bg, theme::pal().accent),
            _ => (None, theme::pal().subtext0),
        };

        let base = bg.map_or(Style::default(), |style| Style::default().bg(style));
        let mut label = label.to_string();

        if tab == Tab::New && !app_state.note_list.selected.is_empty() {
            label = format!("{label} {}", app_state.note_list.selected.len());
        }

        let width = (label.width() + 2) as u16;

        app_state.preview.regions.tabs.push((
            Rect {
                x: *x,
                y: area.y,
                width,
                height: 1,
            },
            tab,
        ));
        *x += width + 1;
        spans.push(Span::styled(
            format!(" {label} "),
            base.fg(fg)
                .add_modifier(if active { Modifier::BOLD } else { Modifier::empty() }),
        ));
        spans.push(Span::raw(" "));
    }
}
