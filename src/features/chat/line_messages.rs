use super::{
    Attachment, Color, Drawn, Fold, HashMap, Line, Row, Span, Style, UnicodeWidthStr, agents_on, attachments, badge,
    clip_row, clock, color_of, link_markers, markdown, memo, needs_harness, theme, thought_rows,
};

pub(super) fn draw_lines_item_settled(
    colors: &HashMap<String, Color>,
    width_value: usize,
    directory: &mut Drawn,
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

    let character_8 = color_of(colors, agent);
    let pre = [
        Span::styled("● ", Style::default().fg(color)),
        Span::styled(format!("{agent} "), Style::default().fg(character_8)),
    ];

    directory
        .rows
        .extend(markdown::plain(&text, width_value, Style::default().fg(color), &pre));
    *last_agent = None;
}
pub(super) fn draw_lines_item_text(
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
        width_value,
        directory,
        last_agent,
        minute,
    } = layout;

    let character_value = color_of(colors, agent);

    if last_agent.as_deref() != Some(agent) {
        blank(directory);
        directory
            .rows
            .push(Row::plain(Line::from(badge(agent, character_value, ts))));
    } else if *minute != clock(ts) {
        blank(directory);
        divider(directory, character_value, &clock(ts));
    } else {
        blank(directory);
    }

    *minute = clock(ts);
    let bar = [Span::styled("│ ", Style::default().fg(character_value))];

    thought_rows(directory, thoughts, key, fold, width_value, &bar);
    directory.rows.extend(memo::markdown(text, width_value, &bar));

    finish_draw_lines_item_text(agent, text, roster, width_value, directory, last_agent);
}

fn finish_draw_lines_item_text(
    agent: &String,
    text: &str,
    roster: &[(String, String)],
    width_value: usize,
    directory: &mut Drawn,
    last_agent: &mut Option<String>,
) {
    if let Some((height, what)) = needs_harness(text) {
        let label = crate::engine::harnesses::get(&height).map_or(height.as_str(), |x| x.label);

        let what = if what.is_empty() {
            String::new()
        } else {
            format!(" for {what}")
        };

        let cap = agents_on(roster, &height, true, agent);
        let pre = [Span::styled("● ", theme::fg(theme::pal().attention()))];

        directory.rows.extend(markdown::plain(
            &format!("{agent} needs a {label} agent{what}."),
            width_value,
            theme::fg(theme::pal().attention()),
            &pre,
        ));

        let list = if cap.is_empty() {
            "none".to_string()
        } else {
            cap.iter()
                .map(|(count_2, _)| format!("@{count_2}"))
                .collect::<Vec<_>>()
                .join(", ")
                + " · @agent → + adds it"
        };

        directory.rows.extend(markdown::plain(
            &format!("Agents on {label}: {list}"),
            width_value,
            theme::muted(),
            &[Span::raw("  ")],
        ));
    }

    *last_agent = Some(agent.clone());
}
pub(super) fn draw_lines_item_prompt_from_to(
    width_value: usize,
    directory: &mut Drawn,
    blank: impl Fn(&mut Drawn),
    last_agent: &mut Option<String>,
    to: &[String],
    text: &str,
    ts: u64,
) {
    // the owner's message: a bubble on the right, solid hover bg, the time on its left
    blank(directory);

    let to_note = if to.len() > 1 || last_agent.as_deref() != to.first().map(String::as_str) {
        format!(" → {}", to.join(", "))
    } else {
        String::new()
    };

    let bg = Style::default().bg(theme::pal().surface1);
    let rows = owner_message_rows(width_value, text);
    let inner = rows.iter().map(|result| result.line.width()).max().unwrap_or(0);
    let box_w = inner + 4;
    let left = width_value.saturating_sub(box_w);
    let stamp = format!("{}{to_note} ", clock(ts));
    let count_value = rows.len();

    for (i, row_value) in rows.into_iter().enumerate() {
        let pad = inner - row_value.line.width();

        // left of the box: the time on the 1st row, a copy button on the last
        let st = if i == 0 && stamp.width() < left {
            stamp.clone()
        } else {
            String::new()
        };

        let copy = i + 1 == count_value && st.width() + 6 < left;
        let used = st.width() + if copy { 5 } else { 0 };
        let mut spans = vec![Span::raw(" ".repeat(left - used)), Span::styled(st, theme::muted())];
        let mut copies: Vec<(u16, u16, String)> = Vec::new();

        if copy {
            spans.push(Span::styled("copy ", theme::accent()));
            copies.push(((left - 5) as u16, (left - 1) as u16, text.trim_end().to_string()));
        }

        spans.push(Span::styled("  ", bg));
        spans.extend(row_value.line.spans);
        spans.push(Span::styled(" ".repeat(pad + 2), bg));
        let shift = (left + 2) as u16;

        copies.extend(
            row_value
                .copies
                .into_iter()
                .map(|(agent_3, bytes, user)| (agent_3 + shift, bytes + shift, user)),
        );
        directory.rows.push(Row {
            wrap: row_value.wrap,
            line: Line::from(spans),
            links: row_value
                .links
                .into_iter()
                .map(|(agent_4, bytes_value, user_value)| (agent_4 + shift, bytes_value + shift, user_value))
                .collect(),
            copies,
        });
    }

    *last_agent = None;
}

fn draw_lines_item_prompt_from_to_body(row: &mut Row) {
    for text_value in &mut row.line.spans {
        if text_value.style.bg.is_none() {
            text_value.style = text_value.style.bg(theme::pal().surface1);
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
    pub(super) width_value: usize,
    pub(super) directory: &'render mut Drawn,
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

fn owner_message_rows(width_value: usize, text: &str) -> Vec<Row> {
    let (body, files) = attachments(text);
    let bw = (width_value * 3 / 4).max(20).min(width_value);

    // markdown as the agents' text (bold, lists, code); the hover bg under every span
    let mut rows = memo::markdown(body.trim_end(), bw.saturating_sub(4), &[]);

    link_markers(&mut rows, &files);

    for row in &mut rows {
        draw_lines_item_prompt_from_to_body(row);
    }

    let clip = |agent_value: &Attachment| {
        agent_value.marker.as_ref().map_or_else(
            || agent_value.path.clone(),
            |message| format!("{message} {}", agent_value.path),
        )
    };

    rows.extend(
        files
            .iter()
            .map(|agent_2| clip_row(agent_2, &clip(agent_2), theme::muted().bg(theme::pal().surface1), 0)),
    );

    rows
}
