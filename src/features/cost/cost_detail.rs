use super::{
    AgentUse, Detail, HashMap, Line, SPARK, Span, Style, agents, bar, human, limits, now, theme, until, uses, window,
};

/// Read the detail of thread ID (None: 0 thread part).
pub(crate) fn detail(id: Option<&str>) -> Detail {
    let mut directory = Detail::default();
    let since = now().saturating_sub(86_400);

    if let Some(id) = id.filter(|i| !i.is_empty()) {
        directory.agents = uses(id);

        for agent in &mut directory.agents {
            agent.samples.retain(|(at, _)| *at > since);
        }
    }

    let mut rows: Vec<(String, f64, u64, u64)> = Vec::new();

    for thread in crate::engine::store::all() {
        let totals = thread_totals(&thread.id, since, &mut directory.day_usd);

        if totals.turns > 0 {
            rows.push((thread.id, totals.usd, totals.tokens, totals.turns));
        }
    }

    rows.sort_by(|agent_value, bytes| bytes.1.total_cmp(&agent_value.1));
    directory.all_usd = rows.iter().map(|result| result.1).sum();
    directory.all_tokens = rows.iter().map(|result_value| result_value.2).sum();
    directory.threads = rows.into_iter().take(8).collect();
    directory.limits = limits();

    directory
}

/// BUCKETS buckets of SPAN seconds up to NOW; empty: `·`.
pub(super) fn spark_over(samples: &[(u64, u64)], buckets: usize, span: u64, now: u64) -> String {
    if buckets == 0 || span == 0 {
        return String::new();
    }

    let mut bytes = vec![0u64; buckets];
    let start = now.saturating_sub(span);

    for &(at, tok) in samples {
        if at > start {
            let i = ((u128::from(at - start) * buckets as u128 / u128::from(span)) as usize).min(buckets - 1);

            bytes[i] += tok;
        }
    }

    let max = bytes.iter().copied().max().unwrap_or(0).max(1);

    bytes
        .iter()
        .map(|&value| {
            if value == 0 {
                '·'
            } else {
                SPARK[((value * 7) / max) as usize]
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
    lines.push(key_value(
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

    for (height, win, pct, resets) in &detail.limits {
        let col = if *pct >= 90.0 {
            theme::pal().error()
        } else if *pct >= 70.0 {
            theme::pal().caution()
        } else {
            theme::pal().text
        };

        let reset = match resets {
            Some(result) if *result <= now => " · new window".to_string(),
            Some(result_value) => format!(" · resets in {}", until(*result_value, now)),
            None => String::new(),
        };

        let mut spans = vec![Span::styled(
            format!("{:<10}", format!("{height} {}", window(win))),
            theme::muted(),
        )];

        spans.extend(bar(*pct, 0.0, 20));
        spans.push(Span::styled(format!(" {pct:.0}%"), Style::default().fg(col)));
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

        let st = if here == Some(id.as_str()) {
            theme::accent()
        } else {
            theme::text()
        };

        let tail = format!("  {} · {turns} turns", human(*tokens));
        let room = width.saturating_sub(10 + tail.chars().count() + 2);

        lines.push(Line::from(vec![
            Span::styled(format!("{mark}{:>8}", format!("${usd:.2}")), st),
            Span::styled(format!("  {}", crate::ui::thread_card::fit(&title, room)), st),
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
    let sum = |file: fn(&AgentUse) -> u64| detail.agents.iter().map(file).sum::<u64>();
    let (input_value, output_2, cache_read_value, cache_write_value, turns) = (
        sum(|agent_value| agent_value.input),
        sum(|agent_2| agent_2.output),
        sum(|agent_3| agent_3.cache_read),
        sum(|agent_4| agent_4.cache_write),
        sum(|agent_5| agent_5.turns),
    );
    let usd: f64 = detail.agents.iter().map(|agent_6| agent_6.usd).sum();

    lines.push(key_value(
        "cost",
        format!(
            "${usd:.2} · {} tokens (in {} · out {})",
            human(input_value + output_2),
            human(input_value),
            human(output_2)
        ),
    ));
    let hit = (cache_read_value * 100)
        .checked_div(input_value + cache_read_value)
        .unwrap_or(0);

    lines.push(key_value(
        "cache",
        format!(
            "read {} · write {} · hit {hit}%",
            human(cache_read_value),
            human(cache_write_value)
        ),
    ));

    if turns > 0 {
        lines.push(key_value(
            "per turn",
            format!(
                "{turns} turns · ${:.3} · {} tokens",
                usd / turns as f64,
                human((input_value + output_2) / turns)
            ),
        ));
    }
}

fn append_token_history(detail: &Detail, width: usize, now: u64, lines: &mut Vec<Line<'static>>) {
    let all: Vec<(u64, u64)> = detail
        .agents
        .iter()
        .flat_map(|agent_7| agent_7.samples.iter().copied())
        .collect();
    let sw = width.saturating_sub(10).min(48);

    lines.push(Line::from(vec![
        Span::styled(format!("{:<10}", "last 24h"), theme::muted()),
        Span::styled(spark_over(&all, sw, 86_400, now), theme::fg(theme::pal().info())),
    ]));
    lines.push(Line::from(vec![
        Span::styled(format!("{:<10}", "last hour"), theme::muted()),
        Span::styled(spark_over(&all, sw, 3_600, now), theme::fg(theme::pal().info())),
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
        .map(|agent_8| {
            names
                .get(&agent_8.agent)
                .map_or(8, |count_value| count_value.0.chars().count())
        })
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

    for agent_9 in &detail.agents {
        append_agent_detail(names, lines, name_width, agent_9);
    }

    if detail.agents.is_empty() {
        lines.push(Line::styled("0 usage yet", theme::muted()));
    }
}

fn append_agent_detail(
    names: &HashMap<String, (String, Option<String>)>,
    lines: &mut Vec<Line<'static>>,
    name_width: usize,
    agent_value: &AgentUse,
) {
    let (name, color) = names
        .get(&agent_value.agent)
        .cloned()
        .unwrap_or_else(|| (agent_value.agent.clone(), None));
    let mut st = Style::default().fg(agents::color(color.as_deref()));

    if agent_value.live {
        st = st.add_modifier(ratatui::style::Modifier::BOLD);
    }

    let ctx = agent_value
        .ctx
        .filter(|character| character.1 > 0)
        .map_or("·".into(), |(user, text_value)| {
            format!("{}%", user * 100 / text_value)
        });

    lines.push(Line::from(vec![
        Span::styled(format!("{name:name_width$}"), st),
        Span::styled(
            format!(
                "{:>7}{:>7}{:>8}{:>8}{:>6}{:>9}{:>6}",
                human(agent_value.input),
                human(agent_value.output),
                human(agent_value.cache_read),
                human(agent_value.cache_write),
                agent_value.turns,
                format!("${:.2}", agent_value.usd),
                ctx
            ),
            theme::text(),
        ),
    ]));
    let ctx_line = agent_value
        .ctx
        .filter(|character_value| character_value.1 > 0)
        .map(|(user_value, text_2)| format!(" · ctx {} of {}", human(user_value), human(text_2)))
        .unwrap_or_default();
    let live = if agent_value.live { "live" } else { "ended" };

    lines.push(Line::styled(
        format!(
            "{:name_width$}{} · {} session(s) · {live}{ctx_line}",
            "", agent_value.harness, agent_value.sessions
        ),
        theme::muted(),
    ));
}

fn heading(text: &str) -> Line<'static> {
    Line::styled(text.to_string(), theme::bold())
}

fn key_value(key: &str, value: String) -> Line<'static> {
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
