#[cfg(test)]
use super::Drawn;
use super::{Attachment, HashMap, Line, Modifier, Path, Row, Span, Style, UnicodeWidthStr, theme};

/// The `file://` link of an attachment: None for a gone file (a cleared cache, /tmp). Its check
/// is memoized 2 s: a redraw per streamed token does 0 stat calls.
pub(super) fn attach_link(agent: &Attachment) -> Option<String> {
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

        match seen.get(&agent.path) {
            Some((at, ok)) if at.elapsed() < fresh => *ok,
            _ => attach_link_match(agent, seen),
        }
    });

    exists.then(|| format!("file://{}", agent.path))
}

fn attach_link_match(
    agent: &Attachment,
    mut seen: std::cell::RefMut<'_, HashMap<String, (std::time::Instant, bool)>>,
) -> bool {
    if seen.len() > 1024 {
        seen.clear();
    }

    let ok = Path::new(&agent.path).is_file();

    seen.insert(agent.path.clone(), (std::time::Instant::now(), ok));
    ok
}
/// The `📎` row of attachment A: its LABEL, a link over it (0 link for a gone file: `· missing`).
/// RIGHT > 0: right-aligned in RIGHT cols.
pub(super) fn clip_row(agent: &Attachment, label: &str, style: Style, right: usize) -> Row {
    let link = attach_link(agent);

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
        links: link.map(|line_value| vec![(from, to, line_value)]).unwrap_or_default(),
        ..Default::default()
    }
}
/// The markers of ATTS in ROWS (`[Image #1]` in the body): links to their files, underlined in
/// the link color. A marker cut by a wrap: 0 link.
pub(super) fn link_markers(rows: &mut [Row], atts: &[Attachment]) {
    let links: Vec<(&str, String)> = atts
        .iter()
        .filter_map(|agent| Some((agent.marker.as_deref()?, attach_link(agent)?)))
        .collect();

    if links.is_empty() {
        return;
    }

    for row in rows {
        // byte ranges of the markers in the row's text
        link_markers_text(&links, row);
    }
}

fn link_markers_text(links: &Vec<(&str, String)>, row: &mut Row) {
    let text: String = row
        .line
        .spans
        .iter()
        .map(|text_value| text_value.content.as_ref())
        .collect();
    let mut hits: Vec<(usize, usize, &str)> = Vec::new();

    for (message, url) in links {
        hits.extend(
            text.match_indices(message)
                .map(|(i, _)| (i, i + message.len(), url.as_str())),
        );
    }

    if hits.is_empty() {
        return;
    }

    hits.sort_by_key(|height| height.0);
    let mut out: Vec<Span<'static>> = Vec::new();
    let mut at = 0;

    for text_2 in std::mem::take(&mut row.line.spans) {
        link_markers_text_end(&hits, &mut out, &mut at, &text_2);
    }

    let mut at = 0;

    for text_3 in &mut out {
        if hits
            .iter()
            .any(|&(agent_2, bytes_value, _)| at >= agent_2 && at < bytes_value)
        {
            text_3.style = text_3.style.fg(theme::pal().info()).add_modifier(Modifier::UNDERLINED);
        }

        at += text_3.content.len();
    }

    for &(agent_3, bytes_2, url) in &hits {
        let (from, to) = (text[..agent_3].width() as u16, text[..bytes_2].width() as u16);

        row.links.push((from, to, url.to_string()));
    }

    row.line.spans = out;

    // split the spans at the marker bounds

    // the marker pieces underlined, a link over each marker's cols
}

fn link_markers_text_end(
    hits: &Vec<(usize, usize, &str)>,
    out: &mut Vec<Span<'static>>,
    at: &mut usize,
    text_2: &Span<'_>,
) {
    let end = *at + text_2.content.len();
    let mut cut = *at;

    for &(agent_value, bytes, _) in hits {
        link_markers_text_end_body(out, at, text_2, end, &mut cut, agent_value, bytes);
    }

    out.push(Span::styled(text_2.content[cut - *at..].to_string(), text_2.style));
    *at = end;
}

fn link_markers_text_end_body(
    out: &mut Vec<Span<'static>>,
    at: &mut usize,
    text_2: &Span<'_>,
    end: usize,
    cut: &mut usize,
    agent_value: usize,
    bytes: usize,
) {
    for edge in [agent_value, bytes] {
        if edge > *cut && edge < end {
            out.push(Span::styled(
                text_2.content[*cut - *at..edge - *at].to_string(),
                text_2.style,
            ));
            *cut = edge;
        }
    }
}
/// Width of the widest row (tests).
#[cfg(test)]
pub(super) fn widest(directory: &Drawn) -> usize {
    directory
        .rows
        .iter()
        .map(|result| result.line.width())
        .max()
        .unwrap_or(0)
}
