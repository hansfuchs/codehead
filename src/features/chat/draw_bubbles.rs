use self::messages::{BubbleAgentContext, BubbleAnswerContent, BubbleAnswerLayout};

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
    let width = usize::from(width.max(12));

    // content width inside `│ ` … ` │`
    let inner = width - 2;
    let mut drawn = Drawn::default();
    let mut cur: Option<Open> = None;

    let close = |drawn: &mut Drawn, cur: &mut Option<Open>| close_bubble(width, drawn, cur);

    let gap = |drawn: &mut Drawn| {
        if !drawn.rows.is_empty() {
            drawn.rows.push(Row::plain(Line::raw("")));
        }
    };

    // last item of each agent at work: its box gets the wave, unless a message or an answer came
    // after it (a new turn: 0 output yet, the wave goes into a box of its own at the end)
    let last_of = last_working_items(thread);

    for (idx, item) in thread.items.iter().enumerate() {
        // an item outside the open box: the box closes first (its bottom border: its last item's)
        draw_bubble_item(
            BubbleRenderContext {
                thread,
                colors,
                roster,
                fold,
                width,
                inner,
                last_of: &last_of,
            },
            &mut drawn,
            &mut cur,
            close,
            gap,
            idx,
            item,
        );
    }

    close(&mut drawn, &mut cur);

    // agents at work with 0 output in this turn yet: 0 box (their header card shows the work);
    // the box comes with the 1st output (live text included)
    // the 1st open ask: a yellow box below everything, its choices clickable
    finish_draw_bubbles(
        thread,
        colors,
        ask,
        BubbleFinishLayout { width, inner, drawn },
        close,
        gap,
    )
}

fn close_row(width: usize, open: &Open, row: &mut Row) {
    let row_width = row.line.width();

    // a full row (a minute divider): its own right end
    if row_width >= width {
        return;
    }

    row.line
        .spans
        .push(Span::raw(" ".repeat((width - 1).saturating_sub(row_width))));
    row.line.spans.push(Span::styled("│", open.border));
}

fn draw_bubble_item(
    context: BubbleRenderContext<'_>,
    drawn: &mut Drawn,
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
            cur.as_ref().and_then(|open| open.agent.as_deref()) == Some(agent.as_str())
        }
        _ => false,
    };

    if !joins {
        close(drawn, cur);
    }

    drawn.starts.push(drawn.rows.len());

    match item {
        Item::Prompt { from, to, text, ts } if from == "owner" => {
            close(drawn, cur);
            gap(drawn);

            let to_note = if to.len() > 1 {
                format!("→ {}", to.join(", "))
            } else {
                String::new()
            };

            owner_bubble(drawn, context.width, text, &to_note, *ts);
        }
        Item::Settled { .. } => {
            render_bubble_settled(&context, drawn, cur, &close, &gap, idx, item);
        }
        Item::Prompt { .. } => {
            render_bubble_prompt(&context, drawn, cur, &close, &gap, idx, item);
        }
        // only thoughts, hidden: 0 rows
        Item::Text { text, thoughts, .. }
            if text.trim().is_empty() && (thoughts.is_empty() || context.fold.thoughts == Thoughts::Hidden) => {}
        Item::Text { .. } | Item::Run { .. } | Item::Sub { .. } => {
            render_bubble_text_run_sub(&context, drawn, cur, &close, &gap, idx, item);
        }
        Item::Kid { .. } => {
            render_bubble_kid(&context, drawn, cur, &close, &gap, idx, item);
        }
        Item::Shell { .. } => {
            render_bubble_shell(&context, drawn, cur, &close, &gap, idx, item);
        }
        Item::Edits { .. } => {
            render_bubble_edits(&context, drawn, cur, &close, &gap, idx, item);
        }
        Item::Note { .. } => {
            render_bubble_note(&context, drawn, cur, &close, &gap, idx, item);
        }
    }
}

fn draw_note_bubble(
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
        width,
        drawn,
        cur,
    } = context;

    close(drawn, cur);
    gap(drawn);
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
        width,
        inner,
        mut drawn,
    } = context;

    if let Some(open_ask) = thread.asks.first() {
        gap(&mut drawn);
        let agent_color = color_of(colors, &open_ask.agent);
        let border = theme::fg(theme::pal().attention());

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

        let name = vec![
            Span::styled(
                open_ask.agent.clone(),
                Style::default().fg(agent_color).add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!(" {what}{more}"), border),
        ];
        let (top, _) = top_border(width, border, name, false, "");

        let mut cur = Some(Open {
            start: drawn.rows.len(),
            border,
            agent: None,
            minute: String::new(),
            name: Vec::new(),
            wave: None,
        });

        drawn.rows.push(top);
        let bar = [Span::styled("│ ", border)];

        ask_rows(&mut drawn, open_ask, inner, &bar, ask);
        close(&mut drawn, &mut cur);
    }

    drawn
}

#[path = "bubble_messages.rs"]
mod messages;
use messages::{draw_agent_bubble, draw_settled_bubble};

#[derive(Clone, Copy)]
pub(super) struct BubbleRenderContext<'render> {
    pub(super) thread: &'render Thread,
    pub(super) colors: &'render HashMap<String, Color>,
    pub(super) roster: &'render [(String, String)],
    pub(super) fold: &'render Fold<'render>,
    pub(super) width: usize,
    pub(super) inner: usize,
    pub(super) last_of: &'render HashMap<&'render str, usize>,
}

pub(super) struct BubbleNoteContext<'render> {
    pub(super) thread: &'render Thread,
    pub(super) colors: &'render HashMap<String, Color>,
    pub(super) roster: &'render [(String, String)],
    pub(super) width: usize,
    pub(super) drawn: &'render mut Drawn,
    pub(super) cur: &'render mut Option<Open>,
}

pub(super) struct BubbleFinishLayout {
    pub(super) width: usize,
    pub(super) inner: usize,
    pub(super) drawn: Drawn,
}

fn close_bubble(width: usize, drawn: &mut Drawn, cur: &mut Option<Open>) {
    let Some(open) = cur.take() else { return };

    // 1 blank row above the bottom border (0 below the top border)
    drawn
        .rows
        .push(Row::plain(Line::from(vec![Span::styled("│ ", open.border)])));

    for row in &mut drawn.rows[open.start + 1..] {
        close_row(width, &open, row);
    }

    let (bottom, wave_at) = bottom_border(width, open.border, open.name, open.wave.is_some());

    if let Some(color) = open.wave {
        drawn.waves.push((drawn.rows.len(), wave_at, color));
    }

    drawn.rows.push(bottom);
}

fn last_working_items(thread: &Thread) -> HashMap<&str, usize> {
    let last_of: HashMap<&str, usize> = thread
        .items
        .iter()
        .enumerate()
        .filter_map(|(i, item)| match item {
            Item::Text { agent, .. } | Item::Run { agent, .. } | Item::Sub { agent, .. } => Some((agent.as_str(), i)),
            _ => None,
        })
        .collect();
    let turn_at = thread
        .items
        .iter()
        .rposition(|item| matches!(item, Item::Prompt { .. } | Item::Settled { .. }));

    last_of
        .into_iter()
        .filter(|&(_, last)| turn_at.is_none_or(|turn| last > turn))
        .collect()
}

#[path = "bubble_routes.rs"]
mod routes;
use routes::{
    render_bubble_edits, render_bubble_kid, render_bubble_note, render_bubble_prompt, render_bubble_settled,
    render_bubble_shell, render_bubble_text_run_sub,
};
