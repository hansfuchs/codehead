use super::{
    Call, Color, Drawn, Fold, HashMap, Hit, Item, Line, Modifier, Open, Row, Span, Style, Thoughts, Thread, agents_on,
    call_summary, clock, color_of, markdown, memo, minute_divider, needs_harness, owner_bubble, run_lines,
    run_thought_rows, sub_rows, theme, thought_count, thought_rows, tool_label, top_border,
};
use std::fmt::Write as FormatWrite;

pub(super) fn draw_settled_bubble(
    layout: BubbleAnswerLayout<'_>,
    close: impl Fn(&mut Drawn, &mut Option<Open>),
    gap: impl Fn(&mut Drawn),
    content: BubbleAnswerContent<'_>,
) {
    let BubbleAnswerLayout {
        colors,
        width,
        inner,
        drawn,
        cur,
    } = layout;

    let BubbleAnswerContent {
        agent,
        question,
        answer,
        allow,
        permission,
        asked,
        ts,
    } = content;

    // the question as the agent's message, the answer as the owner's
    close(drawn, cur);
    gap(drawn);
    let border = open_answer_box(drawn, cur, width, colors, agent, permission, asked);
    let bar = [Span::styled("│ ", border)];

    drawn.rows.extend(markdown::plain(question, inner, theme::bold(), &bar));

    if !allow {
        let refusal = if permission { "denied" } else { "declined" };

        drawn.rows.push(Row::plain(Line::from(bar.to_vec())));
        drawn
            .rows
            .extend(markdown::plain(refusal, inner, theme::fg(theme::pal().caution()), &bar));
    }

    close(drawn, cur);

    if allow {
        let answer = if permission && answer.is_empty() {
            "allow"
        } else {
            answer
        };

        gap(drawn);
        owner_bubble(drawn, width, answer, "", ts);
    }
}

pub(super) fn draw_agent_bubble(
    mut context: BubbleAgentContext<'_>,
    close: impl Fn(&mut Drawn, &mut Option<Open>),
    gap: impl Fn(&mut Drawn),
    item: &Item,
    agent: &String,
    ts: u64,
) {
    let border = prepare_agent_bubble(&mut context, &close, &gap, agent, ts);

    draw_agent_content(&mut context, item, agent, border);
}

fn draw_sub_rows(
    thread: &Thread,
    fold: &Fold<'_>,
    inner: usize,
    drawn: &mut Drawn,
    bar: &[Span<'static>; 1],
    sub: usize,
    key: usize,
) {
    if let Some(subagent) = thread.subs.get(sub) {
        sub_rows(drawn, subagent, sub, key, fold, inner, bar);
    }
}
pub(super) fn draw_message(
    context: BubbleTextContext<'_>,
    agent: &String,
    text: &str,
    thoughts: &[String],
    key: usize,
) {
    let BubbleTextContext {
        roster,
        fold,
        inner,
        drawn,
        bar,
    } = context;

    // the thoughts before the message: a folded line on its top
    thought_rows(drawn, thoughts, key, fold, inner, bar);

    if !text.trim().is_empty() {
        drawn.rows.extend(memo::markdown(text, inner, bar));
    }

    if let Some((harness, what)) = needs_harness(text) {
        let label = crate::engine::harnesses::get(&harness).map_or(harness.as_str(), |known| known.label);

        let what = if what.is_empty() {
            String::new()
        } else {
            format!(" for {what}")
        };

        let capable = agents_on(roster, &harness, true, agent);

        drawn.rows.push(Row::plain(Line::from(bar.to_vec())));
        let pre = [bar[0].clone(), Span::styled("● ", theme::fg(theme::pal().attention()))];

        drawn.rows.extend(markdown::plain(
            &format!("{agent} needs a {label} agent{what}."),
            inner,
            theme::fg(theme::pal().attention()),
            &pre,
        ));

        let list = if capable.is_empty() {
            "none".to_string()
        } else {
            capable
                .iter()
                .map(|(name, _)| format!("@{name}"))
                .collect::<Vec<_>>()
                .join(", ")
                + " · @agent → + adds it"
        };

        drawn.rows.extend(markdown::plain(
            &format!("Agents on {label}: {list}"),
            inner,
            theme::muted(),
            &[bar[0].clone(), Span::raw("  ")],
        ));
    }
}
pub(super) fn draw_run(
    context: BubbleRunContext<'_>,
    agent: &String,
    key: usize,
    calls: &[Call],
    start: u64,
    end: u64,
    thoughts: &[(usize, String)],
) {
    let BubbleRunContext {
        thread,
        fold,
        inner,
        drawn,
        bar,
    } = context;

    let shown = fold.run(key);
    let running = calls.iter().any(|call| call.output.is_none()) && thread.busy.contains(agent);

    let mark = if shown {
        "▾ "
    } else if running {
        "◦ "
    } else {
        "▸ "
    };

    let failed = calls
        .iter()
        .filter(|call| matches!(call.output, Some((false, _))))
        .count();
    let (head, mut stats) = run_lines(calls, end.saturating_sub(start) as f64 / 1000.0);
    let think = fold.thoughts != Thoughts::Hidden && (shown || fold.thoughts == Thoughts::Open);
    let tbar = [bar[0].clone(), Span::raw("  ")];

    // `5 calls · 2 thoughts · 12.0s`
    if let Some(at) = stats.rfind(" · ") {
        stats.insert_str(at, &thought_count(thoughts, fold));
    }

    if failed > 0 {
        write!(stats, " · {failed} failed").unwrap();
    }

    let row_at = drawn.rows.len();
    let pre = [bar[0].clone(), Span::styled(mark, theme::accent())];
    let rest = [bar[0].clone(), Span::raw("  ")];
    let mut rows = markdown::plain(&head, inner, theme::text(), &pre);

    for row in rows.iter_mut().skip(1) {
        row.line.spans[1] = Span::raw("  ");
    }

    rows.extend(markdown::plain(&stats, inner, theme::muted(), &rest));

    for i in 0..rows.len() {
        drawn.hits.insert(row_at + i, Hit::Run(key));
    }

    drawn.rows.extend(rows);

    if think && !shown {
        for calls_before in 0..=calls.len() {
            run_thought_rows(drawn, thoughts, calls_before, inner, &tbar);
        }
    }

    if shown {
        draw_run_calls(inner, drawn, bar, calls, thoughts, think, &tbar);
    }
}
pub(super) fn draw_run_calls(
    inner: usize,
    drawn: &mut Drawn,
    bar: &[Span<'static>; 1],
    calls: &[Call],
    thoughts: &[(usize, String)],
    think: bool,
    tbar: &[Span<'static>; 2],
) {
    for (i, call) in calls.iter().enumerate() {
        if think {
            run_thought_rows(drawn, thoughts, i, inner, tbar);
        }

        let head = format!("{} {}", tool_label(&call.name), call_summary(&call.name, &call.input));

        drawn.rows.extend(markdown::plain(
            &head,
            inner,
            theme::text(),
            &[bar[0].clone(), Span::raw("  ")],
        ));
        drawn
            .rows
            .extend(memo::call(call, inner, &[bar[0].clone(), Span::raw("    ")]));
    }

    if think {
        run_thought_rows(drawn, thoughts, calls.len(), inner, tbar);
    }
}

#[derive(Clone, Copy)]
pub(super) struct BubbleAnswerContent<'render> {
    pub(super) agent: &'render str,
    pub(super) question: &'render str,
    pub(super) answer: &'render str,
    pub(super) allow: bool,
    pub(super) permission: bool,
    pub(super) asked: u64,
    pub(super) ts: u64,
}

pub(super) struct BubbleAnswerLayout<'render> {
    pub(super) colors: &'render HashMap<String, Color>,
    pub(super) width: usize,
    pub(super) inner: usize,
    pub(super) drawn: &'render mut Drawn,
    pub(super) cur: &'render mut Option<Open>,
}

pub(super) struct BubbleAgentContext<'render> {
    pub(super) thread: &'render Thread,
    pub(super) colors: &'render HashMap<String, Color>,
    pub(super) roster: &'render [(String, String)],
    pub(super) fold: &'render Fold<'render>,
    pub(super) width: usize,
    pub(super) inner: usize,
    pub(super) drawn: &'render mut Drawn,
    pub(super) cur: &'render mut Option<Open>,
    pub(super) last_of: &'render HashMap<&'render str, usize>,
    pub(super) idx: usize,
}

pub(super) struct BubbleTextContext<'render> {
    pub(super) roster: &'render [(String, String)],
    pub(super) fold: &'render Fold<'render>,
    pub(super) inner: usize,
    pub(super) drawn: &'render mut Drawn,
    pub(super) bar: &'render [Span<'static>; 1],
}

pub(super) struct BubbleRunContext<'render> {
    pub(super) thread: &'render Thread,
    pub(super) fold: &'render Fold<'render>,
    pub(super) inner: usize,
    pub(super) drawn: &'render mut Drawn,
    pub(super) bar: &'render [Span<'static>; 1],
}

fn prepare_agent_bubble(
    context: &mut BubbleAgentContext<'_>,
    close: &impl Fn(&mut Drawn, &mut Option<Open>),
    gap: &impl Fn(&mut Drawn),
    agent: &str,
    ts: u64,
) -> Style {
    let color = color_of(context.colors, agent);
    let border = Style::default().fg(theme::desaturate(color, 0.4));
    let bar = [Span::styled("│ ", border)];

    if context.cur.as_ref().and_then(|open| open.agent.as_deref()) != Some(agent) {
        close(context.drawn, context.cur);
        gap(context.drawn);

        let busy = at_work_to_the_end(context, agent);
        let name = vec![Span::styled(
            agent.to_owned(),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        )];

        // the box's time: its 1st item's (a later minute: a divider with its time)
        let (top, _) = top_border(context.width, border, Vec::new(), false, &clock(ts));

        *context.cur = Some(Open {
            start: context.drawn.rows.len(),
            border,
            agent: Some(agent.to_owned()),
            minute: clock(ts),
            name,
            wave: busy.then_some(color),
        });
        context.drawn.rows.push(top);
    } else if let Some(open) = context.cur.as_mut()
        && open.minute != clock(ts)
    {
        // a new minute in the same box: blank, `── 13:42`, blank (the item's own)
        open.minute = clock(ts);
        context.drawn.rows.push(Row::plain(Line::from(bar.to_vec())));
        context
            .drawn
            .rows
            .push(minute_divider(context.width, border, &open.minute));
        context.drawn.rows.push(Row::plain(Line::from(bar.to_vec())));
    } else {
        // the next item of the box: 1 blank row between (0 below the top border)
        context.drawn.rows.push(Row::plain(Line::from(bar.to_vec())));
    }

    border
}

/// AGENT works and every item from this one on is its own: its box gets the wave.
fn at_work_to_the_end(context: &BubbleAgentContext<'_>, agent: &str) -> bool {
    let own = |item: &Item| matches!(item, Item::Text { agent: owner, .. } | Item::Run { agent: owner, .. } | Item::Sub { agent: owner, .. } if owner == agent);

    context.thread.working(agent)
        && context
            .last_of
            .get(agent)
            .is_some_and(|&last| last >= context.idx && context.thread.items[context.idx..=last].iter().all(own))
}

fn draw_agent_content(context: &mut BubbleAgentContext<'_>, item: &Item, agent: &String, border: Style) {
    let bar = [Span::styled("│ ", border)];

    match item {
        Item::Text {
            text, thoughts, key, ..
        } => {
            draw_message(
                BubbleTextContext {
                    roster: context.roster,
                    fold: context.fold,
                    inner: context.inner,
                    drawn: context.drawn,
                    bar: &bar,
                },
                agent,
                text,
                thoughts,
                *key,
            );
        }
        Item::Run {
            key,
            calls,
            start,
            end,
            thoughts,
            ..
        } => {
            draw_run(
                BubbleRunContext {
                    thread: context.thread,
                    fold: context.fold,
                    inner: context.inner,
                    drawn: context.drawn,
                    bar: &bar,
                },
                agent,
                *key,
                calls,
                *start,
                *end,
                thoughts,
            );
        }
        Item::Sub { sub, key, .. } => {
            draw_sub_rows(
                context.thread,
                context.fold,
                context.inner,
                context.drawn,
                &bar,
                *sub,
                *key,
            );
        }
        _ => {}
    }
}

fn open_answer_box(
    drawn: &mut Drawn,
    cur: &mut Option<Open>,
    width: usize,
    colors: &HashMap<String, Color>,
    agent: &str,
    permission: bool,
    asked: u64,
) -> Style {
    let color = color_of(colors, agent);
    let border = Style::default().fg(theme::desaturate(color, 0.4));
    let what = if permission { " asked to run" } else { " asked" };
    let name = vec![
        Span::styled(
            agent.to_owned(),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ),
        Span::styled(what, theme::muted()),
    ];

    let (top, _) = top_border(width, border, Vec::new(), false, &clock(asked));

    *cur = Some(Open {
        start: drawn.rows.len(),
        border,
        agent: None,
        minute: String::new(),
        name,
        wave: None,
    });
    drawn.rows.push(top);

    border
}
