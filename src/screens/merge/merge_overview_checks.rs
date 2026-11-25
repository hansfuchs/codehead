use super::{
    Line, Modifier, OverviewRows, Row, Span, Stop, Style, UnicodeWidthStr, Verdict, check_name, gh, highlight, secs,
    theme,
};

pub(super) fn append_checks(overview: &mut OverviewRows<'_>) {
    // checks
    let (fail, pass, skip, pending) = overview.pr.check_counts();
    let open = overview.tab.is_some_and(|tab| tab.checks_open);
    let mut parts = Vec::new();

    if fail > 0 {
        parts.push(Span::styled(
            format!("{fail} failed"),
            Style::default().fg(theme::pal().error()).add_modifier(Modifier::BOLD),
        ));
    }

    if pending > 0 {
        parts.push(Span::styled(
            format!("{pending} running"),
            theme::fg(theme::pal().progress()),
        ));
    }

    parts.push(Span::styled(
        format!("{pass} passed"),
        Style::default().fg(theme::pal().success()),
    ));
    let mut spans = vec![Span::styled(
        format!("{} checks  ", if open { "▾" } else { "▸" }),
        highlight(overview.focus, Stop::Checks, theme::bold()),
    )];

    for (i, part) in parts.into_iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled(" · ", theme::muted()));
        }

        spans.push(part);
    }

    spans.push(Span::styled(
        if open { "   ←: hide" } else { "   →: show" },
        theme::muted(),
    ));
    let width = Line::from(spans.clone()).width() as u16;

    overview.rows.push(Row {
        line: Line::from(spans),
        stops: vec![(0, width, Stop::Checks)],
        input: 0,
    });

    if skip > 0 {
        overview
            .rows
            .push(Row::new(Line::styled(format!("  {skip} skipped"), theme::muted())));
    }

    append_check_groups(overview, open);
}

fn append_check_groups(overview: &mut OverviewRows<'_>, open: bool) {
    if open {
        let shown: Vec<&gh::Check> = overview
            .pr
            .checks
            .iter()
            .filter(|check| check.verdict != Verdict::Skip)
            .collect();
        let mut last: Option<Verdict> = None;

        for (i, check) in shown.iter().enumerate() {
            append_check_row(overview, &shown, &mut last, i, check);
        }

        if !shown.is_empty() {
            overview.rows.push(Row::new(Line::styled(
                "    ctrl+⏎ / ctrl+click: open the check on GitHub",
                theme::muted(),
            )));
        }
    }

    overview.rows.push(Row::new(Line::raw("")));
}

fn append_check_row(
    overview: &mut OverviewRows<'_>,
    shown: &[&gh::Check],
    last: &mut Option<Verdict>,
    i: usize,
    check: &gh::Check,
) {
    if *last != Some(check.verdict) {
        let count = shown.iter().filter(|x| x.verdict == check.verdict).count();

        let label = match check.verdict {
            Verdict::Fail => format!("  {count} failed"),
            Verdict::Pending => format!("  {count} running"),
            _ => format!("  {count} successful"),
        };

        overview.rows.push(Row::new(Line::styled(label, theme::muted())));
        *last = Some(check.verdict);
    }

    let (mark, color) = match check.verdict {
        Verdict::Fail => ("✕", theme::pal().error()),
        Verdict::Pending => ("◷", theme::pal().progress()),
        _ => ("✓", theme::pal().success()),
    };

    let stop = Stop::Check(i);

    let detail = match check.verdict {
        Verdict::Fail if check.secs > 0 => format!("failed after {}", secs(check.secs)),
        Verdict::Fail => "failed".into(),
        Verdict::Pending => "running".into(),
        _ if check.secs > 0 => format!("successful in {}", secs(check.secs)),
        _ if !check.description.is_empty() => check.description.clone(),
        _ => "successful".into(),
    };

    let required = if check.required { "  required" } else { "" };
    let name = check_name(check);
    let room = overview.width.saturating_sub(detail.width() + required.len() + 8);
    let line = Line::from(vec![
        Span::styled(format!("    {mark} "), Style::default().fg(color)),
        Span::styled(
            crate::ui::thread_card::fit(&name, room),
            highlight(overview.focus, stop, theme::text()),
        ),
        Span::styled(format!("  {detail}"), theme::muted()),
        Span::styled(required, theme::text()),
    ]);
    let width = line.width() as u16;

    overview.rows.push(Row {
        line,
        stops: vec![(0, width, stop)],
        input: 0,
    });
}
