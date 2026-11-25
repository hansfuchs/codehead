use super::{
    Call, Color, Drawn, Fold, HashMap, Hit, Item, Line, Modifier, Open, Row, Span, Style, Thoughts, Thread, agents_on,
    call_summary, clock, color_of, markdown, memo, minute_divider, needs_harness, owner_bubble, run_lines,
    run_thought_rows, sub_rows, theme, thought_count, thought_rows, tool_label, top_border,
};
use std::fmt::Write as FormatWrite;

pub(super) fn draw_bubbles_item_settled(
    layout: BubbleAnswerLayout<'_>,
    close: impl Fn(&mut Drawn, &mut Option<Open>),
    gap: impl Fn(&mut Drawn),
    content: BubbleAnswerContent<'_>,
) {
    let BubbleAnswerLayout {
        colors,
        width_value,
        inner,
        directory,
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
    close(directory, cur);
    gap(directory);
    let border = open_answer_box(directory, cur, width_value, colors, agent, permission, asked);
    let bar = [Span::styled("│ ", border)];

    directory
        .rows
        .extend(markdown::plain(question, inner, theme::bold(), &bar));

    if !allow {
        let st = if permission { "denied" } else { "declined" };

        directory.rows.push(Row::plain(Line::from(bar.to_vec())));
        directory
            .rows
            .extend(markdown::plain(st, inner, theme::fg(theme::pal().caution()), &bar));
    }

    close(directory, cur);

    if allow {
        let answer = if permission && answer.is_empty() {
            "allow"
        } else {
            answer
        };

        gap(directory);
        owner_bubble(directory, width_value, answer, "", ts);
    }
}

pub(super) fn draw_bubbles_item_text_agent_ts(
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

fn draw_bubbles_item_text_agent_ts_item_sub_sub_key(
    thread: &Thread,
    fold: &Fold<'_>,
    inner: usize,
    directory: &mut Drawn,
    bar: &[Span<'static>; 1],
    sub: usize,
    key: usize,
) {
    if let Some(text_value) = thread.subs.get(sub) {
        sub_rows(directory, text_value, sub, key, fold, inner, bar);
    }
}
pub(super) fn draw_bubbles_item_text_agent_ts_item_text(
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
        directory,
        bar,
    } = context;

    // the thoughts before the message: a folded line on its top
    thought_rows(directory, thoughts, key, fold, inner, bar);

    if !text.trim().is_empty() {
        directory.rows.extend(memo::markdown(text, inner, bar));
    }

    if let Some((height, what)) = needs_harness(text) {
        let label = crate::engine::harnesses::get(&height).map_or(height.as_str(), |x| x.label);

        let what = if what.is_empty() {
            String::new()
        } else {
            format!(" for {what}")
        };

        let cap = agents_on(roster, &height, true, agent);

        directory.rows.push(Row::plain(Line::from(bar.to_vec())));
        let pre = [bar[0].clone(), Span::styled("● ", theme::fg(theme::pal().attention()))];

        directory.rows.extend(markdown::plain(
            &format!("{agent} needs a {label} agent{what}."),
            inner,
            theme::fg(theme::pal().attention()),
            &pre,
        ));

        let list = if cap.is_empty() {
            "none".to_string()
        } else {
            cap.iter()
                .map(|(count_value, _)| format!("@{count_value}"))
                .collect::<Vec<_>>()
                .join(", ")
                + " · @agent → + adds it"
        };

        directory.rows.extend(markdown::plain(
            &format!("Agents on {label}: {list}"),
            inner,
            theme::muted(),
            &[bar[0].clone(), Span::raw("  ")],
        ));
    }
}
pub(super) fn draw_bubbles_item_text_agent_ts_item_run(
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
        directory,
        bar,
    } = context;

    let shown = fold.run(key);
    let running = calls.iter().any(|character_4| character_4.output.is_none()) && thread.busy.contains(agent);

    let mark = if shown {
        "▾ "
    } else if running {
        "◦ "
    } else {
        "▸ "
    };

    let failed = calls
        .iter()
        .filter(|character_5| matches!(character_5.output, Some((false, _))))
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

    let row_at = directory.rows.len();
    let pre = [bar[0].clone(), Span::styled(mark, theme::accent())];
    let rest = [bar[0].clone(), Span::raw("  ")];
    let mut rows = markdown::plain(&head, inner, theme::text(), &pre);

    for row_value in rows.iter_mut().skip(1) {
        row_value.line.spans[1] = Span::raw("  ");
    }

    rows.extend(markdown::plain(&stats, inner, theme::muted(), &rest));

    for i in 0..rows.len() {
        directory.hits.insert(row_at + i, Hit::Run(key));
    }

    directory.rows.extend(rows);

    if think && !shown {
        for count_2 in 0..=calls.len() {
            run_thought_rows(directory, thoughts, count_2, inner, &tbar);
        }
    }

    if shown {
        draw_bubbles_item_text_agent_ts_item_run_head(inner, directory, bar, calls, thoughts, think, &tbar);
    }
}
pub(super) fn draw_bubbles_item_text_agent_ts_item_run_head(
    inner: usize,
    directory: &mut Drawn,
    bar: &[Span<'static>; 1],
    calls: &[Call],
    thoughts: &[(usize, String)],
    think: bool,
    tbar: &[Span<'static>; 2],
) {
    for (count_3, call) in calls.iter().enumerate() {
        if think {
            run_thought_rows(directory, thoughts, count_3, inner, tbar);
        }

        let head = format!("{} {}", tool_label(&call.name), call_summary(&call.name, &call.input));

        directory.rows.extend(markdown::plain(
            &head,
            inner,
            theme::text(),
            &[bar[0].clone(), Span::raw("  ")],
        ));
        directory
            .rows
            .extend(memo::call(call, inner, &[bar[0].clone(), Span::raw("    ")]));
    }

    if think {
        run_thought_rows(directory, thoughts, calls.len(), inner, tbar);
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
    pub(super) width_value: usize,
    pub(super) inner: usize,
    pub(super) directory: &'render mut Drawn,
    pub(super) cur: &'render mut Option<Open>,
}

pub(super) struct BubbleAgentContext<'render> {
    pub(super) thread: &'render Thread,
    pub(super) colors: &'render HashMap<String, Color>,
    pub(super) roster: &'render [(String, String)],
    pub(super) fold: &'render Fold<'render>,
    pub(super) width_value: usize,
    pub(super) inner: usize,
    pub(super) directory: &'render mut Drawn,
    pub(super) cur: &'render mut Option<Open>,
    pub(super) last_of: &'render HashMap<&'render str, usize>,
    pub(super) idx: usize,
}

pub(super) struct BubbleTextContext<'render> {
    pub(super) roster: &'render [(String, String)],
    pub(super) fold: &'render Fold<'render>,
    pub(super) inner: usize,
    pub(super) directory: &'render mut Drawn,
    pub(super) bar: &'render [Span<'static>; 1],
}

pub(super) struct BubbleRunContext<'render> {
    pub(super) thread: &'render Thread,
    pub(super) fold: &'render Fold<'render>,
    pub(super) inner: usize,
    pub(super) directory: &'render mut Drawn,
    pub(super) bar: &'render [Span<'static>; 1],
}

fn prepare_agent_bubble(
    context: &mut BubbleAgentContext<'_>,
    close: &impl Fn(&mut Drawn, &mut Option<Open>),
    gap: &impl Fn(&mut Drawn),
    agent: &String,
    ts: u64,
) -> Style {
    let character_3 = color_of(context.colors, agent);
    let border = Style::default().fg(theme::desaturate(character_3, 0.4));
    let bar = [Span::styled("│ ", border)];

    if context.cur.as_ref().and_then(|output_3| output_3.agent.as_deref()) != Some(agent.as_str()) {
        close(context.directory, context.cur);
        gap(context.directory);

        // the box's time: its 1st item's (a later minute: a divider with its time)
        let busy = context.thread.working(agent) && context.last_of.get(agent.as_str()).is_some_and(|&line_2| line_2 >= context.idx && context.thread.items[context.idx..=line_2].iter().all(|it| matches!(it, Item::Text { agent: agent_value, .. } | Item::Run { agent: agent_value, .. } | Item::Sub { agent: agent_value, .. } if agent_value == agent)));
        let name = vec![Span::styled(
            agent.clone(),
            Style::default().fg(character_3).add_modifier(Modifier::BOLD),
        )];
        let (top, _) = top_border(context.width_value, border, Vec::new(), false, &clock(ts));

        *context.cur = Some(Open {
            start: context.directory.rows.len(),
            border,
            agent: Some(agent.clone()),
            minute: clock(ts),
            name,
            wave: busy.then_some(character_3),
        });
        context.directory.rows.push(top);
    } else if let Some(output_4) = context.cur.as_mut()
        && output_4.minute != clock(ts)
    {
        // a new minute in the same box: blank, `── 13:42`, blank (the item's own)
        output_4.minute = clock(ts);
        context.directory.rows.push(Row::plain(Line::from(bar.to_vec())));
        context
            .directory
            .rows
            .push(minute_divider(context.width_value, border, &output_4.minute));
        context.directory.rows.push(Row::plain(Line::from(bar.to_vec())));
    } else {
        // the next item of the box: 1 blank row between (0 below the top border)
        context.directory.rows.push(Row::plain(Line::from(bar.to_vec())));
    }

    border
}

fn draw_agent_content(context: &mut BubbleAgentContext<'_>, item: &Item, agent: &String, border: Style) {
    let bar = [Span::styled("│ ", border)];

    match item {
        Item::Text {
            text, thoughts, key, ..
        } => {
            draw_bubbles_item_text_agent_ts_item_text(
                BubbleTextContext {
                    roster: context.roster,
                    fold: context.fold,
                    inner: context.inner,
                    directory: context.directory,
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
            draw_bubbles_item_text_agent_ts_item_run(
                BubbleRunContext {
                    thread: context.thread,
                    fold: context.fold,
                    inner: context.inner,
                    directory: context.directory,
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
            draw_bubbles_item_text_agent_ts_item_sub_sub_key(
                context.thread,
                context.fold,
                context.inner,
                context.directory,
                &bar,
                *sub,
                *key,
            );
        }
        _ => {}
    }
}

fn open_answer_box(
    directory: &mut Drawn,
    cur: &mut Option<Open>,
    width_value: usize,
    colors: &HashMap<String, Color>,
    agent: &str,
    permission: bool,
    asked: u64,
) -> Style {
    let character_value = color_of(colors, agent);
    let border = Style::default().fg(theme::desaturate(character_value, 0.4));
    let what = if permission { " asked to run" } else { " asked" };
    let name = vec![
        Span::styled(
            agent.to_owned(),
            Style::default().fg(character_value).add_modifier(Modifier::BOLD),
        ),
        Span::styled(what, theme::muted()),
    ];

    let (top, _) = top_border(width_value, border, Vec::new(), false, &clock(asked));

    *cur = Some(Open {
        start: directory.rows.len(),
        border,
        agent: None,
        minute: String::new(),
        name,
        wave: None,
    });
    directory.rows.push(top);

    border
}
