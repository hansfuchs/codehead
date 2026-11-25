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
        let Some(detail) = &self.tokens.detail else {
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
            .map(|card| (card.id.clone(), card.name()))
            .collect();
        let lines =
            crate::features::cost::detail_lines(detail, self.thread.as_deref(), &names, &titles, body.width as usize);
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

fn draw_quick_items(menu: &mut Menu, frame: &mut Frame, body: Rect) {
    let key_width = menu
        .items
        .iter()
        .map(|item| item.key.chars().count())
        .max()
        .unwrap_or(0);

    // per section: a blank row (after the 1st), its heading, its items
    let mut y = body.y;
    let mut last: Option<&str> = None;

    for (i, item) in menu.items.iter().enumerate() {
        if let ControlFlow::Break(()) = draw_quick_row(
            QuickRow {
                rows: &mut menu.layout.rows,
                cursor: menu.qcursor,
                body,
                key_width,
            },
            frame,
            &mut y,
            &mut last,
            i,
            item,
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
    item: &Item,
) -> ControlFlow<()> {
    let QuickRow {
        rows,
        cursor,
        body,
        key_width,
    } = look;

    if *last != Some(item.section) {
        if last.is_some() {
            *y += 1;
        }

        *last = Some(item.section);

        if *y < body.bottom() {
            frame.render_widget(
                Paragraph::new(Span::styled(
                    item.section.to_string(),
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
    let (key_style, label_style) = Menu::row_style(hl);
    let label = thread_card::fit(&item.label, (area.width as usize).saturating_sub(key_width + 4));
    let line = Line::from(vec![
        Span::styled(format!(" {:<key_width$}  ", item.key), key_style),
        Span::styled(label, label_style),
    ]);

    frame.render_widget(
        Paragraph::new(line).style(if hl { theme::pill() } else { Style::default() }),
        area,
    );
    rows.push((area, i));

    ControlFlow::Continue(())
}

fn draw_message_search(menu: &mut Menu, frame: &mut Frame, body: Rect) {
    let input_row = Rect { height: 1, ..body };

    frame.render_widget(
        Paragraph::new(Span::styled("› ", theme::accent())),
        Rect { width: 2, ..input_row },
    );
    menu.search_state.search.render(
        frame,
        Rect {
            x: input_row.x + 2,
            width: input_row.width.saturating_sub(2),
            ..input_row
        },
        true,
    );
    let list = Rect {
        y: body.y + 2,
        height: body.height.saturating_sub(2),
        ..body
    };
    let query = menu.search_state.search.text();

    let Some(msgs) = menu.search_state.msgs.as_ref() else {
        frame.render_widget(
            Paragraph::new(Span::styled("reading messages…", theme::muted())),
            Rect { height: 1, ..list },
        );
        return;
    };

    if menu.search_state.hits.is_empty() {
        draw_search_empty(frame, list, &query, msgs.len());
        return;
    }

    let count = menu.search_state.hits.len();

    menu.search_state.scursor = menu.search_state.scursor.min(count - 1);
    let fits = (list.height / Menu::HIT_H).max(1) as usize;

    if menu.search_state.scursor < menu.search_state.soffset {
        menu.search_state.soffset = menu.search_state.scursor;
    }

    if menu.search_state.scursor >= menu.search_state.soffset + fits {
        menu.search_state.soffset = menu.search_state.scursor + 1 - fits;
    }

    let mut rows: Vec<(Rect, usize)> = Vec::new();

    for (slot, &hit) in menu
        .search_state
        .hits
        .iter()
        .enumerate()
        .skip(menu.search_state.soffset)
        .take(fits)
    {
        let area = draw_search_result(menu, frame, list, slot, &msgs[hit], &query);

        rows.push((area, slot));
    }

    draw_search_count(menu, frame, body, count, fits, rows);
}

fn draw_search_count(
    menu: &mut Menu,
    frame: &mut Frame<'_>,
    body: Rect,
    count: usize,
    fits: usize,
    rows: Vec<(Rect, usize)>,
) {
    menu.layout.rows = rows;
    let more = count.saturating_sub(menu.search_state.soffset + fits);

    let cap = if count >= crate::domain::search::MAX { "+" } else { "" };

    let text = format!("{count}{cap} · {}↑ · {more}↓", menu.search_state.soffset);

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
    key_width: usize,
}

fn draw_search_result(
    menu: &Menu,
    frame: &mut Frame<'_>,
    list: Rect,
    slot: usize,
    message: &crate::domain::search::Msg,
    query: &str,
) -> Rect {
    let width = list.width as usize;
    let y = list.y + (slot - menu.search_state.soffset) as u16 * Menu::HIT_H;
    let area = Rect { y, height: 2, ..list };
    let hl = slot == menu.search_state.scursor;

    let bg = if hl {
        Style::default().bg(theme::pal().surface0)
    } else {
        Style::default()
    };

    let mark = if hl { "▌ " } else { "  " };
    let head = search_header(menu, message, width, hl);
    let (snip, marks) = crate::domain::search::snippet(message, query, width.saturating_sub(2));
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

fn search_header(menu: &Menu, message: &crate::domain::search::Msg, width: usize, hl: bool) -> Line<'static> {
    let who = if message.from == "owner" {
        "you".to_string()
    } else {
        message.from.clone()
    };

    let here = menu.thread.as_deref() == Some(message.thread.as_str());

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
    let room = width.saturating_sub(who.chars().count() + when.chars().count() + 8);
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
                        menu.roster
                            .iter()
                            .find(|agent| agent.name == message.from)
                            .and_then(|agent| agent.color.as_deref()),
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
