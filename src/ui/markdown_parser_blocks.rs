use super::{Block, HeadingLevel, Line, MarkdownState, Piece, Row, Span, UnicodeWidthStr, flush, gap, margin, theme};

pub(super) fn start_paragraph(state: &mut MarkdownState<'_>, at: usize) {
    if state.block.is_none() {
        gap(state, at);
        let margin = margin(state.prefix, state.quote, state.lists.len());

        let rest = if state.lists.is_empty() {
            margin.clone()
        } else {
            [margin.clone(), vec![Span::raw("  ")]].concat()
        };

        state.block = Some(Block {
            pieces: Vec::new(),
            first: if state.lists.is_empty() { margin } else { rest.clone() },
            rest,
        });
    }
}

pub(super) fn start_heading(state: &mut MarkdownState<'_>, level: HeadingLevel, at: usize) {
    gap(state, at);
    state.heading = true;

    let mark = match level {
        HeadingLevel::H1 => "# ",
        HeadingLevel::H2 => "## ",
        _ => "### ",
    };

    let margin = margin(state.prefix, state.quote, 0);

    state.block = Some(Block {
        pieces: vec![Piece {
            text: mark.into(),
            style: theme::muted(),
            ..Default::default()
        }],
        first: margin.clone(),
        rest: margin,
    });
    state.styles.push(theme::bold());
}

pub(super) fn end_heading(state: &mut MarkdownState<'_>) {
    state.heading = false;
    state.styles.pop();
    flush(state);
}

pub(super) fn start_quote(state: &mut MarkdownState<'_>) {
    flush(state);
    state.quote += 1;
}

pub(super) fn end_quote(state: &mut MarkdownState<'_>) {
    flush(state);
    state.quote = state.quote.saturating_sub(1);
}

pub(super) fn start_list(state: &mut MarkdownState<'_>, start: Option<u64>, at: usize) {
    flush(state);

    if state.lists.is_empty() {
        gap(state, at);
    }

    state.lists.push(start);
}

pub(super) fn end_list(state: &mut MarkdownState<'_>) {
    flush(state);
    state.lists.pop();
}

pub(super) fn start_item(state: &mut MarkdownState<'_>) {
    flush(state);
    let margin = margin(state.prefix, state.quote, state.lists.len());

    let mark = match state.lists.last_mut() {
        Some(Some(number)) => {
            let mark = format!("{number}. ");

            *number += 1;
            mark
        }
        _ => "• ".into(),
    };

    let rest = [margin.clone(), vec![Span::raw(" ".repeat(mark.width()))]].concat();
    let first = [margin, vec![Span::styled(mark, theme::muted())]].concat();

    state.block = Some(Block {
        pieces: Vec::new(),
        first,
        rest,
    });
}

pub(super) fn end_item(state: &mut MarkdownState<'_>) {
    match state.block.take() {
        Some(block) if block.pieces.is_empty() => state.out.push(Row::plain(Line::from(block.first))),
        block => {
            state.block = block;
            flush(state);
        }
    }
}

pub(super) fn task_marker(state: &mut MarkdownState<'_>, done: bool) {
    if let Some(block) = state.block.as_mut() {
        block.pieces.push(Piece {
            text: if done { "☑ ".into() } else { "☐ ".into() },
            style: theme::muted(),
            ..Default::default()
        });
    }
}

pub(super) fn rule(state: &mut MarkdownState<'_>) {
    flush(state);
    let margin = margin(state.prefix, state.quote, 0);
    let width = state
        .width
        .saturating_sub(margin.iter().map(|span| span.content.width()).sum());

    state.out.push(Row::plain(Line::from(
        [margin, vec![Span::styled("─".repeat(width.min(40)), theme::line())]].concat(),
    )));
}

pub(super) fn start_html(state: &mut MarkdownState<'_>, at: usize) {
    flush(state);
    gap(state, at);
}
