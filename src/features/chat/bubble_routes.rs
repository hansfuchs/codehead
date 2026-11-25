use super::{
    BubbleAgentContext, BubbleAnswerContent, BubbleAnswerLayout, BubbleNoteContext, BubbleRenderContext, Drawn, Item,
    Modifier, Open, Span, Style, clock, color_of, draw_agent_bubble, draw_note_bubble, draw_settled_bubble, edits_row,
    kid_rows, kid_tag, markdown, shell_rows, theme, top_border,
};

pub(super) fn render_bubble_settled(
    context: &BubbleRenderContext<'_>,
    drawn: &mut Drawn,
    cur: &mut Option<Open>,
    close: &impl Fn(&mut Drawn, &mut Option<Open>),
    gap: &impl Fn(&mut Drawn),
    _idx: usize,
    item: &Item,
) {
    let Item::Settled {
        agent,
        question,
        answer,
        allow,
        permission,
        asked,
        ts,
    } = item
    else {
        return;
    };

    draw_settled_bubble(
        BubbleAnswerLayout {
            colors: context.colors,
            width: context.width,
            inner: context.inner,
            drawn,
            cur,
        },
        close,
        gap,
        BubbleAnswerContent {
            agent,
            question,
            answer,
            allow: *allow,
            permission: *permission,
            asked: *asked,
            ts: *ts,
        },
    );
}
pub(super) fn render_bubble_prompt(
    context: &BubbleRenderContext<'_>,
    drawn: &mut Drawn,
    cur: &mut Option<Open>,
    close: &impl Fn(&mut Drawn, &mut Option<Open>),
    gap: &impl Fn(&mut Drawn),
    _idx: usize,
    item: &Item,
) {
    let Item::Prompt { from, to, text, ts } = item else {
        return;
    };

    close(drawn, cur);
    gap(drawn);
    let color = color_of(context.colors, from);
    let border = Style::default().fg(theme::desaturate(color, 0.4));
    let name = vec![
        Span::styled(from.clone(), Style::default().fg(color).add_modifier(Modifier::BOLD)),
        Span::styled(format!(" → {}", to.join(", ")), theme::muted()),
    ];
    let (top, _) = top_border(context.width, border, Vec::new(), false, &clock(*ts));

    *cur = Some(Open {
        start: drawn.rows.len(),
        border,
        agent: None,
        minute: String::new(),
        name,
        wave: None,
    });
    drawn.rows.push(top);
    let bar = [Span::styled("│ ", border)];

    drawn
        .rows
        .extend(markdown::plain(text.trim_end(), context.inner, theme::muted(), &bar));
    close(drawn, cur);
}
pub(super) fn render_bubble_text_run_sub(
    context: &BubbleRenderContext<'_>,
    drawn: &mut Drawn,
    cur: &mut Option<Open>,
    close: &impl Fn(&mut Drawn, &mut Option<Open>),
    gap: &impl Fn(&mut Drawn),
    idx: usize,
    item: &Item,
) {
    let (Item::Text { agent, ts, .. } | Item::Run { agent, start: ts, .. } | Item::Sub { agent, ts, .. }) = item else {
        return;
    };

    draw_agent_bubble(
        BubbleAgentContext {
            thread: context.thread,
            colors: context.colors,
            roster: context.roster,
            fold: context.fold,
            width: context.width,
            inner: context.inner,
            drawn,
            cur,
            last_of: context.last_of,
            idx,
        },
        close,
        gap,
        item,
        agent,
        *ts,
    );
}
pub(super) fn render_bubble_kid(
    context: &BubbleRenderContext<'_>,
    drawn: &mut Drawn,
    cur: &mut Option<Open>,
    close: &impl Fn(&mut Drawn, &mut Option<Open>),
    gap: &impl Fn(&mut Drawn),
    _idx: usize,
    item: &Item,
) {
    let Item::Kid {
        agent,
        title,
        state,
        text,
        ts,
    } = item
    else {
        return;
    };

    // a box of its own (0 merge with the agent's own messages)
    close(drawn, cur);
    gap(drawn);
    let color = color_of(context.colors, agent);
    let border = Style::default().fg(theme::desaturate(color, 0.4));
    let mut name = vec![Span::styled(
        agent.clone(),
        Style::default().fg(color).add_modifier(Modifier::BOLD),
    )];

    name.extend(kid_tag(agent, state));
    let (top, _) = top_border(context.width, border, Vec::new(), false, &clock(*ts));

    *cur = Some(Open {
        start: drawn.rows.len(),
        border,
        agent: None,
        minute: String::new(),
        name,
        wave: None,
    });
    drawn.rows.push(top);
    let bar = [Span::styled("│ ", border)];

    drawn.rows.extend(kid_rows(title, state, text, context.inner, &bar));
    close(drawn, cur);
}
pub(super) fn render_bubble_shell(
    context: &BubbleRenderContext<'_>,
    drawn: &mut Drawn,
    cur: &mut Option<Open>,
    close: &impl Fn(&mut Drawn, &mut Option<Open>),
    gap: &impl Fn(&mut Drawn),
    _idx: usize,
    item: &Item,
) {
    let Item::Shell {
        command,
        output,
        code,
        ts,
        dir,
    } = item
    else {
        return;
    };

    close(drawn, cur);
    gap(drawn);
    drawn
        .rows
        .extend(shell_rows(command, output, *code, *ts, dir.as_deref(), context.width));
}
pub(super) fn render_bubble_edits(
    context: &BubbleRenderContext<'_>,
    drawn: &mut Drawn,
    cur: &mut Option<Open>,
    close: &impl Fn(&mut Drawn, &mut Option<Open>),
    gap: &impl Fn(&mut Drawn),
    _idx: usize,
    item: &Item,
) {
    let Item::Edits { agent, text } = item else { return };

    close(drawn, cur);
    gap(drawn);
    drawn
        .rows
        .push(edits_row(agent, text, color_of(context.colors, agent), context.width));
}
pub(super) fn render_bubble_note(
    context: &BubbleRenderContext<'_>,
    drawn: &mut Drawn,
    cur: &mut Option<Open>,
    close: &impl Fn(&mut Drawn, &mut Option<Open>),
    gap: &impl Fn(&mut Drawn),
    _idx: usize,
    item: &Item,
) {
    let Item::Note { agent, text, color } = item else {
        return;
    };

    draw_note_bubble(
        BubbleNoteContext {
            thread: context.thread,
            colors: context.colors,
            roster: context.roster,
            width: context.width,
            drawn,
            cur,
        },
        close,
        gap,
        item,
        agent,
        text,
        *color,
    );
}
