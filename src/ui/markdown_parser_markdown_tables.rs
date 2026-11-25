use super::{Block, MarkdownState, Modifier, Style, flush, gap, margin, table_rows};

pub(super) fn start_table(state: &mut MarkdownState<'_>, at: usize) {
    flush(state);
    gap(state, at);
    state.table = Some((Vec::new(), 0));
}

pub(super) fn start_table_row(state: &mut MarkdownState<'_>, style: Style, is_head: bool) {
    if let Some((rows, _)) = state.table.as_mut() {
        rows.push(Vec::new());
    }

    if is_head {
        state.styles.push(style.add_modifier(Modifier::BOLD));
    }
}

pub(super) fn end_table_head(state: &mut MarkdownState<'_>) {
    state.styles.pop();

    if let Some((rows, head)) = state.table.as_mut() {
        *head = rows.len();
    }
}

pub(super) fn start_table_cell(state: &mut MarkdownState<'_>) {
    state.block = Some(Block {
        pieces: Vec::new(),
        first: Vec::new(),
        rest: Vec::new(),
    });
}

pub(super) fn end_table_cell(state: &mut MarkdownState<'_>) {
    if let (Some((rows, _)), Some(block)) = (state.table.as_mut(), state.block.take())
        && let Some(row) = rows.last_mut()
    {
        row.push(block.pieces);
    }
}

pub(super) fn end_table(state: &mut MarkdownState<'_>) {
    if let Some((rows, head)) = state.table.take() {
        state.out.extend(table_rows(
            &rows,
            head,
            state.width,
            &margin(state.prefix, state.quote, state.lists.len()),
        ));
    }
}
