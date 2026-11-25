use super::{COL, Frame, Line, Paragraph, Rect, SCREENS, Screen, Span, Wizard, theme};

impl Wizard {
    pub(super) fn draw(&self, frame: &mut Frame) {
        draw_wizard(self, frame);
    }
}

fn draw_wizard(wizard: &Wizard, frame: &mut Frame) {
    let full = frame.area();

    frame.render_widget(ratatui::widgets::Clear, full);
    let width = COL.min(full.width.saturating_sub(4));
    let x = full.x + (full.width.saturating_sub(width)) / 2;
    let height = full.height.saturating_sub(2);
    let col = Rect {
        x,
        y: full.y + 1,
        width,
        height,
    };
    let mut lines: Vec<Line> = Vec::new();

    lines.push(wizard.header(width));
    lines.push(Line::from(""));
    lines.push(Line::from(""));
    let (title, concept) = wizard.title();

    // tests read the screen: 0 decode
    let reveal = if cfg!(test) {
        1.0
    } else {
        (wizard.entered.elapsed().as_secs_f32() / 0.45).min(1.0)
    };

    lines.push(Line::from(crate::ui::avatar::decode(
        vec![Span::styled(format!("  {title}"), theme::accent_bold())],
        reveal,
        theme::accent(),
        theme::muted(),
    )));
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(format!("  {concept}"), theme::text())));
    lines.push(Line::from(""));

    draw_screen_content(wizard, frame, full, width, x, col, lines);
}

fn draw_screen_content(
    wizard: &Wizard,
    frame: &mut Frame<'_>,
    full: Rect,
    width: u16,
    x: u16,
    col: Rect,
    mut lines: Vec<Line<'static>>,
) {
    match SCREENS[wizard.screen] {
        Screen::Harness => wizard.draw_harness(&mut lines),
        Screen::Home => wizard.draw_home(&mut lines),
        Screen::Crew => wizard.draw_crew(&mut lines),
        Screen::Try => wizard.draw_try(&mut lines, width),
        Screen::Next => wizard.draw_next(&mut lines),
    }

    frame.render_widget(Paragraph::new(lines), col);

    // footer: the keys (esc 1st: never cut), the note on its own row above
    let foot = Rect {
        x,
        y: full.y + full.height.saturating_sub(2),
        width,
        height: 1,
    };
    let mut spans = Vec::new();

    for (i, (key, what)) in wizard.keys().into_iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled("  ", theme::muted()));
        }

        spans.push(Span::styled(key, theme::bold()));
        spans.push(Span::styled(format!(" {what}"), theme::muted()));
    }

    frame.render_widget(Paragraph::new(Line::from(spans)), foot);

    if !wizard.harness_state.note.is_empty() && foot.y > col.y {
        let row = Rect { y: foot.y - 1, ..foot };

        frame.render_widget(
            Paragraph::new(Span::styled(
                format!("· {}", wizard.harness_state.note),
                theme::fg(theme::pal().caution()),
            )),
            row,
        );
    }
}
