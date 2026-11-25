use std::ops::ControlFlow;

use super::{Frame, Item, Line, Menu, Modifier, Paragraph, Rect, Span, Style, theme, thread_card};

impl Menu {
    pub(super) fn draw_quick(&mut self, frame: &mut Frame, body: Rect) {
        draw_quick_items(self, frame, body);
    }

    pub(super) fn draw_search(&mut self, frame: &mut Frame, body: Rect) {
        draw_message_search(self, frame, body);
    }

    pub(super) fn draw_tokens(&mut self, frame: &mut Frame, body: Rect) {
        let Some(directory) = &self.tokens.detail else {
            frame.render_widget(
                Paragraph::new(Span::styled("reading usage…", theme::muted())),
                Rect { height: 1, ..body },
            );
            return;
        };

        let names = self
            .roster
            .iter()
            .map(|agent| (agent.name.clone(), (agent.name.clone(), agent.color.clone())))
            .collect();
        let titles = self
            .list
            .cards
            .iter()
            .map(|character| (character.id.clone(), character.name()))
            .collect();
        let lines = crate::features::cost::detail_lines(
            directory,
            self.thread.as_deref(),
            &names,
            &titles,
            body.width as usize,
        );
        let max = (lines.len() as u16).saturating_sub(body.height);

        self.tokens.dscroll = self.tokens.dscroll.min(max);
        frame.render_widget(Paragraph::new(lines).scroll((self.tokens.dscroll, 0)), body);

        if max > 0 {
            let text = format!("{}↑ · {}↓", self.tokens.dscroll, max - self.tokens.dscroll);

            frame.render_widget(
                Paragraph::new(Span::styled(text, theme::muted())).right_aligned(),
                Rect {
                    y: body.y.saturating_sub(2),
                    height: 1,
                    ..body
                },
            );
        }
    }
}

fn draw_quick_items(widget_state: &mut Menu, frame: &mut Frame, body: Rect) {
    let kw = widget_state
        .items
        .iter()
        .map(|i| i.key.chars().count())
        .max()
        .unwrap_or(0);

    // per section: a blank row (after the 1st), its heading, its items
    let mut y = body.y;
    let mut last: Option<&str> = None;

    for (i, it) in widget_state.items.iter().enumerate() {
        if let ControlFlow::Break(()) = draw_quick_row(
            QuickRow {
                rows: &mut widget_state.layout.rows,
                cursor: widget_state.qcursor,
                body,
                kw,
            },
            frame,
            &mut y,
            &mut last,
            i,
            it,
        ) {
            break;
        }
    }
}

fn draw_quick_row(
    look: QuickRow<'_>,
    frame: &mut Frame<'_>,
    y: &mut u16,
    last: &mut Option<&str>,
    i: usize,
    it: &Item,
) -> ControlFlow<()> {
    let QuickRow { rows, cursor, body, kw } = look;

    if *last != Some(it.section) {
        if last.is_some() {
            *y += 1;
        }

        *last = Some(it.section);

        if *y < body.bottom() {
            frame.render_widget(
                Paragraph::new(Span::styled(
                    it.section.to_string(),
                    theme::muted().add_modifier(Modifier::BOLD),
                )),
                Rect {
                    y: *y,
                    height: 1,
                    ..body
                },
            );
        }

        *y += 1;
    }

    if *y >= body.bottom() {
        return ControlFlow::Break(());
    }

    let row_y = *y;

    *y += 1;
    let y = row_y;
    let area = Rect { y, height: 1, ..body };
    let hl = i == cursor;
    let (ks, ls) = Menu::row_style(hl);
    let label = thread_card::fit(&it.label, (area.width as usize).saturating_sub(kw + 4));
    let line = Line::from(vec![
        Span::styled(format!(" {:<kw$}  ", it.key), ks),
        Span::styled(label, ls),
    ]);

    frame.render_widget(
        Paragraph::new(line).style(if hl { theme::pill() } else { Style::default() }),
        area,
    );
    rows.push((area, i));

    ControlFlow::Continue(())
}

fn draw_message_search(widget_state: &mut Menu, frame: &mut Frame, body: Rect) {
    let fr = Rect { height: 1, ..body };

    frame.render_widget(
        Paragraph::new(Span::styled("› ", theme::accent())),
        Rect { width: 2, ..fr },
    );
    widget_state.search_state.search.render(
        frame,
        Rect {
            x: fr.x + 2,
            width: fr.width.saturating_sub(2),
            ..fr
        },
        true,
    );
    let list = Rect {
        y: body.y + 2,
        height: body.height.saturating_sub(2),
        ..body
    };
    let query = widget_state.search_state.search.text();

    let Some(msgs) = widget_state.search_state.msgs.as_ref() else {
        frame.render_widget(
            Paragraph::new(Span::styled("reading messages…", theme::muted())),
            Rect { height: 1, ..list },
        );
        return;
    };

    if widget_state.search_state.hits.is_empty() {
        draw_search_empty(frame, list, &query, msgs.len());
        return;
    }

    let count_value = widget_state.search_state.hits.len();

    widget_state.search_state.scursor = widget_state.search_state.scursor.min(count_value - 1);
    let fits = (list.height / Menu::HIT_H).max(1) as usize;

    if widget_state.search_state.scursor < widget_state.search_state.soffset {
        widget_state.search_state.soffset = widget_state.search_state.scursor;
    }

    if widget_state.search_state.scursor >= widget_state.search_state.soffset + fits {
        widget_state.search_state.soffset = widget_state.search_state.scursor + 1 - fits;
    }

    let mut rows: Vec<(Rect, usize)> = Vec::new();

    for (slot, &mi) in widget_state
        .search_state
        .hits
        .iter()
        .enumerate()
        .skip(widget_state.search_state.soffset)
        .take(fits)
    {
        let area = draw_search_result(widget_state, frame, list, slot, &msgs[mi], &query);

        rows.push((area, slot));
    }

    draw_search_count(widget_state, frame, body, count_value, fits, rows);
}

fn draw_search_count(
    widget_state: &mut Menu,
    frame: &mut Frame<'_>,
    body: Rect,
    count_value: usize,
    fits: usize,
    rows: Vec<(Rect, usize)>,
) {
    widget_state.layout.rows = rows;
    let more = count_value.saturating_sub(widget_state.search_state.soffset + fits);

    let cap = if count_value >= crate::domain::search::MAX {
        "+"
    } else {
        ""
    };

    let text = format!("{count_value}{cap} · {}↑ · {more}↓", widget_state.search_state.soffset);

    frame.render_widget(
        Paragraph::new(Span::styled(text, theme::muted())).right_aligned(),
        Rect {
            y: body.y + 1,
            height: 1,
            ..body
        },
    );
}

struct QuickRow<'rows> {
    rows: &'rows mut Vec<(Rect, usize)>,
    cursor: usize,
    body: Rect,
    kw: usize,
}

fn draw_search_result(
    widget_state: &Menu,
    frame: &mut Frame<'_>,
    list: Rect,
    slot: usize,
    message: &crate::domain::search::Msg,
    query: &str,
) -> Rect {
    let width_value = list.width as usize;
    let y = list.y + (slot - widget_state.search_state.soffset) as u16 * Menu::HIT_H;
    let area = Rect { y, height: 2, ..list };
    let hl = slot == widget_state.search_state.scursor;

    let bg = if hl {
        Style::default().bg(theme::pal().surface0)
    } else {
        Style::default()
    };

    let mark = if hl { "▌ " } else { "  " };
    let head = search_header(widget_state, message, width_value, hl);
    let (snip, marks) = crate::domain::search::snippet(message, query, width_value.saturating_sub(2));
    let mut body_spans = vec![Span::styled(mark, theme::accent())];

    body_spans.extend(crate::ui::fuzzy::spans(
        &snip,
        &marks,
        theme::text(),
        theme::accent_bold().add_modifier(Modifier::UNDERLINED),
    ));
    frame.render_widget(Paragraph::new(vec![head, Line::from(body_spans)]).style(bg), area);

    area
}

fn search_header(
    widget_state: &Menu,
    message: &crate::domain::search::Msg,
    width_value: usize,
    hl: bool,
) -> Line<'static> {
    let who = if message.from == "owner" {
        "you".to_string()
    } else {
        message.from.clone()
    };

    let here = widget_state.thread.as_deref() == Some(message.thread.as_str());

    let place = if here {
        "this thread".to_string()
    } else {
        message.title.clone()
    };

    let when = format!(
        "{} · {}",
        crate::domain::chat::clock(message.ts),
        thread_card::ago(message.ts)
    );
    let room = width_value.saturating_sub(who.chars().count() + when.chars().count() + 8);
    let mark = if hl { "▌ " } else { "  " };

    Line::from(vec![
        Span::styled(mark, theme::accent()),
        Span::styled(
            who,
            if message.from == "owner" {
                theme::text().add_modifier(Modifier::BOLD)
            } else {
                Style::default()
                    .fg(crate::domain::agents::color(
                        widget_state
                            .roster
                            .iter()
                            .find(|agent| agent.name == message.from)
                            .and_then(|agent_value| agent_value.color.as_deref()),
                    ))
                    .add_modifier(Modifier::BOLD)
            },
        ),
        Span::styled(format!(" · {} · ", thread_card::fit(&place, room)), theme::muted()),
        Span::styled(when, theme::muted()),
    ])
}

fn draw_search_empty(frame: &mut Frame<'_>, list: Rect, query: &str, count: usize) {
    let msg = if query.trim().is_empty() {
        format!("{count} messages · type to search")
    } else {
        "0 messages match".into()
    };

    frame.render_widget(
        Paragraph::new(Span::styled(msg, theme::muted())),
        Rect { height: 1, ..list },
    );
}
