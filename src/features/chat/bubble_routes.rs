use super::{
    BubbleAgentContext, BubbleAnswerContent, BubbleAnswerLayout, BubbleNoteContext, BubbleRenderContext, Drawn, Item,
    Modifier, Open, Span, Style, clock, color_of, draw_bubbles_item_settled, draw_bubbles_item_text_agent_ts,
    draw_bubbles_joins_character_, edits_row, kid_rows, kid_tag, markdown, shell_rows, theme, top_border,
};

pub(super) fn render_bubble_settled(
    context: &BubbleRenderContext<'_>,
    directory: &mut Drawn,
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

    draw_bubbles_item_settled(
        BubbleAnswerLayout {
            colors: context.colors,
            width_value: context.width_value,
            inner: context.inner,
            directory,
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
    directory: &mut Drawn,
    cur: &mut Option<Open>,
    close: &impl Fn(&mut Drawn, &mut Option<Open>),
    gap: &impl Fn(&mut Drawn),
    _idx: usize,
    item: &Item,
) {
    let Item::Prompt { from, to, text, ts } = item else {
        return;
    };

    close(directory, cur);
    gap(directory);
    let character_2 = color_of(context.colors, from);
    let border = Style::default().fg(theme::desaturate(character_2, 0.4));
    let name = vec![
        Span::styled(
            from.clone(),
            Style::default().fg(character_2).add_modifier(Modifier::BOLD),
        ),
        Span::styled(format!(" → {}", to.join(", ")), theme::muted()),
    ];
    let (top, _) = top_border(context.width_value, border, Vec::new(), false, &clock(*ts));

    *cur = Some(Open {
        start: directory.rows.len(),
        border,
        agent: None,
        minute: String::new(),
        name,
        wave: None,
    });
    directory.rows.push(top);
    let bar = [Span::styled("│ ", border)];

    directory
        .rows
        .extend(markdown::plain(text.trim_end(), context.inner, theme::muted(), &bar));
    close(directory, cur);
}
pub(super) fn render_bubble_text_run_sub(
    context: &BubbleRenderContext<'_>,
    directory: &mut Drawn,
    cur: &mut Option<Open>,
    close: &impl Fn(&mut Drawn, &mut Option<Open>),
    gap: &impl Fn(&mut Drawn),
    idx: usize,
    item: &Item,
) {
    let (Item::Text { agent, ts, .. } | Item::Run { agent, start: ts, .. } | Item::Sub { agent, ts, .. }) = item else {
        return;
    };

    draw_bubbles_item_text_agent_ts(
        BubbleAgentContext {
            thread: context.thread,
            colors: context.colors,
            roster: context.roster,
            fold: context.fold,
            width_value: context.width_value,
            inner: context.inner,
            directory,
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
    directory: &mut Drawn,
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
    close(directory, cur);
    gap(directory);
    let character_6 = color_of(context.colors, agent);
    let border = Style::default().fg(theme::desaturate(character_6, 0.4));
    let mut name = vec![Span::styled(
        agent.clone(),
        Style::default().fg(character_6).add_modifier(Modifier::BOLD),
    )];

    name.extend(kid_tag(agent, state));
    let (top, _) = top_border(context.width_value, border, Vec::new(), false, &clock(*ts));

    *cur = Some(Open {
        start: directory.rows.len(),
        border,
        agent: None,
        minute: String::new(),
        name,
        wave: None,
    });
    directory.rows.push(top);
    let bar = [Span::styled("│ ", border)];

    directory.rows.extend(kid_rows(title, state, text, context.inner, &bar));
    close(directory, cur);
}
pub(super) fn render_bubble_shell(
    context: &BubbleRenderContext<'_>,
    directory: &mut Drawn,
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

    close(directory, cur);
    gap(directory);
    directory.rows.extend(shell_rows(
        command,
        output,
        *code,
        *ts,
        dir.as_deref(),
        context.width_value,
    ));
}
pub(super) fn render_bubble_edits(
    context: &BubbleRenderContext<'_>,
    directory: &mut Drawn,
    cur: &mut Option<Open>,
    close: &impl Fn(&mut Drawn, &mut Option<Open>),
    gap: &impl Fn(&mut Drawn),
    _idx: usize,
    item: &Item,
) {
    let Item::Edits { agent, text } = item else { return };

    close(directory, cur);
    gap(directory);
    directory.rows.push(edits_row(
        agent,
        text,
        color_of(context.colors, agent),
        context.width_value,
    ));
}
pub(super) fn render_bubble_note(
    context: &BubbleRenderContext<'_>,
    directory: &mut Drawn,
    cur: &mut Option<Open>,
    close: &impl Fn(&mut Drawn, &mut Option<Open>),
    gap: &impl Fn(&mut Drawn),
    _idx: usize,
    item: &Item,
) {
    let Item::Note { agent, text, color } = item else {
        return;
    };

    draw_bubbles_joins_character_(
        BubbleNoteContext {
            thread: context.thread,
            colors: context.colors,
            roster: context.roster,
            width_value: context.width_value,
            directory,
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
