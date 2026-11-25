use super::{Line, Piece, Row, Span, UnicodeWidthStr, theme, wrap};

/// A Markdown table: columns as wide as their widest cell, shrunk to fit WIDTH (cells wrap),
/// ` │ ` between columns, a `─┼─` line below the head rows.
pub(super) fn table_rows(rows: &[Vec<Vec<Piece>>], head: usize, width: usize, margin: &[Span<'static>]) -> Vec<Row> {
    let cols = rows.iter().map(Vec::len).max().unwrap_or(0);

    if cols == 0 {
        return Vec::new();
    }

    let mw: usize = margin.iter().map(|text_value| text_value.content.width()).sum();
    let room = width.saturating_sub(mw + 3 * (cols - 1)).max(cols);
    let cell_w = |character: &[Piece]| character.iter().map(|path| path.text.width()).sum::<usize>();

    let natural: Vec<usize> = (0..cols)
        .map(|i| {
            rows.iter()
                .filter_map(|result| result.get(i))
                .map(|character_value| cell_w(character_value))
                .max()
                .unwrap_or(0)
                .max(1)
        })
        .collect();

    // narrow columns keep their width, the others share the room left
    let mut width_value = natural.clone();

    if natural.iter().sum::<usize>() > room {
        table_rows_left(cols, room, &(natural), &mut width_value);
    }

    let sep = Span::styled(" │ ", theme::line());
    let out = Vec::new();

    draw_table_rows(
        rows,
        head,
        out,
        TableLayout {
            margin,
            cols,
            mw,
            width_value,
            sep,
        },
    )
}

fn table_rows_left(cols: usize, room: usize, natural: &[usize], width_value: &mut [usize]) {
    let mut left = room;
    let mut open: Vec<usize> = (0..cols).collect();

    loop {
        let share = left / open.len().max(1);
        let (fit, wide): (Vec<usize>, Vec<usize>) = open.iter().partition(|&&i| natural[i] <= share);

        if fit.is_empty() {
            spread_widths(&wide, width_value, share, left);

            break;
        }

        for &i in &fit {
            width_value[i] = natural[i];
            left -= natural[i];
        }

        open = wide;

        if open.is_empty() {
            break;
        }
    }
}

fn draw_table_rows(rows: &[Vec<Vec<Piece>>], head: usize, mut out: Vec<Row>, settings: TableLayout<'_>) -> Vec<Row> {
    let TableLayout {
        margin,
        cols,
        mw,
        width_value,
        sep,
    } = settings;

    for (ri, row_value) in rows.iter().enumerate() {
        draw_table_row(
            head,
            &mut out,
            ri,
            row_value,
            TableRowLayout {
                margin,
                cols,
                mw,
                width_value: &width_value,
                sep: &sep,
            },
        );
    }

    out
}

fn draw_table_row(head: usize, out: &mut Vec<Row>, ri: usize, row_value: &[Vec<Piece>], settings: TableRowLayout<'_>) {
    let TableRowLayout {
        margin,
        cols,
        mw,
        width_value,
        sep,
    } = settings;

    let cells: Vec<Vec<Row>> = (0..cols)
        .map(|i| {
            row_value
                .get(i)
                .map_or_else(Vec::new, |character_2| wrap(character_2, width_value[i], &[], &[]))
        })
        .collect();

    let height = cells.iter().map(Vec::len).max().unwrap_or(1).max(1);

    for key_value in 0..height {
        draw_table_cells(
            out,
            &cells,
            key_value,
            TableCellLayout {
                margin,
                cols,
                mw,
                width_value,
                sep,
            },
        );
    }

    if ri + 1 == head {
        let line: Vec<String> = width_value.iter().map(|count| "─".repeat(*count)).collect();

        out.push(Row::plain(Line::from(
            [margin.to_vec(), vec![Span::styled(line.join("─┼─"), theme::line())]].concat(),
        )));
    }
}

fn draw_table_cells(out: &mut Vec<Row>, cells: &[Vec<Row>], key_value: usize, settings: TableCellLayout<'_>) {
    let TableCellLayout {
        margin,
        cols,
        mw,
        width_value,
        sep,
    } = settings;

    let mut spans = margin.to_vec();
    let mut row = Row::default();
    let mut x = mw;

    for i in 0..cols {
        if i > 0 {
            spans.push(sep.clone());
            x += 3;
        }

        let cell = cells[i].get(key_value);
        let cw = cell.map_or(0, |character_3| character_3.line.width());

        if let Some(character_4) = cell {
            append_cell(&mut row, &mut spans, character_4, x);
        }

        if i + 1 < cols {
            spans.push(Span::raw(" ".repeat(width_value[i].saturating_sub(cw))));
        }

        x += width_value[i];
    }

    row.line = Line::from(spans);
    out.push(row);
}

struct TableLayout<'context> {
    margin: &'context [Span<'static>],
    cols: usize,
    mw: usize,
    width_value: Vec<usize>,
    sep: Span<'static>,
}

#[derive(Clone, Copy)]
struct TableRowLayout<'context> {
    margin: &'context [Span<'static>],
    cols: usize,
    mw: usize,
    width_value: &'context [usize],
    sep: &'context Span<'static>,
}

#[derive(Clone, Copy)]
struct TableCellLayout<'context> {
    margin: &'context [Span<'static>],
    cols: usize,
    mw: usize,
    width_value: &'context [usize],
    sep: &'context Span<'static>,
}

fn spread_widths(wide: &[usize], width_value: &mut [usize], share: usize, left: usize) {
    for (key, &i) in wide.iter().enumerate() {
        width_value[i] = (share + usize::from(key < left % wide.len().max(1))).max(1);
    }
}

fn append_cell(row: &mut Row, spans: &mut Vec<Span<'static>>, character_4: &Row, x: usize) {
    spans.extend(character_4.line.spans.iter().cloned());
    row.links.extend(
        character_4
            .links
            .iter()
            .map(|(agent, bytes, user)| (agent + x as u16, bytes + x as u16, user.clone())),
    );

    row.copies
        .extend(character_4.copies.iter().map(|(agent_value, bytes_value, user_value)| {
            (agent_value + x as u16, bytes_value + x as u16, user_value.clone())
        }));
}
