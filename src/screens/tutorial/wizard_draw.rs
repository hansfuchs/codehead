use super::{COL, Frame, Line, Paragraph, Rect, SCREENS, Screen, Span, Wizard, theme};

impl Wizard {
    pub(super) fn draw(&self, frame: &mut Frame) {
        draw_wizard(self, frame);
    }
}

fn draw_wizard(widget_state: &Wizard, frame: &mut Frame) {
    let full = frame.area();

    frame.render_widget(ratatui::widgets::Clear, full);
    let width_value = COL.min(full.width.saturating_sub(4));
    let x = full.x + (full.width.saturating_sub(width_value)) / 2;
    let height_value = full.height.saturating_sub(2);
    let col = Rect {
        x,
        y: full.y + 1,
        width: width_value,
        height: height_value,
    };
    let mut lines: Vec<Line> = Vec::new();

    lines.push(widget_state.header(width_value));
    lines.push(Line::from(""));
    lines.push(Line::from(""));
    let (title, concept) = widget_state.title();

    // tests read the screen: 0 decode
    let path = if cfg!(test) {
        1.0
    } else {
        (widget_state.entered.elapsed().as_secs_f32() / 0.45).min(1.0)
    };

    lines.push(Line::from(crate::ui::avatar::decode(
        vec![Span::styled(format!("  {title}"), theme::accent_bold())],
        path,
        theme::accent(),
        theme::muted(),
    )));
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(format!("  {concept}"), theme::text())));
    lines.push(Line::from(""));

    draw_screen_content(widget_state, frame, full, width_value, x, col, lines);
}

fn draw_screen_content(
    widget_state: &Wizard,
    frame: &mut Frame<'_>,
    full: Rect,
    width_value: u16,
    x: u16,
    col: Rect,
    mut lines: Vec<Line<'static>>,
) {
    match SCREENS[widget_state.screen] {
        Screen::Harness => widget_state.draw_harness(&mut lines),
        Screen::Home => widget_state.draw_home(&mut lines),
        Screen::Crew => widget_state.draw_crew(&mut lines),
        Screen::Try => widget_state.draw_try(&mut lines, width_value),
        Screen::Next => widget_state.draw_next(&mut lines),
    }

    frame.render_widget(Paragraph::new(lines), col);

    // footer: the keys (esc 1st: never cut), the note on its own row above
    let foot = Rect {
        x,
        y: full.y + full.height.saturating_sub(2),
        width: width_value,
        height: 1,
    };
    let mut spans = Vec::new();

    for (i, (key, what)) in widget_state.keys().into_iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled("  ", theme::muted()));
        }

        spans.push(Span::styled(key, theme::bold()));
        spans.push(Span::styled(format!(" {what}"), theme::muted()));
    }

    frame.render_widget(Paragraph::new(Line::from(spans)), foot);

    if !widget_state.harness_state.note.is_empty() && foot.y > col.y {
        let row = Rect { y: foot.y - 1, ..foot };

        frame.render_widget(
            Paragraph::new(Span::styled(
                format!("· {}", widget_state.harness_state.note),
                theme::fg(theme::pal().caution()),
            )),
            row,
        );
    }
}
