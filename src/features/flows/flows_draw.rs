use super::{FlowsTab, Focus, Frame, Line, Modifier, Paragraph, Rect, Span, Style, Sub, UnicodeWidthStr, theme};

impl FlowsTab {
    pub(crate) fn draw(&mut self, frame: &mut Frame, area: Rect, active: bool) {
        draw_flows_tab(self, frame, area, active);
    }

    fn draw_body(&mut self, frame: &mut Frame<'_>, area: Rect, active: bool) {
        let body = Rect {
            y: area.y + 2,
            height: area.height - 2,
            ..area
        };
        let list_w = (body.width / 3).clamp(24, 40);
        let list = Rect { width: list_w, ..body };

        self.hit_regions.list_rect = list;
        self.draw_list(frame, list, active);
        let right = Rect {
            x: list.right() + 2,
            width: body.width.saturating_sub(list_w + 2),
            ..body
        };

        match self.sub {
            Sub::Flows => self.draw_flow(frame, right, active),
            Sub::Steps => self.draw_step(frame, right, active),
        }

        if let Some(error) = self.error.clone() {
            frame.render_widget(
                Paragraph::new(Line::styled(
                    crate::ui::thread_card::fit(&error, usize::from(area.width)),
                    Style::default().fg(theme::pal().error()),
                )),
                Rect {
                    y: area.bottom() - 1,
                    height: 1,
                    ..area
                },
            );
        }

        self.draw_ask(frame, right);
        self.draw_picker(frame, right, area);
    }
}

fn draw_sub_tab(
    area: Rect,
    active: bool,
    x: &mut u16,
    spans: &mut Vec<Span<'_>>,
    sub: Sub,
    label: &str,
    tab: &mut FlowsTab,
) {
    let on = sub == tab.sub;

    let style = if on {
        if active && tab.focus == Focus::List {
            theme::pill().add_modifier(Modifier::BOLD)
        } else {
            theme::panel().fg(theme::pal().accent).add_modifier(Modifier::BOLD)
        }
    } else {
        theme::muted()
    };

    let tab_width = label.width() as u16 + 2;

    tab.hit_regions.sub_rects.push((
        Rect {
            x: *x,
            y: area.y,
            width: tab_width,
            height: 1,
        },
        sub,
    ));
    *x += tab_width + 1;
    spans.push(Span::styled(format!(" {label} "), style));
    spans.push(Span::raw(" "));
}

fn draw_flows_tab(state: &mut FlowsTab, frame: &mut Frame, area: Rect, active: bool) {
    state.hit_regions.rows.clear();
    state.hit_regions.rail_rows.clear();
    state.hit_regions.sub_rects.clear();
    state.hit_regions.field_rects.clear();

    if area.height < 6 || area.width < 40 {
        return;
    }

    // sub-tabs + filter
    let mut x = area.x;
    let mut spans = Vec::new();

    for (sub, label) in [(Sub::Flows, "flows"), (Sub::Steps, "steps")] {
        draw_sub_tab(area, active, &mut x, &mut spans, sub, label, state);
    }

    if state.filtering || !state.filter.is_empty() {
        spans.push(Span::styled(
            format!("  /{}{}", state.filter, if state.filtering { "▏" } else { "" }),
            theme::accent(),
        ));
    }

    frame.render_widget(Paragraph::new(Line::from(spans)), Rect { height: 1, ..area });
    state.draw_body(frame, area, active);
}
