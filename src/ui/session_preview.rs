use crate::infra::claude::Session;
use crate::ui::theme;
use ratatui::text::{Line, Span};
pub(crate) fn preview(text: &Session) -> Vec<Line<'static>> {
    let mut out = vec![
        Line::styled(text.title.clone(), theme::bold()),
        Line::from(vec![Span::styled("dir ", theme::muted()), Span::raw(text.cwd.clone())]),
        Line::from(vec![
            Span::styled("branch ", theme::muted()),
            Span::raw(if text.branch.is_empty() {
                "-".to_string()
            } else {
                text.branch.clone()
            }),
        ]),
        Line::from(vec![
            Span::styled("session ", theme::muted()),
            Span::raw(text.id.clone()),
            Span::styled(format!(" · {}", text.when), theme::muted()),
        ]),
        Line::raw(""),
        Line::styled(
            "enter: new thread in this dir, this response as context · c: add as context",
            theme::muted(),
        ),
        Line::raw(""),
        Line::styled("last response", theme::bold()),
    ];

    out.extend(text.last.lines().map(|line| Line::raw(line.to_string())));
    out
}
