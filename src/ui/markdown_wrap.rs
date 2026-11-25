use super::{Line, Piece, Row, Span, Style, UnicodeWidthStr, mention_pieces};

struct WrapState<'prefix> {
    width: usize,
    rest: &'prefix [Span<'static>],
    rest_width: usize,
    rows: Vec<Row>,
    cur: Vec<Span<'static>>,
    links: Vec<(u16, u16, String)>,
    copies: Vec<(u16, u16, String)>,
    col: usize,
    limit: usize,
    empty: bool,
    kind: u8,
    prev_space: bool,
}

pub(super) fn wrap(pieces: &[Piece], width: usize, first: &[Span<'static>], rest: &[Span<'static>]) -> Vec<Row> {
    let pieces = mention_pieces(pieces);
    let words = split_words(&pieces);
    let glued = glued_widths(&words);
    let first_width = prefix_width(first);
    let mut state = WrapState {
        width,
        rest,
        rest_width: prefix_width(rest),
        rows: Vec::new(),
        cur: first.to_vec(),
        links: Vec::new(),
        copies: Vec::new(),
        col: first_width,
        limit: width.max(first_width + 1),
        empty: true,
        kind: 0,
        prev_space: true,
    };

    for (index, (piece, space)) in words.into_iter().enumerate() {
        wrap_word(&mut state, &(piece), space, glued[index]);
    }

    if !state.empty || state.rows.is_empty() {
        push_row(&mut state);
    }

    state.rows
}

fn prefix_width(prefix: &[Span<'static>]) -> usize {
    prefix.iter().map(|span| span.content.width()).sum()
}

fn split_words(pieces: &[Piece]) -> Vec<(Piece, bool)> {
    let mut words = Vec::new();

    for piece in pieces {
        split_piece(&mut words, piece);
    }

    words
}

fn split_piece(words: &mut Vec<(Piece, bool)>, piece: &Piece) {
    let mut buffer = String::new();

    for character in piece.text.chars() {
        if character == ' ' {
            push_word(words, piece, &mut buffer);
            words.push((
                Piece {
                    text: " ".into(),
                    ..piece.clone()
                },
                true,
            ));
        } else {
            buffer.push(character);
        }
    }

    push_word(words, piece, &mut buffer);
}

fn push_word(words: &mut Vec<(Piece, bool)>, piece: &Piece, buffer: &mut String) {
    if !buffer.is_empty() {
        words.push((
            Piece {
                text: std::mem::take(buffer),
                ..piece.clone()
            },
            false,
        ));
    }
}

fn glued_widths(words: &[(Piece, bool)]) -> Vec<usize> {
    let mut widths = vec![0; words.len()];
    let mut index = words.len();
    let mut run = 0;

    while index > 0 {
        index -= 1;

        run = if words[index].1 {
            0
        } else {
            run + words[index].0.text.width()
        };

        widths[index] = run;
    }

    widths
}

fn wrap_word(state: &mut WrapState<'_>, piece: &Piece, space: bool, glued: usize) {
    let width = piece.text.width();

    if !space
        && state.prev_space
        && glued > width
        && !state.empty
        && state.col + glued > state.limit
        && glued <= state.width.saturating_sub(state.rest_width)
    {
        trim_space(state);
        push_row(state);
        next_row(state, 1);
        state.empty = true;
    }

    state.prev_space = space;

    if space {
        append_space(state, piece.style);
        return;
    }

    if !state.empty && state.col + width > state.limit {
        trim_space(state);
        push_row(state);
        next_row(state, 1);
    }

    let text = split_long_word(state, piece, piece.text.clone());
    let width = text.width();

    record_links(state, piece, width);
    state.col += width;
    state.cur.push(Span::styled(text, piece.style));
    state.empty = false;
}

fn append_space(state: &mut WrapState<'_>, style: Style) {
    if !state.empty && state.col < state.limit {
        state.cur.push(Span::styled(" ", style));
        state.col += 1;
    }
}

fn trim_space(state: &mut WrapState<'_>) {
    if state.cur.last().is_some_and(|span| span.content == " ") {
        state.cur.pop();
    }
}

fn split_long_word(state: &mut WrapState<'_>, piece: &Piece, mut text: String) -> String {
    while state.col + text.width() > state.limit {
        let room = state.limit.saturating_sub(state.col).max(1);
        let (cut, width) = cut_word(&text, room);

        if cut.is_empty() {
            break;
        }

        text = text[cut.len()..].to_string();
        record_links(state, piece, width);
        state.cur.push(Span::styled(cut, piece.style));
        push_row(state);
        next_row(state, 2);
    }

    text
}

fn cut_word(text: &str, room: usize) -> (String, usize) {
    let mut cut = String::new();
    let mut width = 0;

    for character in text.chars() {
        let columns = unicode_width::UnicodeWidthChar::width(character).unwrap_or(0);

        if width + columns > room {
            break;
        }

        cut.push(character);
        width += columns;
    }

    (cut, width)
}

fn record_links(state: &mut WrapState<'_>, piece: &Piece, width: usize) {
    if let Some(link) = &piece.link {
        state
            .links
            .push((state.col as u16, (state.col + width) as u16, link.clone()));
    }

    if let Some(copy) = &piece.copy {
        state
            .copies
            .push((state.col as u16, (state.col + width) as u16, copy.clone()));
    }
}

fn push_row(state: &mut WrapState<'_>) {
    state.rows.push(Row {
        line: Line::from(std::mem::take(&mut state.cur)),
        links: std::mem::take(&mut state.links),
        copies: std::mem::take(&mut state.copies),
        wrap: state.kind,
    });
}

fn next_row(state: &mut WrapState<'_>, kind: u8) {
    state.kind = kind;
    state.cur = state.rest.to_vec();
    state.col = state.rest_width;
    state.limit = state.width.max(state.col + 1);
}

/// Plain TEXT wrapped to WIDTH in STYLE, PREFIX before every row.
pub(crate) fn plain(text: &str, width: usize, style: Style, prefix: &[Span<'static>]) -> Vec<Row> {
    text.split('\n')
        .flat_map(|line| {
            wrap(
                &[Piece {
                    text: line.replace('\t', "    "),
                    style,
                    ..Default::default()
                }],
                width,
                prefix,
                prefix,
            )
        })
        .collect()
}
