use super::{
    AnswerContent, Color, Drawn, Item, Line, LineMessageContext, LineNoteContext, LineRenderContext, Row, RunContent,
    RunRenderContext, Span, Style, TextContent, badge, color_of, draw_lines_character_character_,
    draw_lines_character_item_prompt_from_to, draw_lines_character_item_sub_agent_sub, draw_lines_item_run,
    draw_lines_item_settled, draw_lines_item_text, edits_row, kid_rows, kid_tag, shell_rows,
};

pub(super) fn render_line_prompt(
    context: &mut LineRenderContext<'_>,
    blank: &impl Fn(&mut Drawn),
    _divider: &impl Fn(&mut Drawn, Color, &str),
    item: &Item,
) {
    let Item::Prompt { from, to, text, ts } = item else {
        return;
    };

    draw_lines_character_item_prompt_from_to(context, &blank, from, to, text, *ts);
}
pub(super) fn render_line_text(
    context: &mut LineRenderContext<'_>,
    blank: &impl Fn(&mut Drawn),
    divider: &impl Fn(&mut Drawn, Color, &str),
    item: &Item,
) {
    let Item::Text {
        agent,
        text,
        ts,
        thoughts,
        key,
    } = item
    else {
        return;
    };

    draw_lines_item_text(
        LineMessageContext {
            colors: context.colors,
            roster: context.roster,
            fold: context.fold,
            width_value: context.width_value,
            directory: context.directory,
            last_agent: context.last_agent,
            minute: context.minute,
        },
        blank,
        divider,
        TextContent {
            agent,
            text,
            ts: *ts,
            thoughts,
            key: *key,
        },
    );
}
pub(super) fn render_line_run(
    context: &mut LineRenderContext<'_>,
    blank: &impl Fn(&mut Drawn),
    divider: &impl Fn(&mut Drawn, Color, &str),
    item: &Item,
) {
    let Item::Run {
        agent,
        key,
        calls,
        start,
        end,
        thoughts,
    } = item
    else {
        return;
    };

    draw_lines_item_run(
        &mut (RunRenderContext {
            thread: context.thread,
            colors: context.colors,
            fold: context.fold,
            width_value: context.width_value,
            directory: context.directory,
            last_agent: context.last_agent,
            minute: context.minute,
        }),
        blank,
        divider,
        RunContent {
            agent,
            key: *key,
            calls,
            start: *start,
            end: *end,
            thoughts,
        },
    );
}
pub(super) fn render_line_sub(
    context: &mut LineRenderContext<'_>,
    blank: &impl Fn(&mut Drawn),
    _divider: &impl Fn(&mut Drawn, Color, &str),
    item: &Item,
) {
    let Item::Sub { agent, sub, ts, key } = item else {
        return;
    };

    draw_lines_character_item_sub_agent_sub(context, &blank, agent, *sub, *ts, *key);
}
pub(super) fn render_line_kid(
    context: &mut LineRenderContext<'_>,
    blank: &impl Fn(&mut Drawn),
    _divider: &impl Fn(&mut Drawn, Color, &str),
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

    let character_6 = color_of(context.colors, agent);

    blank(context.directory);
    let mut head = badge(agent, character_6, *ts);

    head.splice(1..1, kid_tag(agent, state));
    context.directory.rows.push(Row::plain(Line::from(head)));
    context.directory.rows.extend(kid_rows(
        title,
        state,
        text,
        context.width_value,
        &[Span::styled("│ ", Style::default().fg(character_6))],
    ));
    *context.last_agent = None;
}
pub(super) fn render_line_shell(
    context: &mut LineRenderContext<'_>,
    blank: &impl Fn(&mut Drawn),
    _divider: &impl Fn(&mut Drawn, Color, &str),
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

    blank(context.directory);
    context.directory.rows.extend(shell_rows(
        command,
        output,
        *code,
        *ts,
        dir.as_deref(),
        context.width_value,
    ));
    *context.last_agent = None;
}
pub(super) fn render_line_note(
    context: &mut LineRenderContext<'_>,
    _blank: &impl Fn(&mut Drawn),
    _divider: &impl Fn(&mut Drawn, Color, &str),
    item: &Item,
) {
    let Item::Note { agent, text, color } = item else {
        return;
    };

    draw_lines_character_character_(
        LineNoteContext {
            thread: context.thread,
            colors: context.colors,
            roster: context.roster,
            width_value: context.width_value,
            directory: context.directory,
            last_agent: context.last_agent,
        },
        item,
        agent,
        text,
        *color,
    );
}
pub(super) fn render_line_edits(
    context: &mut LineRenderContext<'_>,
    _blank: &impl Fn(&mut Drawn),
    _divider: &impl Fn(&mut Drawn, Color, &str),
    item: &Item,
) {
    let Item::Edits { agent, text } = item else { return };

    context.directory.rows.push(edits_row(
        agent,
        text,
        color_of(context.colors, agent),
        context.width_value,
    ));
    *context.last_agent = None;
}
pub(super) fn render_line_settled(
    context: &mut LineRenderContext<'_>,
    _blank: &impl Fn(&mut Drawn),
    _divider: &impl Fn(&mut Drawn, Color, &str),
    item: &Item,
) {
    let Item::Settled {
        agent,
        question,
        answer,
        allow,
        permission,
        ..
    } = item
    else {
        return;
    };

    draw_lines_item_settled(
        context.colors,
        context.width_value,
        context.directory,
        context.last_agent,
        AnswerContent {
            agent,
            question,
            answer,
            allow: *allow,
            permission: *permission,
        },
    );
}
