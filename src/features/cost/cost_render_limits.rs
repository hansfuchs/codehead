use super::{Limit, Line, Snap, Span, bar, theme, until, window};

pub(super) fn append_plan_limits(
    snapshot: &Snap,
    now: u64,
    width: usize,
    lines: &mut Vec<Line<'_>>,
    compact: bool,
    name_w: usize,
    cost_next: Option<(u64, f64)>,
) {
    if snapshot.limits.is_empty() {
        return;
    }

    if !compact {
        lines.push(Line::raw(""));
        lines.push(Line::styled("plan limits", theme::muted()));
    }

    // the harness in the label: 2+ harnesses only
    let multiple_harnesses = snapshot
        .limits
        .iter()
        .any(|line_value| line_value.harness != snapshot.limits[0].harness);

    let label_width = snapshot
        .limits
        .iter()
        .map(|line_3| limit_label(line_3, multiple_harnesses).chars().count())
        .max()
        .unwrap_or(0)
        .max(name_w.saturating_sub(1));

    // the windows side by side, 2 columns between them (too narrow: 1 per row)
    let count_value = snapshot.limits.len();
    let segment_width = width.saturating_sub(2 * (count_value - 1)) / count_value;
    let side_by_side = snapshot
        .limits
        .iter()
        .all(|line_4| segment_width >= limit_label(line_4, multiple_harnesses).chars().count() + 1 + 4 + 9);
    let mut row: Vec<Span> = Vec::new();

    for (i, line_5) in snapshot.limits.iter().enumerate() {
        append_plan_limit(
            now,
            PlanLimitLayout {
                width,
                multiple_harnesses,
                label_width,
                segment_width,
                side_by_side,
            },
            lines,
            cost_next,
            &mut row,
            i,
            line_5,
        );
    }

    if side_by_side {
        lines.push(Line::from(row));
    }
}

pub(super) fn append_plan_limit(
    now: u64,
    plan_limit_layout: PlanLimitLayout,
    lines: &mut Vec<Line<'_>>,
    cost_next: Option<(u64, f64)>,
    row: &mut Vec<Span<'_>>,
    i: usize,
    line_5: &crate::engine::limits::Window,
) {
    let PlanLimitLayout {
        width,
        multiple_harnesses,
        label_width,
        segment_width,
        side_by_side,
    } = plan_limit_layout;

    // window reset since the last report: 0 %
    let fresh = line_5.resets.is_some_and(|result| result <= now);
    let pct = if fresh { 0.0 } else { line_5.pct };

    let share = match (cost_next, line_5.usd_per_pct) {
        (Some((_, usd)), Some(rate)) if rate > 0.0 => usd / rate,
        _ => 0.0,
    };

    let tail = match line_5.resets {
        _ if fresh => " new".to_string(),
        Some(result_value) => format!(" {:>3.0}% {:>3}", pct, until(result_value, now)),
        None => format!(" {pct:>3.0}%"),
    };

    let tail = format!("{tail:<9}");

    if side_by_side {
        let lab = limit_label(line_5, multiple_harnesses);
        let bw = segment_width - lab.chars().count() - 1 - 9;

        row.extend((i > 0).then_some(Span::raw("  ")));

        row.push(Span::styled(format!("{lab} "), theme::muted()));
        row.extend(bar(pct, share, bw));
        row.push(Span::styled(tail, theme::muted()));
    } else {
        let bw = width.saturating_sub(label_width + 1 + 9).max(4);
        let mut spans = vec![Span::styled(
            format!("{:label_width$} ", limit_label(line_5, multiple_harnesses)),
            theme::muted(),
        )];

        spans.extend(bar(pct, share, bw));
        spans.push(Span::styled(tail, theme::muted()));
        lines.push(Line::from(spans));
    }
}

pub(super) fn limit_label(limit: &Limit, multiple_harnesses: bool) -> String {
    if multiple_harnesses {
        format!("{} {}", limit.harness, window(&limit.label))
    } else {
        window(&limit.label)
    }
}

#[derive(Clone, Copy)]
pub(super) struct PlanLimitLayout {
    width: usize,
    multiple_harnesses: bool,
    label_width: usize,
    segment_width: usize,
    side_by_side: bool,
}
