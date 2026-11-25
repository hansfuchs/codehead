use super::{Frame, Pos, Position, Rect, ShellComp, Span, TextArea, theme};

impl ShellComp {
    /// The suggestion right of the text's end in TA (rendered this frame), muted.
    pub(crate) fn render_suggestion(
        &mut self,
        frame: &mut Frame,
        ta: &TextArea,
        thread_commands: &[String],
        area: Rect,
    ) {
        render_suggestion_shell_comp(self, frame, ta, thread_commands, area);
    }
}

fn render_suggestion_shell_comp(
    shell: &mut ShellComp,
    frame: &mut Frame,
    ta: &TextArea,
    thread_commands: &[String],
    area: Rect,
) {
    let Some(rest) = shell.suggestion(ta, thread_commands) else {
        return;
    };

    let end = Pos {
        row: 0,
        col: ta.text().chars().count(),
    };
    let Some(screen) = ta.screen_pos(end) else { return };

    if screen.x >= area.right() || !area.contains(Position { x: area.x, y: screen.y }) {
        return;
    }

    let room = (area.right() - screen.x) as usize;
    let shown: String = rest.chars().take(room).collect();

    frame.render_widget(
        ratatui::widgets::Paragraph::new(Span::styled(shown, theme::muted())),
        Rect {
            x: screen.x,
            y: screen.y,
            width: room as u16,
            height: 1,
        },
    );
}
