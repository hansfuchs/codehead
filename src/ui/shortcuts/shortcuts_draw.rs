use super::{Focus, Frame, Line, Modifier, Paragraph, Rect, Span, Style, Tab, UnicodeWidthStr, dropdown, theme};

impl Tab {
    pub(crate) fn draw(&mut self, frame: &mut Frame, area: Rect, active: bool) {
        self.rows.clear();

        if area.height < 4 || area.width < 30 {
            return;
        }

        let lw = (area.width / 3).clamp(18, 32);
        let list = Rect { width: lw, ..area };

        self.list_rect = list;
        frame.render_widget(
            Paragraph::new(Line::styled("#name → text", theme::muted())),
            Rect { height: 1, ..list },
        );
        let count = self.list.len() + 1;
        let shown = usize::from(list.height.saturating_sub(2));
        let off = dropdown::fit(0, self.at, count, shown);

        for index in off..count.min(off + shown) {
            draw_list_row(frame, list, index, off, active, self);
        }

        // right: the name, the text
        let right = Rect {
            x: list.right() + 2,
            width: area.width.saturating_sub(lw + 2),
            ..area
        };

        if self.at >= self.list.len() {
            self.name_rect = Rect::default();
            self.text_rect = Rect::default();
            frame.render_widget(
                Paragraph::new(Line::styled(
                    "enter: a new shortcut. in a prompt: # opens the shortcuts, tab / enter puts the text in",
                    theme::muted(),
                )),
                Rect { height: 1, ..right },
            );
            return;
        }

        let label = |text: &str, on: bool| draw_form_label(text, on);

        frame.render_widget(
            Paragraph::new(Line::from(label("name", self.focus == Focus::Name))),
            Rect { height: 1, ..right },
        );
        self.name_rect = Rect {
            y: right.y + 1,
            height: 1,
            ..right
        };
        draw_form(frame, active, right, label, self);
    }
}

fn draw_form_label(text: &str, on: bool) -> Span<'static> {
    Span::styled(text.to_string(), if on { theme::accent_bold() } else { theme::muted() })
}

fn draw_form(
    frame: &mut Frame<'_>,
    active: bool,
    right: Rect,
    label: impl Fn(&str, bool) -> Span<'static>,
    tab: &mut Tab,
) {
    tab.name
        .render(frame, tab.name_rect, active && tab.focus == Focus::Name);
    let err = tab.error.clone();
    let ty = right.y + 3;

    frame.render_widget(
        Paragraph::new(Line::from(label("text", tab.focus == Focus::Text))),
        Rect {
            y: ty,
            height: 1,
            ..right
        },
    );
    let th = right.bottom().saturating_sub(ty + 2);

    tab.text_rect = Rect {
        y: ty + 1,
        height: th,
        ..right
    };
    tab.text
        .render(frame, tab.text_rect, active && tab.focus == Focus::Text);

    if let Some(error) = err {
        frame.render_widget(
            Paragraph::new(Line::styled(error, Style::default().fg(theme::pal().error()))),
            Rect {
                y: right.bottom().saturating_sub(1),
                height: 1,
                ..right
            },
        );
    }
}

fn draw_list_row(frame: &mut Frame, list: Rect, index: usize, off: usize, active: bool, tab: &mut Tab) {
    let row_area = Rect {
        y: list.y + 2 + (index - off) as u16,
        height: 1,
        ..list
    };
    let on = index == tab.at;

    let (label, st) = match tab.list.get(index) {
        Some(shortcut) => (format!(" #{}", shortcut.name), theme::text()),
        None => (" + add shortcut".to_string(), theme::accent()),
    };

    let st = match (on, active && tab.focus == Focus::List) {
        (true, true) => theme::pill().add_modifier(Modifier::BOLD),
        (true, false) => theme::panel().add_modifier(Modifier::BOLD),
        _ => st,
    };

    let label = crate::ui::thread_card::fit(&label, usize::from(row_area.width));
    let pad = usize::from(row_area.width).saturating_sub(label.width());

    frame.render_widget(
        Paragraph::new(Line::styled(format!("{label}{}", " ".repeat(pad)), st)),
        row_area,
    );
    tab.rows.push((row_area, index));
}
