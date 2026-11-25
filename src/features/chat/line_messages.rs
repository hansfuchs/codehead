use super::{
    Attachment, Color, Drawn, Fold, HashMap, Line, Row, Span, Style, UnicodeWidthStr, agents_on, attachments, badge,
    clip_row, clock, color_of, link_markers, markdown, memo, needs_harness, theme, thought_rows,
};

pub(super) fn draw_settled_line(
    colors: &HashMap<String, Color>,
    width: usize,
    drawn: &mut Drawn,
    last_agent: &mut Option<String>,
    context: AnswerContent<'_>,
) {
    let AnswerContent {
        agent,
        question,
        answer,
        allow,
        permission,
    } = context;

    let text = match (permission, allow) {
        (false, _) => format!(
            "asked: {} · answered: {answer}",
            question.lines().next().unwrap_or_default()
        ),
        (true, true) => format!("{} · allowed", question.lines().next().unwrap_or_default()),
        (true, false) => format!("{} · denied", question.lines().next().unwrap_or_default()),
    };

    let color = if allow {
        theme::pal().success()
    } else {
        theme::pal().caution()
    };

    let agent_color = color_of(colors, agent);
    let pre = [
        Span::styled("● ", Style::default().fg(color)),
        Span::styled(format!("{agent} "), Style::default().fg(agent_color)),
    ];

    drawn
        .rows
        .extend(markdown::plain(&text, width, Style::default().fg(color), &pre));
    *last_agent = None;
}
pub(super) fn draw_message_lines(
    layout: LineMessageContext<'_>,
    blank: impl Fn(&mut Drawn),
    divider: impl Fn(&mut Drawn, Color, &str),
    content: TextContent<'_>,
) {
    let TextContent {
        agent,
        text,
        ts,
        thoughts,
        key,
    } = content;

    let LineMessageContext {
        colors,
        roster,
        fold,
        width,
        drawn,
        last_agent,
        minute,
    } = layout;

    let color = color_of(colors, agent);

    if last_agent.as_deref() != Some(agent) {
        blank(drawn);
        drawn.rows.push(Row::plain(Line::from(badge(agent, color, ts))));
    } else if *minute != clock(ts) {
        blank(drawn);
        divider(drawn, color, &clock(ts));
    } else {
        blank(drawn);
    }

    *minute = clock(ts);
    let bar = [Span::styled("│ ", Style::default().fg(color))];

    thought_rows(drawn, thoughts, key, fold, width, &bar);
    drawn.rows.extend(memo::markdown(text, width, &bar));

    finish_message(agent, text, roster, width, drawn, last_agent);
}

fn finish_message(
    agent: &String,
    text: &str,
    roster: &[(String, String)],
    width: usize,
    drawn: &mut Drawn,
    last_agent: &mut Option<String>,
) {
    if let Some((harness, what)) = needs_harness(text) {
        let label = crate::engine::harnesses::get(&harness).map_or(harness.as_str(), |known| known.label);

        let what = if what.is_empty() {
            String::new()
        } else {
            format!(" for {what}")
        };

        let capable = agents_on(roster, &harness, true, agent);
        let pre = [Span::styled("● ", theme::fg(theme::pal().attention()))];

        drawn.rows.extend(markdown::plain(
            &format!("{agent} needs a {label} agent{what}."),
            width,
            theme::fg(theme::pal().attention()),
            &pre,
        ));

        let list = if capable.is_empty() {
            "none".to_string()
        } else {
            capable
                .iter()
                .map(|(name, _)| format!("@{name}"))
                .collect::<Vec<_>>()
                .join(", ")
                + " · @agent → + adds it"
        };

        drawn.rows.extend(markdown::plain(
            &format!("Agents on {label}: {list}"),
            width,
            theme::muted(),
            &[Span::raw("  ")],
        ));
    }

    *last_agent = Some(agent.clone());
}
pub(super) fn draw_owner_prompt(
    width: usize,
    drawn: &mut Drawn,
    blank: impl Fn(&mut Drawn),
    last_agent: &mut Option<String>,
    to: &[String],
    text: &str,
    ts: u64,
) {
    // the owner's message: a bubble on the right, solid hover bg, the time on its left
    blank(drawn);

    let to_note = if to.len() > 1 || last_agent.as_deref() != to.first().map(String::as_str) {
        format!(" → {}", to.join(", "))
    } else {
        String::new()
    };

    let bg = Style::default().bg(theme::pal().surface1);
    let rows = owner_message_rows(width, text);
    let inner = rows.iter().map(|row| row.line.width()).max().unwrap_or(0);
    let box_w = inner + 4;
    let left = width.saturating_sub(box_w);
    let stamp = format!("{}{to_note} ", clock(ts));
    let row_count = rows.len();

    for (i, row) in rows.into_iter().enumerate() {
        let pad = inner - row.line.width();

        // left of the box: the time on the 1st row, a copy button on the last
        let time = if i == 0 && stamp.width() < left {
            stamp.clone()
        } else {
            String::new()
        };

        let copy = i + 1 == row_count && time.width() + 6 < left;
        let used = time.width() + if copy { 5 } else { 0 };
        let mut spans = vec![Span::raw(" ".repeat(left - used)), Span::styled(time, theme::muted())];
        let mut copies: Vec<(u16, u16, String)> = Vec::new();

        if copy {
            spans.push(Span::styled("copy ", theme::accent()));
            copies.push(((left - 5) as u16, (left - 1) as u16, text.trim_end().to_string()));
        }

        spans.push(Span::styled("  ", bg));
        spans.extend(row.line.spans);
        spans.push(Span::styled(" ".repeat(pad + 2), bg));
        let shift = (left + 2) as u16;

        copies.extend(
            row.copies
                .into_iter()
                .map(|(start, end, text)| (start + shift, end + shift, text)),
        );
        drawn.rows.push(Row {
            wrap: row.wrap,
            line: Line::from(spans),
            links: row
                .links
                .into_iter()
                .map(|(start, end, link)| (start + shift, end + shift, link))
                .collect(),
            copies,
        });
    }

    *last_agent = None;
}

fn shade_row(row: &mut Row) {
    for span in &mut row.line.spans {
        if span.style.bg.is_none() {
            span.style = span.style.bg(theme::pal().surface1);
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct AnswerContent<'render> {
    pub(super) agent: &'render String,
    pub(super) question: &'render str,
    pub(super) answer: &'render String,
    pub(super) allow: bool,
    pub(super) permission: bool,
}

pub(super) struct LineMessageContext<'render> {
    pub(super) colors: &'render HashMap<String, Color>,
    pub(super) roster: &'render [(String, String)],
    pub(super) fold: &'render Fold<'render>,
    pub(super) width: usize,
    pub(super) drawn: &'render mut Drawn,
    pub(super) last_agent: &'render mut Option<String>,
    pub(super) minute: &'render mut String,
}

#[derive(Clone, Copy)]
pub(super) struct TextContent<'render> {
    pub(super) agent: &'render String,
    pub(super) text: &'render str,
    pub(super) ts: u64,
    pub(super) thoughts: &'render [String],
    pub(super) key: usize,
}

fn owner_message_rows(width: usize, text: &str) -> Vec<Row> {
    let (body, files) = attachments(text);
    let bubble_width = (width * 3 / 4).max(20).min(width);

    // markdown as the agents' text (bold, lists, code); the hover bg under every span
    let mut rows = memo::markdown(body.trim_end(), bubble_width.saturating_sub(4), &[]);

    link_markers(&mut rows, &files);

    for row in &mut rows {
        shade_row(row);
    }

    let clip = |file: &Attachment| {
        file.marker
            .as_ref()
            .map_or_else(|| file.path.clone(), |marker| format!("{marker} {}", file.path))
    };

    rows.extend(
        files
            .iter()
            .map(|file| clip_row(file, &clip(file), theme::muted().bg(theme::pal().surface1), 0)),
    );

    rows
}
