#[cfg(test)]
use super::Drawn;
use super::{Attachment, HashMap, Line, Modifier, Path, Row, Span, Style, UnicodeWidthStr, theme};

/// The `file://` link of an attachment: None for a gone file (a cleared cache, /tmp). Its check
/// is memoized 2 s: a redraw per streamed token does 0 stat calls.
pub(super) fn attach_link(file: &Attachment) -> Option<String> {
    use std::cell::RefCell;
    use std::time::{Duration, Instant};

    thread_local! {
        static SEEN: RefCell<HashMap<String, (Instant, bool)>> = RefCell::new(HashMap::new());
    }

    let fresh = if cfg!(test) {
        Duration::ZERO
    } else {
        Duration::from_secs(2)
    };

    let exists = SEEN.with(|seen| {
        let seen = seen.borrow_mut();

        match seen.get(&file.path) {
            Some((at, ok)) if at.elapsed() < fresh => *ok,
            _ => check_file(file, seen),
        }
    });

    exists.then(|| format!("file://{}", file.path))
}

fn check_file(file: &Attachment, mut seen: std::cell::RefMut<'_, HashMap<String, (std::time::Instant, bool)>>) -> bool {
    if seen.len() > 1024 {
        seen.clear();
    }

    let ok = Path::new(&file.path).is_file();

    seen.insert(file.path.clone(), (std::time::Instant::now(), ok));
    ok
}
/// The `📎` row of attachment A: its LABEL, a link over it (0 link for a gone file: `· missing`).
/// RIGHT > 0: right-aligned in RIGHT cols.
pub(super) fn clip_row(file: &Attachment, label: &str, style: Style, right: usize) -> Row {
    let link = attach_link(file);

    let label = if link.is_some() {
        format!("📎 {label}")
    } else {
        format!("📎 {label} · missing")
    };

    let lead = right.saturating_sub(label.width());
    let (from, to) = (lead as u16, (lead + label.width()) as u16);

    let mut spans = vec![Span::styled(label, style)];

    if lead > 0 {
        spans.insert(0, Span::raw(" ".repeat(lead)));
    }

    Row {
        line: Line::from(spans),
        links: link.map(|link| vec![(from, to, link)]).unwrap_or_default(),
        ..Default::default()
    }
}
/// The markers of ATTS in ROWS (`[Image #1]` in the body): links to their files, underlined in
/// the link color. A marker cut by a wrap: 0 link.
pub(super) fn link_markers(rows: &mut [Row], atts: &[Attachment]) {
    let links: Vec<(&str, String)> = atts
        .iter()
        .filter_map(|file| Some((file.marker.as_deref()?, attach_link(file)?)))
        .collect();

    if links.is_empty() {
        return;
    }

    for row in rows {
        link_row_markers(&links, row);
    }
}

fn link_row_markers(links: &Vec<(&str, String)>, row: &mut Row) {
    let text: String = row.line.spans.iter().map(|span| span.content.as_ref()).collect();
    let mut hits: Vec<(usize, usize, &str)> = Vec::new();

    // byte ranges of the markers in the row's text
    for (marker, url) in links {
        hits.extend(
            text.match_indices(marker)
                .map(|(i, _)| (i, i + marker.len(), url.as_str())),
        );
    }

    if hits.is_empty() {
        return;
    }

    hits.sort_by_key(|hit| hit.0);
    let mut out: Vec<Span<'static>> = Vec::new();
    let mut at = 0;

    // split the spans at the marker bounds
    for span in std::mem::take(&mut row.line.spans) {
        split_span(&hits, &mut out, &mut at, &span);
    }

    let mut at = 0;

    // the marker pieces underlined, a link over each marker's cols
    for span in &mut out {
        if hits.iter().any(|&(start, end, _)| at >= start && at < end) {
            span.style = span.style.fg(theme::pal().info()).add_modifier(Modifier::UNDERLINED);
        }

        at += span.content.len();
    }

    for &(start, end, url) in &hits {
        let (from, to) = (text[..start].width() as u16, text[..end].width() as u16);

        row.links.push((from, to, url.to_string()));
    }

    row.line.spans = out;
}

fn split_span(hits: &Vec<(usize, usize, &str)>, out: &mut Vec<Span<'static>>, at: &mut usize, span: &Span<'_>) {
    let span_end = *at + span.content.len();
    let mut cut = *at;

    for &(marker_start, marker_end, _) in hits {
        push_pieces(out, at, span, span_end, &mut cut, marker_start, marker_end);
    }

    out.push(Span::styled(span.content[cut - *at..].to_string(), span.style));
    *at = span_end;
}

fn push_pieces(
    out: &mut Vec<Span<'static>>,
    at: &mut usize,
    span: &Span<'_>,
    span_end: usize,
    cut: &mut usize,
    marker_start: usize,
    marker_end: usize,
) {
    for edge in [marker_start, marker_end] {
        if edge > *cut && edge < span_end {
            out.push(Span::styled(
                span.content[*cut - *at..edge - *at].to_string(),
                span.style,
            ));
            *cut = edge;
        }
    }
}
/// Width of the widest row (tests).
#[cfg(test)]
pub(super) fn widest(drawn: &Drawn) -> usize {
    drawn.rows.iter().map(|row| row.line.width()).max().unwrap_or(0)
}
