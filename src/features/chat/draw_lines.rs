use self::messages::{AnswerContent, LineMessageContext, TextContent};
use super::{
    Attachment, Call, Color, Colors, Drawn, Fold, HashMap, Hit, Item, Line, Modifier, Picker, Row, Span, Style,
    Thoughts, Thread, UnicodeWidthStr, agents_on, ask_rows, attachments, badge, call_summary, clip_row, clock,
    color_of, edits_row, error_hint, kid_rows, kid_tag, link_markers, markdown, memo, needs_harness, resume_of,
    run_summary, run_thought_rows, shell_rows, sub_rows, theme, thought_count, thought_rows, tool_label,
};
use std::fmt::Write as FormatWrite;

pub(super) fn draw_lines(
    thread: &Thread,
    width: u16,
    colors: &Colors,
    roster: &[(String, String)],
    fold: &Fold,
    ask: &Picker,
) -> Drawn {
    let width = usize::from(width.max(10));
    let mut drawn = Drawn::default();
    let blank = |drawn: &mut Drawn| drawn.rows.push(Row::plain(Line::raw("")));
    let mut last_agent: Option<String> = None;

    // the minute of the last item of LAST_AGENT: a later one gets `│ ── 13:42`
    let mut minute = String::new();

    let divider = |drawn: &mut Drawn, color: Color, at: &str| {
        let fill = width.saturating_sub(2 + at.width() + 1);

        drawn.rows.push(Row::plain(Line::from(vec![
            Span::styled("│ ", Style::default().fg(color)),
            Span::styled("─".repeat(fill), theme::line()),
            Span::styled(format!(" {at}"), theme::muted()),
        ])));
        drawn
            .rows
            .push(Row::plain(Line::from(Span::styled("│", Style::default().fg(color)))));
    };

    for item in &thread.items {
        draw_line_item(
            LineRenderContext {
                thread,
                colors,
                roster,
                fold,
                width,
                drawn: &mut drawn,
                last_agent: &mut last_agent,
                minute: &mut minute,
            },
            blank,
            divider,
            item,
        );
    }

    // the 1st open ask: below everything, its choices clickable
    finish_draw_lines(thread, colors, ask, width, drawn, blank)
}

fn draw_line_item(
    mut context: LineRenderContext<'_>,
    blank: impl Fn(&mut Drawn),
    divider: impl Fn(&mut Drawn, Color, &str),
    item: &Item,
) {
    context.drawn.starts.push(context.drawn.rows.len());

    match item {
        Item::Prompt { from, to, text, ts } if from == "owner" => {
            draw_owner_prompt(context.width, context.drawn, blank, context.last_agent, to, text, *ts);
        }
        Item::Prompt { .. } => {
            render_line_prompt(&mut context, &blank, &divider, item);
        }
        // only thoughts, hidden: 0 rows
        Item::Text { text, thoughts, .. }
            if text.trim().is_empty() && (thoughts.is_empty() || context.fold.thoughts == Thoughts::Hidden) => {}
        Item::Text { .. } => {
            render_line_text(&mut context, &blank, &divider, item);
        }
        Item::Run { .. } => {
            render_line_run(&mut context, &blank, &divider, item);
        }
        Item::Sub { .. } => {
            render_line_sub(&mut context, &blank, &divider, item);
        }
        Item::Kid { .. } => {
            render_line_kid(&mut context, &blank, &divider, item);
        }
        Item::Shell { .. } => {
            render_line_shell(&mut context, &blank, &divider, item);
        }
        Item::Note { .. } => {
            render_line_note(&mut context, &blank, &divider, item);
        }
        Item::Edits { .. } => {
            render_line_edits(&mut context, &blank, &divider, item);
        }
        Item::Settled { .. } => {
            render_line_settled(&mut context, &blank, &divider, item);
        }
    }
}

fn draw_agent_prompt(
    context: &mut LineRenderContext<'_>,
    blank: &impl Fn(&mut Drawn),
    from: &str,
    to: &[String],
    text: &str,
    ts: u64,
) {
    blank(context.drawn);
    let color = color_of(context.colors, from);
    let mut head = badge(from, color, ts);

    head.push(Span::styled(format!(" → {}", to.join(", ")), theme::muted()));
    context.drawn.rows.push(Row::plain(Line::from(head)));
    let bar = [Span::styled("┆ ", Style::default().fg(color))];

    context
        .drawn
        .rows
        .extend(markdown::plain(text.trim_end(), context.width, theme::muted(), &bar));
    *context.last_agent = None;
}

fn draw_subagent_end(
    context: &mut LineRenderContext<'_>,
    blank: &impl Fn(&mut Drawn),
    agent: &str,
    sub: usize,
    ts: u64,
    key: usize,
) {
    let color = color_of(context.colors, agent);

    blank(context.drawn);
    context.drawn.rows.push(Row::plain(Line::from(badge(agent, color, ts))));

    if let Some(subagent) = context.thread.subs.get(sub) {
        sub_rows(
            context.drawn,
            subagent,
            sub,
            key,
            context.fold,
            context.width,
            &[Span::styled("│ ", Style::default().fg(color))],
        );
    }

    *context.last_agent = None;
}

fn draw_note_line(context: LineNoteContext<'_>, item: &Item, agent: &String, text: &str, color: Color) {
    let LineNoteContext {
        thread,
        colors,
        roster,
        width,
        drawn,
        last_agent,
    } = context;

    let agent_color = color_of(colors, agent);
    let pre = [
        Span::styled("● ", Style::default().fg(color)),
        Span::styled(format!("{agent} "), Style::default().fg(agent_color)),
    ];

    drawn
        .rows
        .extend(markdown::plain(text, width, Style::default().fg(color), &pre));

    if color == theme::pal().error() {
        for hint in error_hint(agent, text, roster, resume_of(thread, item)) {
            drawn
                .rows
                .extend(markdown::plain(&hint, width, theme::muted(), &[Span::raw("  ")]));
        }
    }

    *last_agent = None;
}

fn finish_draw_lines(
    thread: &Thread,
    colors: &HashMap<String, Color>,
    ask: &Picker,
    width: usize,
    mut drawn: Drawn,
    blank: impl Fn(&mut Drawn),
) -> Drawn {
    if let Some(open_ask) = thread.asks.first() {
        let agent_color = color_of(colors, &open_ask.agent);

        blank(&mut drawn);
        let edge = theme::fg(theme::pal().attention());

        let more = if thread.asks.len() > 1 {
            format!(" · {} more waiting", thread.asks.len() - 1)
        } else {
            String::new()
        };

        let what = if open_ask.tool == crate::engine::flows::run::WAIT_TOOL {
            "waits on you"
        } else if open_ask.questions().is_empty() {
            "asks to run"
        } else {
            "asks"
        };

        drawn.rows.push(Row::plain(Line::from(vec![
            Span::styled("┃ ", edge),
            Span::styled(
                open_ask.agent.clone(),
                Style::default().fg(agent_color).add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!(" {what}{more}"), edge),
        ])));
        ask_rows(&mut drawn, open_ask, width, &[Span::styled("┃ ", edge)], ask);
    }

    drawn
}

fn draw_run_item(
    layout: &mut RunRenderContext<'_>,
    blank: impl Fn(&mut Drawn),
    divider: impl Fn(&mut Drawn, Color, &str),
    content: RunContent<'_>,
) {
    let RunContent {
        agent,
        key,
        calls,
        start,
        end,
        thoughts,
    } = content;

    let color = begin_run(layout, &blank, &divider, agent, start);
    let shown = layout.fold.run(key);
    let running = calls.iter().any(|call| call.output.is_none()) && layout.thread.busy.contains(agent);
    let mark = layout.fold.run_mark(key, running);
    let summary = summary_with_thoughts(calls, start, end, thoughts, layout.fold);

    let fold: &Fold<'_> = layout.fold;
    let width: usize = layout.width;
    let drawn: &mut Drawn = layout.drawn;

    let think = fold.thoughts != Thoughts::Hidden && (shown || fold.thoughts == Thoughts::Open);
    let tbar = [Span::styled("│   ", Style::default().fg(color))];
    let row_at = drawn.rows.len();
    let pre = [
        Span::styled("│ ", Style::default().fg(color)),
        Span::styled(mark, theme::accent()),
    ];
    let rest = [Span::styled("│   ", Style::default().fg(color))];

    for (i, row) in markdown::plain(&summary, width, theme::muted(), &pre)
        .into_iter()
        .enumerate()
    {
        let row = if i == 0 {
            row
        } else {
            Row {
                line: Line::from([rest.to_vec(), row.line.spans[2..].to_vec()].concat()),
                ..row
            }
        };

        drawn.hits.insert(row_at + i, Hit::Run(key));
        drawn.rows.push(row);
    }

    let body_at = drawn.rows.len();

    if think && !shown {
        for calls_before in 0..=calls.len() {
            run_thought_rows(drawn, thoughts, calls_before, width, &tbar);
        }
    }

    if shown {
        draw_run_calls(width, drawn, calls, thoughts, color, think, &tbar);
    }

    drawn.body(body_at, Hit::Run(key));
}

fn draw_run_calls(
    width: usize,
    drawn: &mut Drawn,
    calls: &[Call],
    thoughts: &[(usize, String)],
    color: Color,
    think: bool,
    tbar: &[Span<'static>; 1],
) {
    for (i, call) in calls.iter().enumerate() {
        if think {
            run_thought_rows(drawn, thoughts, i, width, tbar);
        }

        let head = format!("{} {}", tool_label(&call.name), call_summary(&call.name, &call.input));
        let bar = [Span::styled("│   ", Style::default().fg(color))];

        drawn.rows.extend(markdown::plain(&head, width, theme::text(), &bar));
        drawn.rows.extend(memo::call(
            call,
            width,
            &[Span::styled("│     ", Style::default().fg(color))],
        ));
    }

    if think {
        run_thought_rows(drawn, thoughts, calls.len(), width, tbar);
    }
}

#[path = "line_messages.rs"]
mod messages;
use messages::{draw_message_lines, draw_owner_prompt, draw_settled_line};

pub(super) struct LineRenderContext<'render> {
    pub(super) thread: &'render Thread,
    pub(super) colors: &'render HashMap<String, Color>,
    pub(super) roster: &'render [(String, String)],
    pub(super) fold: &'render Fold<'render>,
    pub(super) width: usize,
    pub(super) drawn: &'render mut Drawn,
    pub(super) last_agent: &'render mut Option<String>,
    pub(super) minute: &'render mut String,
}

pub(super) struct LineNoteContext<'render> {
    pub(super) thread: &'render Thread,
    pub(super) colors: &'render HashMap<String, Color>,
    pub(super) roster: &'render [(String, String)],
    pub(super) width: usize,
    pub(super) drawn: &'render mut Drawn,
    pub(super) last_agent: &'render mut Option<String>,
}

pub(super) struct RunRenderContext<'render> {
    pub(super) thread: &'render Thread,
    pub(super) colors: &'render HashMap<String, Color>,
    pub(super) fold: &'render Fold<'render>,
    pub(super) width: usize,
    pub(super) drawn: &'render mut Drawn,
    pub(super) last_agent: &'render mut Option<String>,
    pub(super) minute: &'render mut String,
}

#[derive(Clone, Copy)]
pub(super) struct RunContent<'render> {
    pub(super) agent: &'render String,
    pub(super) key: usize,
    pub(super) calls: &'render Vec<Call>,
    pub(super) start: u64,
    pub(super) end: u64,
    pub(super) thoughts: &'render Vec<(usize, String)>,
}

fn begin_run(
    layout: &mut RunRenderContext<'_>,
    blank: &impl Fn(&mut Drawn),
    divider: &impl Fn(&mut Drawn, Color, &str),
    agent: &str,
    start: u64,
) -> Color {
    let color = color_of(layout.colors, agent);

    if layout.last_agent.as_deref() != Some(agent) {
        blank(layout.drawn);
        layout
            .drawn
            .rows
            .push(Row::plain(Line::from(badge(agent, color, start))));
        *layout.last_agent = Some(agent.to_string());
    } else if *layout.minute != clock(start) {
        blank(layout.drawn);
        divider(layout.drawn, color, &clock(start));
    }

    *layout.minute = clock(start);

    color
}

fn summary_with_thoughts(calls: &[Call], start: u64, end: u64, thoughts: &[(usize, String)], fold: &Fold) -> String {
    let failed = calls
        .iter()
        .filter(|call| matches!(call.output, Some((false, _))))
        .count();
    let mut summary = run_summary(calls, end.saturating_sub(start) as f64 / 1000.0);

    summary.push_str(&thought_count(thoughts, fold));

    if failed > 0 {
        write!(summary, " · {failed} failed").unwrap();
    }

    summary
}

#[path = "line_routes.rs"]
mod routes;
use routes::{
    render_line_edits, render_line_kid, render_line_note, render_line_prompt, render_line_run, render_line_settled,
    render_line_shell, render_line_sub, render_line_text,
};
