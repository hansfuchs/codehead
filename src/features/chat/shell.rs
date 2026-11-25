use super::{Line, Modifier, Row, Span, Style, clock, markdown, theme};

/// Lines of shell output shown in the thread (the rest: a count).
pub(super) const SHELL_LINES: usize = 20;
/// The rows of the owner's shell COMMAND: `! command · 13:42 · exit 1`, its OUTPUT under it.
pub(super) fn shell_rows(command: &str, output: &str, code: i32, ts: u64, dir: Option<&str>, width: usize) -> Vec<Row> {
    let shell = Style::default().fg(theme::shell());
    let mut head = vec![
        Span::styled("! ", shell.add_modifier(Modifier::BOLD)),
        Span::styled(command.to_string(), theme::bold()),
        Span::styled(format!("  {}", clock(ts)), theme::muted()),
    ];

    if let Some(dir) = dir {
        let home = crate::infra::config::home().display().to_string();

        let shown = match dir.strip_prefix(&home) {
            Some("") => "~".to_string(),
            Some(rest) if rest.starts_with('/') => format!("~{rest}"),
            _ => dir.to_string(),
        };

        head.push(Span::styled(format!(" · in {shown}"), theme::muted()));
    }

    if code != 0 {
        let what = if code < 0 {
            "killed".to_string()
        } else {
            format!("exit {code}")
        };

        head.push(Span::styled(
            format!(" · {what}"),
            Style::default().fg(theme::pal().error()),
        ));
    }

    let mut rows = vec![Row::plain(Line::from(head))];
    let lines: Vec<&str> = output.trim_end().lines().collect();
    let bar = [Span::styled("  ⎿ ", shell)];
    let pad = [Span::raw("    ")];

    if lines.is_empty() {
        rows.extend(markdown::plain("(0 output)", width, theme::muted(), &bar));
    }

    for (i, line) in lines.iter().take(SHELL_LINES).enumerate() {
        rows.extend(markdown::plain(
            line,
            width,
            theme::muted(),
            if i == 0 { &bar } else { &pad },
        ));
    }

    if lines.len() > SHELL_LINES {
        rows.extend(markdown::plain(
            &format!("… {} more lines", lines.len() - SHELL_LINES),
            width,
            theme::muted(),
            &pad,
        ));
    }

    rows
}
