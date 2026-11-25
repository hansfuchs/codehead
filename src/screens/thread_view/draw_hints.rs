use super::Color;
use super::header_labels::PromptHint;
use super::{
    Duration, Frame, Instant, Line, Modifier, Paragraph, Rect, Span, Style, View, hint_keys, theme, tilde, to_width,
};

impl View {
    pub(super) fn draw_hints(&mut self, frame: &mut Frame, hint: Rect) -> Rect {
        // hint row: who gets the message, the send keys
        let mut to = self.recipient_spans();

        let shell = self.shell_mode();

        self.mode_spans(&mut to, shell);

        self.layout.hint_rect = hint;
        let lead: u16 = to.iter().map(|span| span.content.chars().count() as u16).sum();
        let mode = prompt_hint(self);
        let left_w = to_width(hint, mode);

        frame.render_widget(Paragraph::new(Line::from(to)), Rect { width: left_w, ..hint });

        // the dir input: right of `in`, half the row at most
        let mut cwd_rect = Rect::default();

        if let Some(input) = self.shell_state.cwd_edit.as_mut().filter(|_| shell) {
            cwd_rect = Rect {
                x: hint.x + lead,
                width: (hint.width / 2)
                    .saturating_sub(lead)
                    .max(10)
                    .min(hint.width.saturating_sub(lead)),
                ..hint
            };
            input.render(frame, cwd_rect, true);
        }

        let keys = hint_keys(mode);

        frame.render_widget(
            Paragraph::new(Line::from(theme::hints(keys))).right_aligned(),
            Rect {
                x: hint.x + hint.width / 2,
                width: hint.width - hint.width / 2,
                ..hint
            },
        );

        cwd_rect
    }
    pub(super) fn recipient_spans(&mut self) -> Vec<Span<'static>> {
        let mut to = vec![Span::styled("  → ", theme::muted())];

        for (i, name) in self.recipients().into_iter().enumerate() {
            push_recipient(&mut to, i, &name, self.color(&name));
        }

        if self.agents.members.is_empty() {
            to.clear();
        }

        to
    }
    pub(super) fn mode_spans(&mut self, to: &mut Vec<Span<'static>>, shell: bool) {
        if shell {
            shell_mode_spans(self, to);
        } else {
            chat_mode_spans(self, to);
        }
    }
}

fn shell_mode_spans(view: &mut View, to: &mut Vec<Span<'static>>) {
    let shell_style = Style::default().fg(theme::shell());

    *to = vec![
        Span::styled("  ! shell mode", shell_style.add_modifier(Modifier::BOLD)),
        Span::styled(" · in ", theme::muted()),
    ];

    if view.shell_state.cwd_edit.is_none() {
        to.push(Span::styled(tilde(&view.shell_state.shell_cwd), theme::muted()));
    }
}

fn chat_mode_spans(view: &mut View, to: &mut Vec<Span<'static>>) {
    if view.shell_state.shell_running > 0 {
        to.push(Span::styled(
            format!("  · ! {} running", view.shell_state.shell_running),
            Style::default().fg(theme::shell()),
        ));
    }

    // where the thread works: repo ⎇ branch (worktree), per checkout
    if view
        .checkout_state
        .spots_at
        .is_none_or(|at| at.elapsed() > Duration::from_secs(3))
    {
        view.checkout_state.spots = crate::screens::thread_view::workspot::of_thread(&view.id);
        view.checkout_state.spots_at = Some(Instant::now());
    }

    to.extend(crate::screens::thread_view::workspot::spans(&view.checkout_state.spots));
}

fn prompt_hint(view: &View) -> PromptHint {
    let shell = view.shell_mode();

    if view.shell_state.cwd_edit.is_some() && shell {
        PromptHint::Directory
    } else if view.prompt.hist.is_open() {
        PromptHint::History
    } else if shell {
        PromptHint::Shell
    } else if view.prompt.input.is_empty() {
        PromptHint::Empty
    } else {
        PromptHint::Text
    }
}

fn push_recipient(to: &mut Vec<Span<'static>>, index: usize, name: &str, color: Color) {
    if index > 0 {
        to.push(Span::styled(", ", theme::muted()));
    }

    to.push(Span::styled(name.to_string(), Style::default().fg(color)));
}
