use super::{Color, Line, Row, Span, Style, UnicodeWidthStr, theme};

pub(super) fn lang_w(lang: &str) -> usize {
    if lang == "text" { 0 } else { lang.width() + 1 }
}

/// A code block: syntax colors on the panel color (FG: 1 color for every char instead), a head row
/// with the language + a copy button. Long lines: cut at the row end, or WRAP (at a word end, `↪ ` rows).
/// Code blocks up to this many lines get syntax colors (1 pass each, cached).
pub(super) const MAX_HIGHLIGHT: usize = 4000;

pub(crate) fn code_rows(
    text: &str,
    lang: &str,
    width: usize,
    margin: &[Span<'static>],
    wrap: bool,
    fg: Option<Color>,
) -> Vec<Row> {
    let lang = if lang.is_empty() {
        crate::ui::code::language(text, &[])
    } else {
        lang
    };

    let lines: Vec<&str> = text.trim_end_matches('\n').split('\n').collect();
    let mw: usize = margin.iter().map(|span| span.content.width()).sum();
    let room = width.saturating_sub(mw + 2).max(1);
    let bg = Style::default().bg(theme::pal().surface0);
    let mut out = Vec::new();

    // head row: the language left, a copy button right
    let label = if lang == "text" {
        String::new()
    } else {
        format!(" {lang}")
    };

    let button = " copy ";
    let fill = (room + 2).saturating_sub(label.width() + button.width());
    let mut head = margin.to_vec();

    head.push(Span::styled(label, bg.fg(theme::pal().subtext0)));
    head.push(Span::styled(" ".repeat(fill), bg));
    head.push(Span::styled(button, bg.fg(theme::pal().subtext0)));
    let at = (mw + fill + lang_w(lang)) as u16;

    out.push(Row {
        line: Line::from(head),
        copies: vec![(at, at + button.width() as u16, text.trim_end_matches('\n').to_string())],
        ..Default::default()
    });

    // 1 highlight pass per block (cached); very long blocks get plain rows
    let colored =
        (fg.is_none() && lines.len() <= MAX_HIGHLIGHT).then(|| crate::ui::code::highlight(&lines.join("\n"), lang));

    draw_code_rows(
        &(lines),
        out,
        colored.as_ref(),
        CodeLayout {
            margin,
            wrap,
            fg,
            room,
            bg,
        },
    )
}

fn draw_code_rows(
    lines: &Vec<&str>,
    mut out: Vec<Row>,
    colored: Option<&CodeColors>,
    settings: CodeLayout<'_>,
) -> Vec<Row> {
    let CodeLayout {
        margin,
        wrap,
        fg,
        room,
        bg,
    } = settings;

    for (i, line) in lines.iter().enumerate() {
        draw_code_line(
            &mut out,
            colored,
            i,
            line,
            CodeRow {
                margin,
                wrap,
                fg,
                room,
                bg,
            },
        );
    }

    out
}

fn draw_code_line(out: &mut Vec<Row>, colored: Option<&CodeColors>, i: usize, line: &&str, settings: CodeRow<'_>) {
    let CodeRow {
        margin,
        wrap,
        fg,
        room,
        bg,
    } = settings;

    let chars: Vec<(Color, char)> = match (fg, colored.and_then(|colored| colored.get(i))) {
        (Some(fg), _) => line.chars().filter(|ch| !ch.is_control()).map(|ch| (fg, ch)).collect(),
        (None, Some(colors)) => colors.clone(),
        (None, None) => line.chars().map(|ch| (theme::pal().text, ch)).collect(),
    };

    let segments: Vec<std::ops::Range<usize>> = if wrap {
        wrap_code(&chars, room)
    } else {
        std::iter::once(0..cut_at(&chars, room)).collect()
    };

    for (row, seg) in segments.into_iter().enumerate() {
        draw_code_segment(out, margin, room, bg, &chars, row, seg);
    }
}

fn draw_code_segment(
    out: &mut Vec<Row>,
    margin: &[Span<'static>],
    room: usize,
    bg: Style,
    chars: &[(Color, char)],
    row: usize,
    seg: std::ops::Range<usize>,
) {
    let mut spans = margin.to_vec();

    spans.push(Span::styled(" ", bg));
    let mut width = 0;

    // a continuation row: `↪ ` (the line goes on, 0 new line in the source)
    if row > 0 {
        spans.push(Span::styled(CONT, bg.fg(theme::pal().subtext0)));
        width += CONT.width();
    }

    let mut run = String::new();
    let mut run_color = None;

    for &(color, ch) in &chars[seg] {
        if run_color != Some(color) && !run.is_empty() {
            spans.push(Span::styled(
                std::mem::take(&mut run),
                bg.fg(run_color.unwrap_or(theme::pal().text)),
            ));
        }

        run_color = Some(color);
        run.push(ch);
        width += unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
    }

    if !run.is_empty() {
        spans.push(Span::styled(run, bg.fg(run_color.unwrap_or(theme::pal().text))));
    }

    spans.push(Span::styled(" ".repeat((room + 1).saturating_sub(width)), bg));
    out.push(Row::plain(Line::from(spans)));
}

/// The mark of a wrapped code line's continuation rows.
pub(super) const CONT: &str = "↪ ";

pub(super) fn char_w(character: char) -> usize {
    unicode_width::UnicodeWidthChar::width(character).unwrap_or(0)
}

/// End of the chars of CHARS that fit ROOM columns.
pub(super) fn cut_at(chars: &[(Color, char)], room: usize) -> usize {
    let mut width = 0;

    chars
        .iter()
        .position(|&(_, character)| {
            width += char_w(character);
            width > room
        })
        .unwrap_or(chars.len())
}

/// Rows of 1 code line CHARS in ROOM columns: the 1st row full width, the next ones after `↪ `.
/// A row ends after a space or a separator (`,` `;` `)` `/` …) in its 2nd half, else at the room.
pub(super) fn wrap_code(chars: &[(Color, char)], room: usize) -> Vec<std::ops::Range<usize>> {
    let mut out = Vec::new();
    let mut start = 0;

    loop {
        let budget = if out.is_empty() {
            room
        } else {
            room.saturating_sub(CONT.width()).max(1)
        };

        let fit_end = start + cut_at(&chars[start..], budget);

        if fit_end >= chars.len() {
            out.push(start..chars.len());
            return out;
        }

        let half = start + (fit_end - start) / 2;
        let brk = (half + 1..=fit_end)
            .rev()
            .find(|&at| chars[at - 1].1.is_whitespace() || ",;:)]}/|&=>.-".contains(chars[at - 1].1));

        // 0 chars fit (a wide char in a 1-column room): take 1 anyway
        let end = brk.unwrap_or(fit_end).max(start + 1);

        out.push(start..end);
        start = end;
    }
}

#[derive(Clone, Copy)]
struct CodeLayout<'context> {
    margin: &'context [Span<'static>],
    wrap: bool,
    fg: Option<Color>,
    room: usize,
    bg: Style,
}

#[derive(Clone, Copy)]
struct CodeRow<'context> {
    margin: &'context [Span<'static>],
    wrap: bool,
    fg: Option<Color>,
    room: usize,
    bg: Style,
}

type CodeColors = std::sync::Arc<Vec<Vec<(Color, char)>>>;
