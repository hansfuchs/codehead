use self::messages::{BubbleAgentContext, BubbleAnswerContent, BubbleAnswerLayout};
use std::ops::ControlFlow;

use super::{
    Call, Color, Colors, Drawn, Fold, HashMap, Hit, Item, Line, Modifier, Open, Picker, Row, Span, Style, Thoughts,
    Thread, agents_on, ask_rows, bottom_border, call_summary, clock, color_of, edits_row, error_hint, kid_rows,
    kid_tag, markdown, memo, minute_divider, needs_harness, owner_bubble, resume_of, run_lines, run_thought_rows,
    shell_rows, sub_rows, theme, thought_count, thought_rows, tool_label, top_border,
};

pub(super) fn draw_bubbles(
    thread: &Thread,
    width: u16,
    colors: &Colors,
    roster: &[(String, String)],
    fold: &Fold,
    ask: &Picker,
) -> Drawn {
    let width_value = usize::from(width.max(12));

    // content width inside `│ ` … ` │`
    let inner = width_value - 2;
    let mut directory = Drawn::default();
    let mut cur: Option<Open> = None;

    let close = |directory: &mut Drawn, cur: &mut Option<Open>| close_bubble(width_value, directory, cur);

    let gap = |directory_2: &mut Drawn| {
        if !directory_2.rows.is_empty() {
            directory_2.rows.push(Row::plain(Line::raw("")));
        }
    };

    // last item of each agent at work: its box gets the wave, unless a message or an answer came
    // after it (a new turn: 0 output yet, the wave goes into a box of its own at the end)
    let last_of = last_working_items(thread);

    for (idx, item) in thread.items.iter().enumerate() {
        // an item outside the open box: the box closes first (its bottom border: its last item's)
        draw_bubbles_joins(
            BubbleRenderContext {
                thread,
                colors,
                roster,
                fold,
                width_value,
                inner,
                last_of: &last_of,
            },
            &mut directory,
            &mut cur,
            close,
            gap,
            idx,
            item,
        );
    }

    close(&mut directory, &mut cur);

    // agents at work with 0 output in this turn yet: 0 box (their header card shows the work);
    // the box comes with the 1st output (live text included)
    // the 1st open ask: a yellow box below everything, its choices clickable
    finish_draw_bubbles(
        thread,
        colors,
        ask,
        BubbleFinishLayout {
            width_value,
            inner,
            directory,
        },
        close,
        gap,
    )
}

fn draw_bubbles_branch_state(width_value: usize, output_value: &Open, row: &mut Row) {
    if let ControlFlow::Break(()) = draw_bubbles_branch(width_value, output_value, row) {}
}

fn draw_bubbles_branch(width_value: usize, output_value: &Open, row: &mut Row) -> ControlFlow<()> {
    if let ControlFlow::Break(()) = draw_bubbles_body(width_value, output_value, row) {
        return ControlFlow::Break(());
    }

    ControlFlow::Continue(())
}

fn draw_bubbles_body(width_value: usize, output_value: &Open, row: &mut Row) -> ControlFlow<()> {
    let rw = row.line.width();

    if rw >= width_value {
        return ControlFlow::Break(());
    }

    row.line
        .spans
        .push(Span::raw(" ".repeat((width_value - 1).saturating_sub(rw))));
    row.line.spans.push(Span::styled("│", output_value.border));

    // a full row (a minute divider): its own right end

    ControlFlow::Continue(())
}

fn draw_bubbles_joins(
    context: BubbleRenderContext<'_>,
    directory: &mut Drawn,
    cur: &mut Option<Open>,
    close: impl Fn(&mut Drawn, &mut Option<Open>),
    gap: impl Fn(&mut Drawn),
    idx: usize,
    item: &Item,
) {
    let joins = match item {
        Item::Text { text, thoughts, .. }
            if text.trim().is_empty() && (thoughts.is_empty() || context.fold.thoughts == Thoughts::Hidden) =>
        {
            true
        }
        Item::Text { agent, .. } | Item::Run { agent, .. } | Item::Sub { agent, .. } => {
            cur.as_ref().and_then(|output_2| output_2.agent.as_deref()) == Some(agent.as_str())
        }
        _ => false,
    };

    if !joins {
        close(directory, cur);
    }

    directory.starts.push(directory.rows.len());

    match item {
        Item::Prompt { from, to, text, ts } if from == "owner" => {
            close(directory, cur);
            gap(directory);

            let to_note = if to.len() > 1 {
                format!("→ {}", to.join(", "))
            } else {
                String::new()
            };

            owner_bubble(directory, context.width_value, text, &to_note, *ts);
        }
        Item::Settled { .. } => {
            render_bubble_settled(&context, directory, cur, &close, &gap, idx, item);
        }
        Item::Prompt { .. } => {
            render_bubble_prompt(&context, directory, cur, &close, &gap, idx, item);
        }
        // only thoughts, hidden: 0 rows
        Item::Text { text, thoughts, .. }
            if text.trim().is_empty() && (thoughts.is_empty() || context.fold.thoughts == Thoughts::Hidden) => {}
        Item::Text { .. } | Item::Run { .. } | Item::Sub { .. } => {
            render_bubble_text_run_sub(&context, directory, cur, &close, &gap, idx, item);
        }
        Item::Kid { .. } => {
            render_bubble_kid(&context, directory, cur, &close, &gap, idx, item);
        }
        Item::Shell { .. } => {
            render_bubble_shell(&context, directory, cur, &close, &gap, idx, item);
        }
        Item::Edits { .. } => {
            render_bubble_edits(&context, directory, cur, &close, &gap, idx, item);
        }
        Item::Note { .. } => {
            render_bubble_note(&context, directory, cur, &close, &gap, idx, item);
        }
    }
}

fn draw_bubbles_joins_character_(
    context: BubbleNoteContext<'_>,
    close: impl Fn(&mut Drawn, &mut Option<Open>),
    gap: impl Fn(&mut Drawn),
    item: &Item,
    agent: &String,
    text: &str,
    color: Color,
) {
    let BubbleNoteContext {
        thread,
        colors,
        roster,
        width_value,
        directory,
        cur,
    } = context;

    close(directory, cur);
    gap(directory);
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
}

fn finish_draw_bubbles(
    thread: &Thread,
    colors: &HashMap<String, Color>,
    ask: &Picker,
    context: BubbleFinishLayout,
    close: impl Fn(&mut Drawn, &mut Option<Open>),
    gap: impl Fn(&mut Drawn),
) -> Drawn {
    let BubbleFinishLayout {
        width_value,
        inner,
        mut directory,
    } = context;

    if let Some(agent_value) = thread.asks.first() {
        gap(&mut directory);
        let character_8 = color_of(colors, &agent_value.agent);
        let border = theme::fg(theme::pal().attention());

        let more = if thread.asks.len() > 1 {
            format!(" · {} more waiting", thread.asks.len() - 1)
        } else {
            String::new()
        };

        let what = if agent_value.tool == crate::engine::flows::run::WAIT_TOOL {
            "waits on you"
        } else if agent_value.questions().is_empty() {
            "asks to run"
        } else {
            "asks"
        };

        let name = vec![
            Span::styled(
                agent_value.agent.clone(),
                Style::default().fg(character_8).add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!(" {what}{more}"), border),
        ];
        let (top, _) = top_border(width_value, border, name, false, "");

        let mut cur = Some(Open {
            start: directory.rows.len(),
            border,
            agent: None,
            minute: String::new(),
            name: Vec::new(),
            wave: None,
        });

        directory.rows.push(top);
        let bar = [Span::styled("│ ", border)];

        ask_rows(&mut directory, agent_value, inner, &bar, ask);
        close(&mut directory, &mut cur);
    }

    directory
}

#[path = "bubble_messages.rs"]
mod messages;
use messages::{draw_bubbles_item_settled, draw_bubbles_item_text_agent_ts};

#[derive(Clone, Copy)]
pub(super) struct BubbleRenderContext<'render> {
    pub(super) thread: &'render Thread,
    pub(super) colors: &'render HashMap<String, Color>,
    pub(super) roster: &'render [(String, String)],
    pub(super) fold: &'render Fold<'render>,
    pub(super) width_value: usize,
    pub(super) inner: usize,
    pub(super) last_of: &'render HashMap<&'render str, usize>,
}

pub(super) struct BubbleNoteContext<'render> {
    pub(super) thread: &'render Thread,
    pub(super) colors: &'render HashMap<String, Color>,
    pub(super) roster: &'render [(String, String)],
    pub(super) width_value: usize,
    pub(super) directory: &'render mut Drawn,
    pub(super) cur: &'render mut Option<Open>,
}

pub(super) struct BubbleFinishLayout {
    pub(super) width_value: usize,
    pub(super) inner: usize,
    pub(super) directory: Drawn,
}

fn close_bubble(width_value: usize, directory_value: &mut Drawn, cur: &mut Option<Open>) {
    let Some(output_value) = cur.take() else { return };

    // 1 blank row above the bottom border (0 below the top border)
    directory_value
        .rows
        .push(Row::plain(Line::from(vec![Span::styled("│ ", output_value.border)])));

    for row in &mut directory_value.rows[output_value.start + 1..] {
        draw_bubbles_branch_state(width_value, &output_value, row);
    }

    let (bottom, wave_at) = bottom_border(
        width_value,
        output_value.border,
        output_value.name,
        output_value.wave.is_some(),
    );

    if let Some(character) = output_value.wave {
        directory_value
            .waves
            .push((directory_value.rows.len(), wave_at, character));
    }

    directory_value.rows.push(bottom);
}

fn last_working_items(thread: &Thread) -> HashMap<&str, usize> {
    let last_of: HashMap<&str, usize> = thread
        .items
        .iter()
        .enumerate()
        .filter_map(|(i, it)| match it {
            Item::Text { agent, .. } | Item::Run { agent, .. } | Item::Sub { agent, .. } => Some((agent.as_str(), i)),
            _ => None,
        })
        .collect();
    let turn_at = thread
        .items
        .iter()
        .rposition(|it| matches!(it, Item::Prompt { .. } | Item::Settled { .. }));

    last_of
        .into_iter()
        .filter(|&(_, line_value)| turn_at.is_none_or(|path| line_value > path))
        .collect()
}

#[path = "bubble_routes.rs"]
mod routes;
use routes::{
    render_bubble_edits, render_bubble_kid, render_bubble_note, render_bubble_prompt, render_bubble_settled,
    render_bubble_shell, render_bubble_text_run_sub,
};
