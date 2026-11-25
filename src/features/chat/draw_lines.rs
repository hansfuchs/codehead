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
    let width_value = usize::from(width.max(10));
    let mut directory = Drawn::default();
    let blank = |directory_value: &mut Drawn| directory_value.rows.push(Row::plain(Line::raw("")));
    let mut last_agent: Option<String> = None;

    // the minute of the last item of LAST_AGENT: a later one gets `│ ── 13:42`
    let mut minute = String::new();

    let divider = |directory_2: &mut Drawn, color_value: Color, at: &str| {
        let fill = width_value.saturating_sub(2 + at.width() + 1);

        directory_2.rows.push(Row::plain(Line::from(vec![
            Span::styled("│ ", Style::default().fg(color_value)),
            Span::styled("─".repeat(fill), theme::line()),
            Span::styled(format!(" {at}"), theme::muted()),
        ])));
        directory_2.rows.push(Row::plain(Line::from(Span::styled(
            "│",
            Style::default().fg(color_value),
        ))));
    };

    for item in &thread.items {
        draw_lines_character(
            LineRenderContext {
                thread,
                colors,
                roster,
                fold,
                width_value,
                directory: &mut directory,
                last_agent: &mut last_agent,
                minute: &mut minute,
            },
            blank,
            divider,
            item,
        );
    }

    // the 1st open ask: below everything, its choices clickable
    finish_draw_lines(thread, colors, ask, width_value, directory, blank)
}

fn draw_lines_character(
    mut context: LineRenderContext<'_>,
    blank: impl Fn(&mut Drawn),
    divider: impl Fn(&mut Drawn, Color, &str),
    item: &Item,
) {
    context.directory.starts.push(context.directory.rows.len());

    match item {
        Item::Prompt { from, to, text, ts } if from == "owner" => {
            draw_lines_item_prompt_from_to(
                context.width_value,
                context.directory,
                blank,
                context.last_agent,
                to,
                text,
                *ts,
            );
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

fn draw_lines_character_item_prompt_from_to(
    context: &mut LineRenderContext<'_>,
    blank: &impl Fn(&mut Drawn),
    from: &str,
    to: &[String],
    text: &str,
    ts: u64,
) {
    blank(context.directory);
    let character = color_of(context.colors, from);
    let mut head = badge(from, character, ts);

    head.push(Span::styled(format!(" → {}", to.join(", ")), theme::muted()));
    context.directory.rows.push(Row::plain(Line::from(head)));
    let bar = [Span::styled("┆ ", Style::default().fg(character))];

    context.directory.rows.extend(markdown::plain(
        text.trim_end(),
        context.width_value,
        theme::muted(),
        &bar,
    ));
    *context.last_agent = None;
}

fn draw_lines_character_item_sub_agent_sub(
    context: &mut LineRenderContext<'_>,
    blank: &impl Fn(&mut Drawn),
    agent: &str,
    sub: usize,
    ts: u64,
    key: usize,
) {
    let character_5 = color_of(context.colors, agent);

    blank(context.directory);
    context
        .directory
        .rows
        .push(Row::plain(Line::from(badge(agent, character_5, ts))));

    if let Some(text_2) = context.thread.subs.get(sub) {
        sub_rows(
            context.directory,
            text_2,
            sub,
            key,
            context.fold,
            context.width_value,
            &[Span::styled("│ ", Style::default().fg(character_5))],
        );
    }

    *context.last_agent = None;
}

fn draw_lines_character_character_(
    context: LineNoteContext<'_>,
    item: &Item,
    agent: &String,
    text: &str,
    color: Color,
) {
    let LineNoteContext {
        thread,
        colors,
        roster,
        width_value,
        directory,
        last_agent,
    } = context;

    let character_7 = color_of(colors, agent);
    let pre = [
        Span::styled("● ", Style::default().fg(color)),
        Span::styled(format!("{agent} "), Style::default().fg(character_7)),
    ];

    directory
        .rows
        .extend(markdown::plain(text, width_value, Style::default().fg(color), &pre));

    if color == theme::pal().error() {
        for height_value in error_hint(agent, text, roster, resume_of(thread, item)) {
            directory.rows.extend(markdown::plain(
                &height_value,
                width_value,
                theme::muted(),
                &[Span::raw("  ")],
            ));
        }
    }

    *last_agent = None;
}

fn finish_draw_lines(
    thread: &Thread,
    colors: &HashMap<String, Color>,
    ask: &Picker,
    width_value: usize,
    mut directory: Drawn,
    blank: impl Fn(&mut Drawn),
) -> Drawn {
    if let Some(agent_5) = thread.asks.first() {
        let character_9 = color_of(colors, &agent_5.agent);

        blank(&mut directory);
        let edge = theme::fg(theme::pal().attention());

        let more = if thread.asks.len() > 1 {
            format!(" · {} more waiting", thread.asks.len() - 1)
        } else {
            String::new()
        };

        let what = if agent_5.tool == crate::engine::flows::run::WAIT_TOOL {
            "waits on you"
        } else if agent_5.questions().is_empty() {
            "asks to run"
        } else {
            "asks"
        };

        directory.rows.push(Row::plain(Line::from(vec![
            Span::styled("┃ ", edge),
            Span::styled(
                agent_5.agent.clone(),
                Style::default().fg(character_9).add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!(" {what}{more}"), edge),
        ])));
        ask_rows(&mut directory, agent_5, width_value, &[Span::styled("┃ ", edge)], ask);
    }

    directory
}

fn draw_lines_item_run(
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

    let character_2 = begin_run(layout, &blank, &divider, agent, start);
    let shown = layout.fold.run(key);
    let running = calls.iter().any(|character_3| character_3.output.is_none()) && layout.thread.busy.contains(agent);

    let mark = if shown {
        "▾ "
    } else if running {
        "◦ "
    } else {
        "▸ "
    };

    let summary = summary_with_thoughts(calls, start, end, thoughts, layout.fold);

    let fold: &Fold<'_> = layout.fold;
    let width_value: usize = layout.width_value;
    let directory: &mut Drawn = layout.directory;

    let think = fold.thoughts != Thoughts::Hidden && (shown || fold.thoughts == Thoughts::Open);
    let tbar = [Span::styled("│   ", Style::default().fg(character_2))];
    let row_at = directory.rows.len();
    let pre = [
        Span::styled("│ ", Style::default().fg(character_2)),
        Span::styled(mark, theme::accent()),
    ];
    let rest = [Span::styled("│   ", Style::default().fg(character_2))];

    for (i, result_value) in markdown::plain(&summary, width_value, theme::muted(), &pre)
        .into_iter()
        .enumerate()
    {
        let result_2 = if i == 0 {
            result_value
        } else {
            Row {
                line: Line::from([rest.to_vec(), result_value.line.spans[2..].to_vec()].concat()),
                ..result_value
            }
        };

        directory.hits.insert(row_at + i, Hit::Run(key));
        directory.rows.push(result_2);
    }

    if think && !shown {
        for count_3 in 0..=calls.len() {
            run_thought_rows(directory, thoughts, count_3, width_value, &tbar);
        }
    }

    if shown {
        draw_lines_item_run_head(width_value, directory, calls, thoughts, character_2, think, &tbar);
    }
}

fn draw_lines_item_run_head(
    width_value: usize,
    directory: &mut Drawn,
    calls: &[Call],
    thoughts: &[(usize, String)],
    character_2: Color,
    think: bool,
    tbar: &[Span<'static>; 1],
) {
    for (count_4, call) in calls.iter().enumerate() {
        if think {
            run_thought_rows(directory, thoughts, count_4, width_value, tbar);
        }

        let head = format!("{} {}", tool_label(&call.name), call_summary(&call.name, &call.input));
        let bar = [Span::styled("│   ", Style::default().fg(character_2))];

        directory
            .rows
            .extend(markdown::plain(&head, width_value, theme::text(), &bar));
        directory.rows.extend(memo::call(
            call,
            width_value,
            &[Span::styled("│     ", Style::default().fg(character_2))],
        ));
    }

    if think {
        run_thought_rows(directory, thoughts, calls.len(), width_value, tbar);
    }
}

#[path = "line_messages.rs"]
mod messages;
use messages::{draw_lines_item_prompt_from_to, draw_lines_item_settled, draw_lines_item_text};

pub(super) struct LineRenderContext<'render> {
    pub(super) thread: &'render Thread,
    pub(super) colors: &'render HashMap<String, Color>,
    pub(super) roster: &'render [(String, String)],
    pub(super) fold: &'render Fold<'render>,
    pub(super) width_value: usize,
    pub(super) directory: &'render mut Drawn,
    pub(super) last_agent: &'render mut Option<String>,
    pub(super) minute: &'render mut String,
}

pub(super) struct LineNoteContext<'render> {
    pub(super) thread: &'render Thread,
    pub(super) colors: &'render HashMap<String, Color>,
    pub(super) roster: &'render [(String, String)],
    pub(super) width_value: usize,
    pub(super) directory: &'render mut Drawn,
    pub(super) last_agent: &'render mut Option<String>,
}

pub(super) struct RunRenderContext<'render> {
    pub(super) thread: &'render Thread,
    pub(super) colors: &'render HashMap<String, Color>,
    pub(super) fold: &'render Fold<'render>,
    pub(super) width_value: usize,
    pub(super) directory: &'render mut Drawn,
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
    let character_2 = color_of(layout.colors, agent);

    if layout.last_agent.as_deref() != Some(agent) {
        blank(layout.directory);
        layout
            .directory
            .rows
            .push(Row::plain(Line::from(badge(agent, character_2, start))));
        *layout.last_agent = Some(agent.to_string());
    } else if *layout.minute != clock(start) {
        blank(layout.directory);
        divider(layout.directory, character_2, &clock(start));
    }

    *layout.minute = clock(start);

    character_2
}

fn summary_with_thoughts(calls: &[Call], start: u64, end: u64, thoughts: &[(usize, String)], fold: &Fold) -> String {
    let failed = calls
        .iter()
        .filter(|character_4| matches!(character_4.output, Some((false, _))))
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
