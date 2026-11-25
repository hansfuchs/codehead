use crate::infra::claude::Session;
use crate::ui::theme;
use ratatui::text::{Line, Span};
pub(crate) fn preview(session: &Session) -> Vec<Line<'static>> {
    let mut out = vec![
        Line::styled(session.title.clone(), theme::bold()),
        Line::from(vec![
            Span::styled("dir ", theme::muted()),
            Span::raw(session.cwd.clone()),
        ]),
        Line::from(vec![
            Span::styled("branch ", theme::muted()),
            Span::raw(if session.branch.is_empty() {
                "-".to_string()
            } else {
                session.branch.clone()
            }),
        ]),
        Line::from(vec![
            Span::styled("session ", theme::muted()),
            Span::raw(session.id.clone()),
            Span::styled(format!(" · {}", session.when), theme::muted()),
        ]),
        Line::raw(""),
        Line::styled(
            "enter: new thread in this dir, this response as context · c: add as context",
            theme::muted(),
        ),
        Line::raw(""),
        Line::styled("last response", theme::bold()),
    ];

    out.extend(session.last.lines().map(|line| Line::raw(line.to_string())));
    out
}
