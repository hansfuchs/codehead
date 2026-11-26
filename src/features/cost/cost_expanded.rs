use super::compact::{Group, groups};
use super::detail::spark_over;
use super::render::{next_cost, share_of};
use super::{
    AgentRow, AgentUse, AllThreads, Limit, Line, Show, Snap, Span, Style, agents, human, theme, until, window,
};

/// Width of the agents table's name column at least.
const NAME_MIN: usize = 6;

/// The expanded box's details, WIDTH columns: the thread, its harnesses (agents, plan windows),
/// the next message, every thread. The compact rows go below them (`draw_at`).
pub(super) fn expanded_lines(snapshot: &Snap, show: Show, now: u64, width: usize) -> Vec<Line<'static>> {
    let mut lines = thread_lines(snapshot, now, width);

    for group in groups(snapshot) {
        lines.push(Line::raw(""));
        lines.extend(group_lines(&group, snapshot, show, now, width));
    }

    if !show.next.to.is_empty() {
        lines.push(Line::raw(""));
        lines.push(next_line(snapshot, show));
    }

    lines.push(Line::raw(""));
    lines.extend(all_lines(
        snapshot,
        show.expanded.and_then(|expanded| expanded.all),
        width,
    ));

    lines
}

/// The thread's totals: title + $, cache + turns, the last hour and 24h of tokens.
fn thread_lines(snapshot: &Snap, now: u64, width: usize) -> Vec<Line<'static>> {
    let sum = |field: fn(&AgentUse) -> u64| snapshot.agents.iter().map(|agent| field(&agent.usage)).sum::<u64>();
    let (input, output, cache_read, cache_write, turns) = (
        sum(|usage| usage.input),
        sum(|usage| usage.output),
        sum(|usage| usage.cache_read),
        sum(|usage| usage.cache_write),
        sum(|usage| usage.turns),
    );
    let total = format!("${:.2} · {} tok", snapshot.cost, human(input + output));
    let title = crate::ui::thread_card::fit(&snapshot.title, width.saturating_sub(total.chars().count() + 2));
    let gap = width.saturating_sub(title.chars().count() + total.chars().count());
    let hit = (cache_read * 100).checked_div(input + cache_read).unwrap_or(0);

    let per_turn = if turns > 0 {
        format!(" · {turns} turns · ${:.3} per turn", snapshot.cost / turns as f64)
    } else {
        String::new()
    };

    let samples: Vec<(u64, u64)> = snapshot
        .agents
        .iter()
        .flat_map(|agent| agent.usage.samples.iter().copied())
        .collect();
    let spark_width = width.saturating_sub(10);

    vec![
        Line::from(vec![
            Span::styled(title, theme::bold()),
            Span::raw(" ".repeat(gap)),
            Span::styled(total, theme::text()),
        ]),
        Line::styled(
            format!("in {} · out {}{per_turn}", human(input), human(output)),
            theme::muted(),
        ),
        Line::styled(
            format!(
                "cache {} read · {} write · hit {hit}%",
                count(cache_read),
                count(cache_write)
            ),
            theme::muted(),
        ),
        spark_line("last hour", &spark_over(&samples, spark_width, 3_600, now)),
        spark_line("last 24h", &spark_over(&samples, spark_width, 86_400, now)),
    ]
}

fn spark_line(label: &str, spark: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{label:<10}"), theme::muted()),
        Span::styled(spark.to_string(), theme::fg(theme::pal().info())),
    ])
}

/// 1 harness: its $ and tokens, a table of its agents, its plan windows in words.
fn group_lines(group: &Group, snapshot: &Snap, show: Show, now: u64, width: usize) -> Vec<Line<'static>> {
    let usd: f64 = group.agents.iter().map(|agent| agent.usage.usd).sum();
    let tokens: u64 = group
        .agents
        .iter()
        .map(|agent| agent.usage.input + agent.usage.output)
        .sum();
    let total = format!("${usd:.2} · {} tok", human(tokens));
    let gap = width.saturating_sub(group.harness.chars().count() + total.chars().count());
    let name_width = group
        .agents
        .iter()
        .map(|agent| agent.name.chars().count())
        .max()
        .unwrap_or(0)
        .max(NAME_MIN)
        + 1;
    let mut lines = vec![
        Line::from(vec![
            Span::styled(group.harness.to_string(), theme::bold()),
            Span::raw(" ".repeat(gap)),
            Span::styled(total, theme::muted()),
        ]),
        Line::styled(
            format!(
                "{:name_width$}{:>7}{:>7}{:>9}{:>9}{:>6}{:>9}",
                "", "in", "out", "cache r", "cache w", "turns", "$"
            ),
            theme::muted(),
        ),
    ];

    for agent in &group.agents {
        lines.extend(agent_lines(agent, name_width));
    }

    let cost_next = next_cost(snapshot, show.next);

    for limit in &group.limits {
        lines.push(window_line(limit, cost_next, now));
    }

    lines
}

/// An agent's row of the table, and its sessions + context below.
fn agent_lines(agent: &AgentRow, name_width: usize) -> [Line<'static>; 2] {
    let usage = &agent.usage;
    let mut style = Style::default().fg(agents::color(agent.color.as_deref()));

    if agent.live {
        style = style.add_modifier(ratatui::style::Modifier::BOLD);
    }

    let ctx = usage
        .ctx
        .filter(|(_, size)| *size > 0)
        .map(|(used, size)| format!(" · context {} of {} ({}%)", human(used), human(size), used * 100 / size))
        .unwrap_or_default();
    let live = if agent.live { "live" } else { "ended" };

    let sessions = match usage.sessions {
        1 => "1 session".to_string(),
        count => format!("{count} sessions"),
    };

    [
        Line::from(vec![
            Span::styled(format!("{:name_width$}", agent.name), style),
            Span::styled(
                format!(
                    "{:>7}{:>7}{:>9}{:>9}{:>6}{:>9}",
                    human(usage.input),
                    human(usage.output),
                    human(usage.cache_read),
                    human(usage.cache_write),
                    usage.turns,
                    format!("${:.2}", usage.usd)
                ),
                theme::text(),
            ),
        ]),
        Line::styled(format!("{:name_width$}{live} · {sessions}{ctx}", ""), theme::muted()),
    ]
}

/// A plan window in words: used %, its reset, the next message's share, $ per 1 %.
fn window_line(limit: &Limit, cost_next: Option<(u64, f64)>, now: u64) -> Line<'static> {
    let fresh = limit.resets.is_some_and(|at| at <= now);
    let pct = if fresh { 0.0 } else { limit.pct };

    let reset = match limit.resets {
        _ if fresh => " · new window".to_string(),
        Some(at) => format!(" · resets in {}", until(at, now)),
        None => String::new(),
    };

    let share = share_of(limit, cost_next);

    let next = if share > 0.0 {
        format!(" · next +{share:.1}%")
    } else {
        String::new()
    };

    let rate = limit
        .usd_per_pct
        .map(|rate| format!(" · ${rate:.2} per 1%"))
        .unwrap_or_default();

    Line::from(vec![
        Span::styled(format!("{:<10}", window(&limit.label)), theme::muted()),
        Span::styled(format!("{pct:.0}%"), theme::text()),
        Span::styled(format!("{reset}{next}{rate}"), theme::muted()),
    ])
}

/// The next message: its recipients, the least tokens and $ it costs.
fn next_line(snapshot: &Snap, show: Show) -> Line<'static> {
    let mut spans = vec![
        Span::styled(format!("{:<10}", "next"), theme::muted()),
        Span::styled(
            show.next
                .to
                .iter()
                .map(|name| format!("@{name}"))
                .collect::<Vec<_>>()
                .join(" "),
            theme::text(),
        ),
    ];

    match next_cost(snapshot, show.next) {
        Some((tok, usd)) => {
            spans.push(Span::styled(format!("  ≥{} tok", human(tok)), theme::text()));
            spans.push(Span::styled(format!(" ~${usd:.2}"), theme::muted()));
        }
        None => spans.push(Span::styled("  0 context yet", theme::muted())),
    }

    Line::from(spans)
}

/// Every thread: the sums, the top 8 by $ (› this thread). ALL None: still on its way.
fn all_lines(snapshot: &Snap, all: Option<&AllThreads>, width: usize) -> Vec<Line<'static>> {
    let mut lines = vec![Line::styled("all threads", theme::bold())];

    // on its way: the rows of a full list held (the box keeps its height when it comes)
    let Some(all) = all else {
        lines.push(Line::styled("reading…", theme::muted()));
        lines.extend(std::iter::repeat_n(Line::raw(""), 1 + super::detail::TOP));
        return lines;
    };

    lines.push(Line::styled(
        format!(
            "${:.2} · {} tok · last 24h ${:.2}",
            all.usd,
            human(all.tokens),
            all.day_usd
        ),
        theme::text(),
    ));

    for thread in &all.threads {
        let here = thread.id == snapshot.id;
        let style = if here { theme::accent() } else { theme::text() };
        let tail = format!("  {} · {} turns", human(thread.tokens), thread.turns);
        let room = width.saturating_sub(12 + tail.chars().count());

        lines.push(Line::from(vec![
            Span::styled(
                format!("{}{:>8}", if here { "› " } else { "  " }, format!("${:.2}", thread.usd)),
                style,
            ),
            Span::styled(format!("  {}", crate::ui::thread_card::fit(&thread.title, room)), style),
            Span::styled(tail, theme::muted()),
        ]));
    }

    lines
}

/// VALUE as `human`, 0 as `0` (in a sentence a `·` reads as a separator).
fn count(value: u64) -> String {
    if value == 0 { "0".into() } else { human(value) }
}
