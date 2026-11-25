use super::{Duration, Line, Span, Style, View, chat, theme};
impl View {
    pub(super) fn pending_rows(&self) -> Vec<Line<'static>> {
        let mut extra: Vec<Line<'static>> = Vec::new();
        let right = self.stream.look == chat::Look::Bubbles;

        for (text, at, err) in &self.prompt.pending {
            pending_message(self, &mut extra, right, text, *at, err.as_deref());
        }

        if let Some((new, old, _)) = &self.prompt.queued {
            extra.push(Line::raw(""));
            extra.push(Line::styled(
                format!("  {new} replaces {old} after {old}'s turn · esc: interrupt {old}"),
                theme::fg(theme::pal().progress()),
            ));
        }

        extra
    }
}

fn pending_message(
    view: &View,
    extra: &mut Vec<Line<'static>>,
    right: bool,
    text: &str,
    at: std::time::Instant,
    err: Option<&str>,
) {
    extra.push(Line::raw(""));

    for line in text.lines().take(6) {
        // `@name`: in the agent's color, as in the stream
        let mut spans = crate::ui::markdown::mention_spans(line, theme::muted(), &view.agents.names);

        let line = if right {
            spans.push(Span::styled(" ▐", theme::muted()));
            Line::from(spans).right_aligned()
        } else {
            spans.insert(0, Span::styled("▌ ", theme::muted()));
            Line::from(spans)
        };

        extra.push(line);
    }

    let status = match err {
        Some(error) => Line::styled(
            format!("send failed: {error}  "),
            Style::default().fg(theme::pal().error()),
        ),
        None if at.elapsed() > Duration::from_secs(90) => {
            Line::styled("unconfirmed · check the host log before a retry  ", theme::muted())
        }
        None => Line::styled(format!("sending… {}s  ", at.elapsed().as_secs()), theme::muted()),
    };

    extra.push(if right { status.right_aligned() } else { status });
}
