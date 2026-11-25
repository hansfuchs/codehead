use super::{
    ARM, Line, MergeBtn, Modifier, OverviewRows, Ready, Row, Span, State, Stop, Style, UnicodeWidthStr, Verdict,
    busy_label, chip, secs, theme,
};
use ratatui::style::Color;

pub(super) fn append_actions(overview: &mut OverviewRows<'_>) {
    // actions: the readiness banner, update, merge
    let ready = overview.pr.ready();
    let (title, detail) = ready.text();

    let (icon, color) = readiness_style(ready);

    let edge = Style::default().fg(color);

    overview.rows.push(Row::new(Line::styled(
        format!("╭{}╮", "─".repeat(overview.width.saturating_sub(2))),
        edge,
    )));

    let busy = overview
        .tab
        .and_then(|thread_4| thread_4.input.busy.as_ref())
        .and_then(busy_label);
    let btn = MergeBtn::of(overview.pr);
    let buttons = action_buttons(overview, &btn, busy);

    append_action_heading(overview, edge, icon, title, color, buttons);
    let detail = action_detail(overview, ready, &btn, busy, detail);

    append_action_footer(overview, edge, detail);
}

fn append_action_footer(overview: &mut OverviewRows<'_>, edge: Style, mut detail: String) {
    let reviewers: Vec<String> = overview
        .pr
        .reviewers
        .iter()
        .map(|(line_2, text_4)| {
            format!(
                "{line_2} {}",
                if text_4 == "APPROVED" {
                    "✓"
                } else if text_4 == "CHANGES_REQUESTED" {
                    "✕"
                } else {
                    "·"
                }
            )
        })
        .chain(overview.pr.requested.iter().map(|result| format!("{result} ◷")))
        .collect();

    if !reviewers.is_empty() {
        detail = format!("{detail} · reviews: {}", reviewers.join(", "));
    }

    let directory = crate::ui::thread_card::fit(&format!("  {detail}"), overview.width.saturating_sub(4));
    let pad = overview.width.saturating_sub(directory.width() + 4);

    overview.rows.push(Row::new(Line::from(vec![
        Span::styled("│ ", edge),
        Span::styled(directory, theme::muted()),
        Span::raw(" ".repeat(pad)),
        Span::styled(" │", edge),
    ])));
    overview.rows.push(Row::new(Line::styled(
        format!("╰{}╯", "─".repeat(overview.width.saturating_sub(2))),
        edge,
    )));
}

fn readiness_style(ready: Ready) -> (&'static str, Color) {
    match ready {
        Ready::Mergeable => ("✓", theme::pal().success()),
        Ready::Merged => ("◆", theme::pal().merged()),
        Ready::Conflicts | Ready::ChecksFailed | Ready::ChangesRequested | Ready::Closed => ("✕", theme::pal().error()),
        Ready::ChecksRunning | Ready::Unknown | Ready::Queued => ("◷", theme::pal().progress()),
        _ => ("!", theme::pal().attention()),
    }
}

fn is_armed(overview: &OverviewRows<'_>, stop: Stop) -> bool {
    overview
        .tab
        .and_then(|thread_3| thread_3.input.armed)
        .is_some_and(|(agent, at)| agent == stop && at.elapsed() < ARM)
}

fn action_buttons(overview: &OverviewRows<'_>, btn: &MergeBtn, busy: Option<&str>) -> Vec<(String, Stop, Style)> {
    let mut buttons: Vec<(String, Stop, Style)> = Vec::new();

    if overview.pr.needs_update() {
        let label = if is_armed(overview, Stop::Update) {
            " ⏎ again: update "
        } else if overview.pr.conflicts() {
            " resolve conflicts "
        } else {
            " update branch "
        };

        buttons.push((
            label.into(),
            Stop::Update,
            chip(
                theme::pal().info(),
                overview.focus == Some(Stop::Update) || is_armed(overview, Stop::Update),
            ),
        ));
    }

    if matches!(overview.pr.state, State::Open | State::Draft) {
        let (label, character_value) = btn.label(overview.pr);

        let (label, st) = match busy {
            Some(bytes) => (bytes.to_string(), chip(theme::pal().progress(), true)),
            None if is_armed(overview, Stop::Merge) => (format!(" ⏎ again:{label}"), chip(character_value, true)),
            None => (label, chip(character_value, overview.focus == Some(Stop::Merge))),
        };

        buttons.push((label, Stop::Merge, st));
    }

    buttons
}

fn append_action_heading(
    overview: &mut OverviewRows<'_>,
    edge: Style,
    icon: &str,
    title: &str,
    color: Color,
    buttons: Vec<(String, Stop, Style)>,
) {
    let bw: usize = buttons.iter().map(|bytes_value| bytes_value.0.width() + 1).sum();
    let head = format!("{icon} {title}");
    let left_w = overview.width.saturating_sub(bw + 4);
    let head = crate::ui::thread_card::fit(&head, left_w);

    // auto-merge on: next to the state, in the merge color
    let auto = overview
        .pr
        .auto_merge
        .as_ref()
        .filter(|_| overview.pr.queued.is_none())
        .map(|message_value| format!(" · auto-merge on ({})", message_value.to_lowercase()))
        .unwrap_or_default();
    let auto = crate::ui::thread_card::fit(&auto, left_w.saturating_sub(head.width()));
    let mut spans = vec![
        Span::styled("│ ", edge),
        Span::styled(head.clone(), Style::default().fg(color).add_modifier(Modifier::BOLD)),
    ];

    if !auto.is_empty() {
        spans.push(Span::styled(
            auto.clone(),
            Style::default().fg(theme::pal().success()).add_modifier(Modifier::BOLD),
        ));
    }

    let used = 2 + head.width() + auto.width();

    spans.push(Span::raw(" ".repeat(overview.width.saturating_sub(used + bw + 1))));
    let mut stops = Vec::new();
    let mut x = overview.width.saturating_sub(bw + 1) as u16;

    for (label, text_3, st) in buttons {
        let lw = label.width() as u16;

        stops.push((x, lw, text_3));
        spans.push(Span::styled(label, st));
        spans.push(Span::raw(" "));
        x += lw + 1;
    }

    spans.push(Span::styled("│", edge));
    overview.rows.push(Row {
        line: Line::from(spans),
        stops,
        input: 0,
    });
}

fn action_detail(
    overview: &OverviewRows<'_>,
    ready: Ready,
    btn: &MergeBtn,
    busy: Option<&str>,
    detail: &str,
) -> String {
    let mut detail = detail.to_string();

    if let Some(query) = &overview.pr.queued {
        let mut parts = Vec::new();

        if query.position > 0 {
            parts.push(format!("position {}", query.position));
        }

        if !query.state.is_empty() {
            parts.push(query.state.to_lowercase().replace('_', " "));
        }

        if query.eta > 0 {
            parts.push(format!("merges in ~{}", secs(query.eta)));
        }

        if !parts.is_empty() {
            detail = parts.join(" · ");
        }
    }

    if is_armed(overview, Stop::Merge) {
        detail = format!("enter again: {}", btn.verb(overview.pr));
    } else if busy.is_some() {
        detail = format!("gh: {}…", btn.verb(overview.pr));
    } else if let MergeBtn::Off(why) = btn
        && overview.pr.state == State::Open
    {
        detail = format!("{detail} · {why}");
    }

    // ready, but checks failed that are not required: say so (0 "all requirements are met" alone)
    let failed = overview
        .pr
        .checks
        .iter()
        .filter(|character_2| character_2.verdict == Verdict::Fail)
        .count();

    if ready == Ready::Mergeable && failed > 0 {
        detail = format!(
            "{detail} · {failed} check{} failed (not required)",
            if failed == 1 { "" } else { "s" }
        );
    }

    detail
}
