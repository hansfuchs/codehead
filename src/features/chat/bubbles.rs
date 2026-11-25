use super::{
    Attachment, Drawn, Line, Modifier, Row, Span, Style, UnicodeWidthChar, UnicodeWidthStr, attachments, clip_row,
    clock, link_markers, markdown, memo, theme,
};

/// The owner's message TEXT as a bubble on the right of a W wide stream: content-sized (max. 3/4),
/// the time (+ `TO_NOTE`) in its top border, attached files below it.
pub(super) fn owner_bubble(drawn: &mut Drawn, width: usize, text: &str, to_note: &str, ts: u64) {
    let (body, files) = attachments(text);
    let body = body.trim_end();
    let cap = (width * 3 / 4 + 1).min(width);
    let stamp = clock(ts);
    let want = body
        .lines()
        .map(|line| line.replace('\t', "    ").width())
        .max()
        .unwrap_or(0)
        + 4;
    let bw = want
        .max(stamp.width() + 6)
        .max(if to_note.is_empty() {
            0
        } else {
            to_note.width() + stamp.width() + 9
        })
        .min(cap)
        .max(8);
    let left = width - bw;
    let border = Style::default().fg(theme::pal().surface1);
    let pad = Span::raw(" ".repeat(left));

    let lead = if to_note.is_empty() {
        Vec::new()
    } else {
        vec![Span::styled(to_note.to_string(), theme::muted())]
    };

    let (top, _) = top_border(bw, border, lead, false, &stamp);

    drawn.rows.push(Row {
        line: Line::from([vec![pad.clone()], top.line.spans].concat()),
        ..Default::default()
    });
    let bar = [pad.clone(), Span::styled("│ ", border)];

    // 0 blank row below the top border, 1 above the bottom border
    let mut rows = memo::markdown(body, left + bw - 2, &bar);

    link_markers(&mut rows, &files);
    rows.push(Row::plain(Line::from(bar.to_vec())));

    finish_owner_bubble(
        drawn,
        width,
        text,
        &files,
        OwnerBubbleLayout { bw, left, border, pad },
        rows,
    );
}

fn finish_owner_bubble(
    drawn: &mut Drawn,
    width: usize,
    text: &str,
    files: &[Attachment],
    context: OwnerBubbleLayout,
    rows: Vec<Row>,
) {
    let OwnerBubbleLayout { bw, left, border, pad } = context;

    for mut row in rows {
        let rw = row.line.width();

        row.line
            .spans
            .push(Span::raw(" ".repeat((left + bw - 1).saturating_sub(rw))));
        row.line.spans.push(Span::styled("│", border));
        drawn.rows.push(row);
    }

    // the bottom border: a copy button on the right (the message as sent)
    let button = " copy ";

    if bw >= button.width() + 6 {
        let fill = bw - 2 - button.width() - 2;
        let at = (left + 1 + fill) as u16;

        drawn.rows.push(Row {
            line: Line::from(vec![
                pad,
                Span::styled(format!("└{}", "─".repeat(fill)), border),
                Span::styled(button, theme::accent()),
                Span::styled("──┘", border),
            ]),
            copies: vec![(at, at + button.width() as u16, text.trim_end().to_string())],
            ..Default::default()
        });
    } else {
        drawn.rows.push(Row::plain(Line::from(vec![
            pad,
            Span::styled(format!("└{}┘", "─".repeat(bw - 2)), border),
        ])));
    }

    drawn.rows.extend(
        files
            .iter()
            .map(|file| clip_row(file, file.name(), theme::muted(), width)),
    );
}
/// A divider in an agent's box: `├──────── 13:42 ─┤`, joined to both borders (the time where
/// the top border has it). Full width W: the close adds 0 right border to it.
pub(super) fn minute_divider(width: usize, border: Style, at: &str) -> Row {
    let fill = width.saturating_sub(1 + at.width() + 2 + 2);

    Row::plain(Line::from(vec![
        Span::styled(format!("├{}", "─".repeat(fill)), border),
        Span::styled(format!(" {at} "), theme::muted()),
        Span::styled("─┤", border),
    ]))
}
/// The name of a sub-thread's end whose agent is unknown.
pub(super) const KID: &str = "subthread";
/// The tag next to AGENT of a sub-thread's end: ` subthread` (0 for agent KID), its state if not
/// done.
pub(super) fn kid_tag(agent: &str, state: &str) -> Vec<Span<'static>> {
    let mut out = Vec::new();

    if agent != KID {
        out.push(Span::styled(" subthread", theme::muted()));
    }

    if state != "done" {
        out.push(Span::styled(
            format!(" · {state}"),
            Style::default().fg(theme::pal().error()),
        ));
    }

    out
}
/// The body of a sub-thread's end at WIDTH: its task (1 muted row), its result (markdown) or why
/// it ended.
pub(super) fn kid_rows(title: &str, state: &str, text: &str, width: usize, bar: &[Span<'static>]) -> Vec<Row> {
    let mut rows = Vec::new();

    if !title.trim().is_empty() {
        let max = width
            .saturating_sub(bar.iter().map(|span| span.content.width()).sum::<usize>() + 4)
            .max(1);

        let task = if title.chars().count() > max {
            title.chars().take(max - 1).collect::<String>() + "…"
        } else {
            title.to_string()
        };

        rows.extend(markdown::plain(
            &format!("↳ “{task}”"),
            width,
            theme::muted().add_modifier(Modifier::ITALIC),
            bar,
        ));
        rows.push(Row::plain(Line::from(bar.to_vec())));
    }

    if state == "done" {
        rows.extend(memo::markdown(text.trim_end(), width, bar));
    } else {
        rows.extend(markdown::plain(
            text.trim_end(),
            width,
            Style::default().fg(theme::pal().error()),
            bar,
        ));
    }

    rows
}
/// Top border of width W: `┌─ ` LEFT ` ─────── ` RIGHT ` ─┐`. Returns the row and the
/// column after LEFT (where a wave goes).
pub(super) fn top_border(width: usize, border: Style, left: Vec<Span<'static>>, wave: bool, right: &str) -> (Row, u16) {
    edge(width, border, left, wave, right, ("┌─", "─┐"))
}
/// Bottom border of width W: `└─ ` LEFT ` ───────┘` (an agent's name: in view at the end of a
/// box taller than the view). Returns the row and the column after LEFT (where a wave goes).
pub(super) fn bottom_border(width: usize, border: Style, left: Vec<Span<'static>>, wave: bool) -> (Row, u16) {
    edge(width, border, left, wave, "", ("└─", "─┘"))
}
/// LEFT cut to ROOM columns, its last char `…` when cut.
pub(super) fn cut_spans(left: Vec<Span<'static>>, room: usize) -> Vec<Span<'static>> {
    if left.iter().map(|span| span.content.width()).sum::<usize>() <= room {
        return left;
    }

    let mut out = Vec::new();
    let mut used = 0;

    for span in left {
        let sw = span.content.width();

        if used + sw < room {
            used += sw;
            out.push(span);
            continue;
        }

        let mut text = cut_span_text(&span, room, &mut used);

        text.push('…');
        out.push(Span::styled(text, span.style));
        break;
    }

    out
}
/// A border of width W between the corners ENDS: LEFT (cut to fit), a wave's room, RIGHT.
pub(super) fn edge(
    width: usize,
    border: Style,
    left: Vec<Span<'static>>,
    wave: bool,
    right: &str,
    ends: (&str, &str),
) -> (Row, u16) {
    let mut spans = vec![Span::styled(ends.0.to_string(), border)];
    let rw = if right.is_empty() { 0 } else { right.width() + 2 };
    let left = cut_spans(
        left,
        width
            .saturating_sub(2 + 1 + if wave { 4 } else { 0 } + 1 + rw + 2)
            .max(1),
    );
    let lw: usize = left.iter().map(|span| span.content.width()).sum();
    let mut used = 2;

    if lw > 0 {
        spans.push(Span::styled(" ", border));
        spans.extend(left);
        used += 1 + lw;
    }

    let wave_at = used + 1;

    if wave {
        spans.push(Span::raw("    "));
        used += 4;
    }

    if lw > 0 && used < width {
        spans.push(Span::styled(" ", border));
        used += 1;
    }

    let fill = width.saturating_sub(used + rw + 2);

    spans.push(Span::styled("─".repeat(fill), border));

    if rw > 0 {
        spans.push(Span::styled(format!(" {right} "), theme::muted()));
    }

    spans.push(Span::styled(ends.1.to_string(), border));

    (Row::plain(Line::from(spans)), wave_at as u16)
}

pub(super) struct OwnerBubbleLayout {
    pub(super) bw: usize,
    pub(super) left: usize,
    pub(super) border: Style,
    pub(super) pad: Span<'static>,
}

fn cut_span_text(span: &Span<'static>, room: usize, used: &mut usize) -> String {
    let mut text = String::new();

    for character in span.content.chars() {
        let width = character.width().unwrap_or(0);

        if *used + width + 1 > room {
            break;
        }

        *used += width;
        text.push(character);
    }

    text
}
