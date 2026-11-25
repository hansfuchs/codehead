use super::{
    AgentUse, Detail, HashMap, Line, SPARK, Span, Style, agents, bar, human, limits, now, theme, until, uses, window,
};

/// Read the detail of thread ID (None: 0 thread part).
pub(crate) fn detail(id: Option<&str>) -> Detail {
    let mut detail = Detail::default();
    let since = now().saturating_sub(86_400);

    if let Some(id) = id.filter(|id| !id.is_empty()) {
        detail.agents = uses(id);

        for agent in &mut detail.agents {
            agent.samples.retain(|(at, _)| *at > since);
        }
    }

    let mut rows: Vec<(String, f64, u64, u64)> = Vec::new();

    for thread in crate::engine::store::all() {
        let totals = thread_totals(&thread.id, since, &mut detail.day_usd);

        if totals.turns > 0 {
            rows.push((thread.id, totals.usd, totals.tokens, totals.turns));
        }
    }

    rows.sort_by(|left, right| right.1.total_cmp(&left.1));
    detail.all_usd = rows.iter().map(|row| row.1).sum();
    detail.all_tokens = rows.iter().map(|row| row.2).sum();
    detail.threads = rows.into_iter().take(8).collect();
    detail.limits = limits();

    detail
}

/// BUCKETS buckets of SPAN seconds up to NOW; empty: `·`.
pub(super) fn spark_over(samples: &[(u64, u64)], buckets: usize, span: u64, now: u64) -> String {
    if buckets == 0 || span == 0 {
        return String::new();
    }

    let mut totals = vec![0u64; buckets];
    let start = now.saturating_sub(span);

    for &(at, tok) in samples {
        if at > start {
            let i = ((u128::from(at - start) * buckets as u128 / u128::from(span)) as usize).min(buckets - 1);

            totals[i] += tok;
        }
    }

    let max = totals.iter().copied().max().unwrap_or(0).max(1);

    totals
        .iter()
        .map(|&total| {
            if total == 0 {
                '·'
            } else {
                SPARK[((total * 7) / max) as usize]
            }
        })
        .collect()
}

/// Lines of the tokens tab, W columns wide. NAMES: agent → (name, color); TITLES: thread id → title.
pub(crate) fn detail_lines(
    detail: &Detail,
    here: Option<&str>,
    names: &HashMap<String, (String, Option<String>)>,
    titles: &HashMap<String, String>,
    width: usize,
) -> Vec<Line<'static>> {
    let now = now();
    let mut lines: Vec<Line<'static>> = Vec::new();

    append_current_thread(detail, here, names, titles, width, now, &mut lines);

    lines.push(heading("all threads"));
    lines.push(field_row(
        "total",
        format!(
            "${:.2} · {} tokens · last 24h ${:.2}",
            detail.all_usd,
            human(detail.all_tokens),
            detail.day_usd
        ),
    ));

    append_thread_list(detail, here, titles, width, &mut lines);

    append_detail_limits(detail, now, &mut lines);

    lines
}

fn append_detail_limits(detail: &Detail, now: u64, lines: &mut Vec<Line<'static>>) {
    if detail.limits.is_empty() {
        return;
    }

    lines.push(Line::raw(""));
    lines.push(heading("plan limits"));

    for (harness, win, pct, resets) in &detail.limits {
        let color = if *pct >= 90.0 {
            theme::pal().error()
        } else if *pct >= 70.0 {
            theme::pal().caution()
        } else {
            theme::pal().text
        };

        let reset = match resets {
            Some(at) if *at <= now => " · new window".to_string(),
            Some(at) => format!(" · resets in {}", until(*at, now)),
            None => String::new(),
        };

        let mut spans = vec![Span::styled(
            format!("{:<10}", format!("{harness} {}", window(win))),
            theme::muted(),
        )];

        spans.extend(bar(*pct, 0.0, 20));
        spans.push(Span::styled(format!(" {pct:.0}%"), Style::default().fg(color)));
        spans.push(Span::styled(reset, theme::muted()));
        lines.push(Line::from(spans));
    }
}

fn append_thread_list(
    detail: &Detail,
    here: Option<&str>,
    titles: &HashMap<String, String>,
    width: usize,
    lines: &mut Vec<Line<'static>>,
) {
    for (id, usd, tokens, turns) in &detail.threads {
        let title = titles.get(id).cloned().unwrap_or_else(|| id.chars().take(8).collect());
        let mark = if here == Some(id.as_str()) { "› " } else { "  " };

        let style = if here == Some(id.as_str()) {
            theme::accent()
        } else {
            theme::text()
        };

        let tail = format!("  {} · {turns} turns", human(*tokens));
        let room = width.saturating_sub(10 + tail.chars().count() + 2);

        lines.push(Line::from(vec![
            Span::styled(format!("{mark}{:>8}", format!("${usd:.2}")), style),
            Span::styled(format!("  {}", crate::ui::thread_card::fit(&title, room)), style),
            Span::styled(tail, theme::muted()),
        ]));
    }
}

fn append_current_thread(
    detail: &Detail,
    here: Option<&str>,
    names: &HashMap<String, (String, Option<String>)>,
    titles: &HashMap<String, String>,
    width: usize,
    now: u64,
    lines: &mut Vec<Line<'static>>,
) {
    let Some(id) = here else {
        lines.push(Line::styled("0 thread in view: every thread below", theme::muted()));
        lines.push(Line::raw(""));
        return;
    };

    append_thread_totals(detail, titles, width, lines, id);

    append_token_history(detail, width, now, lines);
    lines.push(Line::raw(""));
    append_agent_details(detail, names, lines);

    lines.push(Line::raw(""));
}

fn append_thread_totals(
    detail: &Detail,
    titles: &HashMap<String, String>,
    width: usize,
    lines: &mut Vec<Line<'static>>,
    id: &str,
) {
    let title = titles.get(id).cloned().unwrap_or_else(|| "this thread".into());

    lines.push(heading(&crate::ui::thread_card::fit(&title, width)));
    let sum = |field: fn(&AgentUse) -> u64| detail.agents.iter().map(field).sum::<u64>();
    let (input, output, cache_read, cache_write, turns) = (
        sum(|agent| agent.input),
        sum(|agent| agent.output),
        sum(|agent| agent.cache_read),
        sum(|agent| agent.cache_write),
        sum(|agent| agent.turns),
    );
    let usd: f64 = detail.agents.iter().map(|agent| agent.usd).sum();

    lines.push(field_row(
        "cost",
        format!(
            "${usd:.2} · {} tokens (in {} · out {})",
            human(input + output),
            human(input),
            human(output)
        ),
    ));
    let hit = (cache_read * 100).checked_div(input + cache_read).unwrap_or(0);

    lines.push(field_row(
        "cache",
        format!("read {} · write {} · hit {hit}%", human(cache_read), human(cache_write)),
    ));

    if turns > 0 {
        lines.push(field_row(
            "per turn",
            format!(
                "{turns} turns · ${:.3} · {} tokens",
                usd / turns as f64,
                human((input + output) / turns)
            ),
        ));
    }
}

fn append_token_history(detail: &Detail, width: usize, now: u64, lines: &mut Vec<Line<'static>>) {
    let all: Vec<(u64, u64)> = detail
        .agents
        .iter()
        .flat_map(|agent| agent.samples.iter().copied())
        .collect();
    let spark_width = width.saturating_sub(10).min(48);

    lines.push(Line::from(vec![
        Span::styled(format!("{:<10}", "last 24h"), theme::muted()),
        Span::styled(
            spark_over(&all, spark_width, 86_400, now),
            theme::fg(theme::pal().info()),
        ),
    ]));
    lines.push(Line::from(vec![
        Span::styled(format!("{:<10}", "last hour"), theme::muted()),
        Span::styled(
            spark_over(&all, spark_width, 3_600, now),
            theme::fg(theme::pal().info()),
        ),
    ]));
}

fn append_agent_details(
    detail: &Detail,
    names: &HashMap<String, (String, Option<String>)>,
    lines: &mut Vec<Line<'static>>,
) {
    lines.push(heading("agents"));
    let name_width = detail
        .agents
        .iter()
        .map(|agent| names.get(&agent.agent).map_or(8, |(name, _)| name.chars().count()))
        .max()
        .unwrap_or(5)
        .max(5)
        + 1;

    lines.push(Line::styled(
        format!(
            "{:name_width$}{:>7}{:>7}{:>8}{:>8}{:>6}{:>9}{:>6}",
            "", "in", "out", "cache r", "cache w", "turns", "$", "ctx"
        ),
        theme::muted(),
    ));

    for agent in &detail.agents {
        append_agent_detail(names, lines, name_width, agent);
    }

    if detail.agents.is_empty() {
        lines.push(Line::styled("0 usage yet", theme::muted()));
    }
}

fn append_agent_detail(
    names: &HashMap<String, (String, Option<String>)>,
    lines: &mut Vec<Line<'static>>,
    name_width: usize,
    agent: &AgentUse,
) {
    let (name, color) = names
        .get(&agent.agent)
        .cloned()
        .unwrap_or_else(|| (agent.agent.clone(), None));
    let mut style = Style::default().fg(agents::color(color.as_deref()));

    if agent.live {
        style = style.add_modifier(ratatui::style::Modifier::BOLD);
    }

    let ctx = agent
        .ctx
        .filter(|(_, size)| *size > 0)
        .map_or("·".into(), |(used, size)| format!("{}%", used * 100 / size));

    lines.push(Line::from(vec![
        Span::styled(format!("{name:name_width$}"), style),
        Span::styled(
            format!(
                "{:>7}{:>7}{:>8}{:>8}{:>6}{:>9}{:>6}",
                human(agent.input),
                human(agent.output),
                human(agent.cache_read),
                human(agent.cache_write),
                agent.turns,
                format!("${:.2}", agent.usd),
                ctx
            ),
            theme::text(),
        ),
    ]));
    let ctx_line = agent
        .ctx
        .filter(|(_, size)| *size > 0)
        .map(|(used, size)| format!(" · ctx {} of {}", human(used), human(size)))
        .unwrap_or_default();
    let live = if agent.live { "live" } else { "ended" };

    lines.push(Line::styled(
        format!(
            "{:name_width$}{} · {} session(s) · {live}{ctx_line}",
            "", agent.harness, agent.sessions
        ),
        theme::muted(),
    ));
}

fn heading(text: &str) -> Line<'static> {
    Line::styled(text.to_string(), theme::bold())
}

fn field_row(key: &str, value: String) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{key:<10}"), theme::muted()),
        Span::styled(value, theme::text()),
    ])
}

#[derive(Default)]
struct ThreadTotals {
    usd: f64,
    tokens: u64,
    turns: u64,
}

fn thread_totals(id: &str, since: u64, day_usd: &mut f64) -> ThreadTotals {
    let mut totals = ThreadTotals::default();

    for event in crate::engine::store::events(id) {
        add_thread_turn(&mut totals, &event, since, day_usd);
    }

    totals
}

fn add_thread_turn(totals: &mut ThreadTotals, event: &crate::engine::store::Event, since: u64, day_usd: &mut f64) {
    let crate::engine::store::Kind::TurnEnd {
        cost, input, output, ..
    } = event.kind
    else {
        return;
    };

    totals.usd += cost;
    totals.tokens += input + output;
    totals.turns += 1;

    if event.ts / 1000 > since {
        *day_usd += cost;
    }
}
