use super::{
    Line, Modifier, OverviewRows, Row, Span, Stop, Style, UnicodeWidthStr, Verdict, check_name, gh, highlight, secs,
    theme,
};

pub(super) fn append_checks(overview: &mut OverviewRows<'_>) {
    // checks
    let (fail, pass, skip, pending) = overview.pr.check_counts();
    let open = overview.tab.is_some_and(|thread_5| thread_5.checks_open);
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

    for (i, text_5) in parts.into_iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled(" · ", theme::muted()));
        }

        spans.push(text_5);
    }

    spans.push(Span::styled(
        if open { "   ←: hide" } else { "   →: show" },
        theme::muted(),
    ));
    let cw = Line::from(spans.clone()).width() as u16;

    overview.rows.push(Row {
        line: Line::from(spans),
        stops: vec![(0, cw, Stop::Checks)],
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
            .filter(|character_3| character_3.verdict != Verdict::Skip)
            .collect();
        let mut last: Option<Verdict> = None;

        for (i, character_4) in shown.iter().enumerate() {
            append_check_row(overview, &shown, &mut last, i, character_4);
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
    character_4: &gh::Check,
) {
    if *last != Some(character_4.verdict) {
        let count_value = shown.iter().filter(|x| x.verdict == character_4.verdict).count();

        let label = match character_4.verdict {
            Verdict::Fail => format!("  {count_value} failed"),
            Verdict::Pending => format!("  {count_value} running"),
            _ => format!("  {count_value} successful"),
        };

        overview.rows.push(Row::new(Line::styled(label, theme::muted())));
        *last = Some(character_4.verdict);
    }

    let (mark, mc) = match character_4.verdict {
        Verdict::Fail => ("✕", theme::pal().error()),
        Verdict::Pending => ("◷", theme::pal().progress()),
        _ => ("✓", theme::pal().success()),
    };

    let text_6 = Stop::Check(i);

    let detail = match character_4.verdict {
        Verdict::Fail if character_4.secs > 0 => format!("failed after {}", secs(character_4.secs)),
        Verdict::Fail => "failed".into(),
        Verdict::Pending => "running".into(),
        _ if character_4.secs > 0 => format!("successful in {}", secs(character_4.secs)),
        _ if !character_4.description.is_empty() => character_4.description.clone(),
        _ => "successful".into(),
    };

    let req = if character_4.required { "  required" } else { "" };
    let name = check_name(character_4);
    let room = overview.width.saturating_sub(detail.width() + req.len() + 8);
    let line = Line::from(vec![
        Span::styled(format!("    {mark} "), Style::default().fg(mc)),
        Span::styled(
            crate::ui::thread_card::fit(&name, room),
            highlight(overview.focus, text_6, theme::text()),
        ),
        Span::styled(format!("  {detail}"), theme::muted()),
        Span::styled(req, theme::text()),
    ]);
    let lw = line.width() as u16;

    overview.rows.push(Row {
        line,
        stops: vec![(0, lw, text_6)],
        input: 0,
    });
}
