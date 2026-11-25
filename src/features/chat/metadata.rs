use super::{
    Color, Colors, Fold, Item, Line, Modifier, Row, Span, Style, Thoughts, Thread, UnicodeWidthStr, clock, countdown,
    theme, usage_limit,
};

/// The row of AGENT's edits note TEXT (C: the agent's color): 1 row of W columns, cut with ….
pub(super) fn edits_row(agent: &str, text: &str, color: Color, width_value: usize) -> Row {
    let muted = theme::pal().subtext0;
    let room = width_value.saturating_sub(2 + agent.width() + 1);
    let one = text.split_whitespace().collect::<Vec<_>>().join(" ");

    Row::plain(Line::from(vec![
        Span::styled("● ", Style::default().fg(muted)),
        Span::styled(format!("{agent} "), Style::default().fg(color)),
        Span::styled(crate::ui::thread_card::fit(&one, room), Style::default().fg(muted)),
    ]))
}
pub(super) fn color_of(colors: &Colors, name: &str) -> Color {
    colors.get(name).copied().unwrap_or(theme::pal().accent)
}
pub(super) fn badge(name: &str, color: Color, ts: u64) -> Vec<Span<'static>> {
    vec![
        Span::styled(
            format!(" {name} "),
            Style::default()
                .bg(color)
                .fg(theme::pal().on_accent)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(format!(" {}", clock(ts)), theme::muted()),
    ]
}
/// The auto resume time of note ITEM: the last usage-limit note of an agent the host resumes.
pub(super) fn resume_of(thread: &Thread, item: &Item) -> Option<u64> {
    let Item::Note { agent, .. } = item else { return None };
    let at = *thread.resumes.get(agent)?;
    let last = thread.items.iter().rev().find(
        |it| matches!(it, Item::Note { agent: agent_value, text, .. } if agent_value == agent && usage_limit(text)),
    )?;

    std::ptr::eq(last, item).then_some(at)
}
/// The network in an error text.
pub(crate) fn offline(msg: &str) -> bool {
    [
        "ENOTFOUND",
        "ECONNREFUSED",
        "ECONNRESET",
        "ETIMEDOUT",
        "EAI_AGAIN",
        "Can't reach",
        "Connection error",
        "fetch failed",
    ]
    .iter()
    .any(|thread| msg.contains(thread))
}
/// Agents (name, harness) other than SKIP on HARNESS (Some) / not on it (None: other harnesses).
pub(super) fn agents_on<'a>(
    roster: &'a [(String, String)],
    harness: &str,
    same: bool,
    skip: &str,
) -> Vec<&'a (String, String)> {
    roster
        .iter()
        .filter(|(count, height)| count != skip && (height == harness) == same)
        .collect()
}
/// The hint rows of an error: what to do next, which agents can go on.
pub(super) fn error_hint(agent: &str, text: &str, roster: &[(String, String)], resume: Option<u64>) -> Vec<String> {
    let harness = roster
        .iter()
        .find(|(count, _)| count == agent)
        .map(|result| result.1.clone())
        .unwrap_or_default();

    if usage_limit(text) {
        let alt = agents_on(roster, &harness, false, agent);
        let mut value = vec![format!(
            "{agent} stopped: {} usage limit reached.",
            crate::engine::harnesses::get(&harness).map_or(harness.as_str(), |height| height.label)
        )];

        if let Some(at) = resume {
            error_hint_now(&mut value, at);
        }

        if alt.is_empty() {
            value.push("Agents on other harnesses: none".into());
        } else {
            value.push(format!(
                "Agents on other harnesses: {}",
                alt.iter()
                    .map(|(count_value, height_value)| format!("@{count_value} ({height_value})"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
            value.push("Use @agent → ⇄ replace to continue.".into());
        }

        return value;
    }

    if text.contains("plan mode") || text.contains("ExitPlanMode") {
        return vec![format!(
            "Fix: agents tab → {agent} → mode → autopilot, then send the message again."
        )];
    }

    vec![if offline(text) {
        "Once the connection is back, send a message to continue.".into()
    } else {
        "Send a message to continue.".into()
    }]
}

fn error_hint_now(value: &mut Vec<String>, at: u64) {
    let now = crate::engine::now_ms();
    let left = if at > now { countdown(at - now) } else { "now".into() };

    value.push(format!(
        "Resumes automatically in {left} ({}), codehead open or not · a message from you: resume off.",
        clock(at)
    ));
}
/// ` · 2 thoughts` for a run's stats (0 while thoughts are hidden).
pub(super) fn thought_count(thoughts: &[(usize, String)], fold: &Fold) -> String {
    match thoughts.len() {
        _ if fold.thoughts == Thoughts::Hidden => String::new(),
        0 => String::new(),
        1 => " · 1 thought".into(),
        count => format!(" · {count} thoughts"),
    }
}
