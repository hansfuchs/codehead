use super::{Line, Piece, Row, Span, UnicodeWidthStr, theme, wrap};

/// A Markdown table: columns as wide as their widest cell, shrunk to fit WIDTH (cells wrap),
/// ` │ ` between columns, a `─┼─` line below the head rows.
pub(super) fn table_rows(rows: &[Vec<Vec<Piece>>], head: usize, width: usize, margin: &[Span<'static>]) -> Vec<Row> {
    let cols = rows.iter().map(Vec::len).max().unwrap_or(0);

    if cols == 0 {
        return Vec::new();
    }

    let mw: usize = margin.iter().map(|span| span.content.width()).sum();
    let room = width.saturating_sub(mw + 3 * (cols - 1)).max(cols);
    let cell_w = |cell: &[Piece]| cell.iter().map(|piece| piece.text.width()).sum::<usize>();

    let natural: Vec<usize> = (0..cols)
        .map(|i| {
            rows.iter()
                .filter_map(|row| row.get(i))
                .map(|cell| cell_w(cell))
                .max()
                .unwrap_or(0)
                .max(1)
        })
        .collect();

    // narrow columns keep their width, the others share the room left
    let mut widths = natural.clone();

    if natural.iter().sum::<usize>() > room {
        table_rows_left(cols, room, &(natural), &mut widths);
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
            widths,
            sep,
        },
    )
}

fn table_rows_left(cols: usize, room: usize, natural: &[usize], widths: &mut [usize]) {
    let mut left = room;
    let mut open: Vec<usize> = (0..cols).collect();

    loop {
        let share = left / open.len().max(1);
        let (fit, wide): (Vec<usize>, Vec<usize>) = open.iter().partition(|&&i| natural[i] <= share);

        if fit.is_empty() {
            spread_widths(&wide, widths, share, left);

            break;
        }

        for &i in &fit {
            widths[i] = natural[i];
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
        widths,
        sep,
    } = settings;

    for (ri, row_cells) in rows.iter().enumerate() {
        draw_table_row(
            head,
            &mut out,
            ri,
            row_cells,
            TableRowLayout {
                margin,
                cols,
                mw,
                widths: &widths,
                sep: &sep,
            },
        );
    }

    out
}

fn draw_table_row(head: usize, out: &mut Vec<Row>, ri: usize, row_cells: &[Vec<Piece>], settings: TableRowLayout<'_>) {
    let TableRowLayout {
        margin,
        cols,
        mw,
        widths,
        sep,
    } = settings;

    let cells: Vec<Vec<Row>> = (0..cols)
        .map(|i| {
            row_cells
                .get(i)
                .map_or_else(Vec::new, |cell| wrap(cell, widths[i], &[], &[]))
        })
        .collect();

    let height = cells.iter().map(Vec::len).max().unwrap_or(1).max(1);

    for line_index in 0..height {
        draw_table_cells(
            out,
            &cells,
            line_index,
            TableCellLayout {
                margin,
                cols,
                mw,
                widths,
                sep,
            },
        );
    }

    if ri + 1 == head {
        let line: Vec<String> = widths.iter().map(|width| "─".repeat(*width)).collect();

        out.push(Row::plain(Line::from(
            [margin.to_vec(), vec![Span::styled(line.join("─┼─"), theme::line())]].concat(),
        )));
    }
}

fn draw_table_cells(out: &mut Vec<Row>, cells: &[Vec<Row>], line_index: usize, settings: TableCellLayout<'_>) {
    let TableCellLayout {
        margin,
        cols,
        mw,
        widths,
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

        let cell = cells[i].get(line_index);
        let cw = cell.map_or(0, |cell| cell.line.width());

        if let Some(cell) = cell {
            append_cell(&mut row, &mut spans, cell, x);
        }

        if i + 1 < cols {
            spans.push(Span::raw(" ".repeat(widths[i].saturating_sub(cw))));
        }

        x += widths[i];
    }

    row.line = Line::from(spans);
    out.push(row);
}

struct TableLayout<'context> {
    margin: &'context [Span<'static>],
    cols: usize,
    mw: usize,
    widths: Vec<usize>,
    sep: Span<'static>,
}

#[derive(Clone, Copy)]
struct TableRowLayout<'context> {
    margin: &'context [Span<'static>],
    cols: usize,
    mw: usize,
    widths: &'context [usize],
    sep: &'context Span<'static>,
}

#[derive(Clone, Copy)]
struct TableCellLayout<'context> {
    margin: &'context [Span<'static>],
    cols: usize,
    mw: usize,
    widths: &'context [usize],
    sep: &'context Span<'static>,
}

fn spread_widths(wide: &[usize], widths: &mut [usize], share: usize, left: usize) {
    for (rank, &i) in wide.iter().enumerate() {
        widths[i] = (share + usize::from(rank < left % wide.len().max(1))).max(1);
    }
}

fn append_cell(row: &mut Row, spans: &mut Vec<Span<'static>>, cell: &Row, x: usize) {
    spans.extend(cell.line.spans.iter().cloned());
    row.links.extend(
        cell.links
            .iter()
            .map(|(start, end, url)| (start + x as u16, end + x as u16, url.clone())),
    );

    row.copies.extend(
        cell.copies
            .iter()
            .map(|(start, end, text)| (start + x as u16, end + x as u16, text.clone())),
    );
}
