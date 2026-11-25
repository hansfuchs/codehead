use super::{
    Act, Block, BorderType, Borders, Clear, Focus, Frame, Line, Mode, Modifier, Paragraph, Rect, RepoTab, Span,
    StatusTab, Style, ThreadList, View, theme, thread_card,
};

impl ThreadList {
    pub(super) fn draw_cards(&mut self, frame: &mut Frame, list: Rect, tick: usize) {
        draw_cards_thread_list(self, frame, list, tick);
    }

    fn draw_card(
        frame: &mut Frame<'_>,
        list: Rect,
        context: CardRow<'_, '_, '_, '_>,
        vis: &[usize],
        card_height: impl Fn(usize) -> u16,
        ys: &[u16],
        notes: &[crate::domain::notifications::Note],
    ) {
        let CardRow {
            tick,
            focused,
            pos,
            cards,
            offset,
            cursor,
            here,
            mode,
            selected,
            rows,
        } = context;

        let rect = Rect {
            y: list.y + ys[pos] - offset,
            height: card_height(pos),
            ..list
        };
        let card = &cards[vis[pos]];

        thread_card::draw_card(
            frame.buffer_mut(),
            rect,
            card,
            thread_card::Look {
                cur: pos == cursor && focused,
                here: here == Some(card.id.as_str()),
                age: true,
            },
            tick,
        );
        crate::ui::notification_badge::card_badge(frame.buffer_mut(), rect, notes, &card.id);

        if mode == Mode::Manage {
            draw_card_mark(frame, selected, rect, card);
        }

        rows.push((rect, pos));
    }

    /// The actions dropdown: right of the list, below the cursor's card.
    pub(super) fn draw_actions(&mut self, frame: &mut Frame, list: Rect) {
        draw_actions_thread_list(self, frame, list);
    }
}

fn draw_card_mark(
    frame: &mut Frame<'_>,
    selected: &std::collections::HashSet<String>,
    rect: Rect,
    card: &thread_card::Card,
) {
    let on = selected.contains(&card.id);

    let mark = if on {
        Span::styled(" ◼ ", theme::accent_bold())
    } else {
        Span::styled(" ◻ ", theme::line())
    };

    frame.render_widget(
        Paragraph::new(mark),
        Rect {
            x: rect.x + 1,
            y: rect.y,
            width: 3,
            height: 1,
        },
    );
}

fn draw_actions_list(
    frame: &mut Frame<'_>,
    cur: usize,
    acts: &[Act],
    area: Rect,
    title: String,
    thread_list: &mut ThreadList,
) {
    let block = Block::new()
        .borders(Borders::ALL)
        .border_type(BorderType::Plain)
        .border_style(theme::accent())
        .title(Span::styled(title, theme::accent_bold()))
        .style(theme::text());
    let inner = block.inner(area);

    frame.render_widget(block, area);

    for (i, act) in acts.iter().enumerate() {
        let row = Rect {
            y: inner.y + i as u16,
            height: 1,
            ..inner
        };

        if row.y >= inner.bottom() {
            break;
        }

        let hl = i == cur.min(acts.len() - 1);
        let style = if hl { theme::pill() } else { Style::default() };
        let line = Line::from(vec![
            Span::styled(
                format!(" {:<6}", act.key()),
                if hl {
                    style.add_modifier(Modifier::BOLD)
                } else {
                    theme::accent()
                },
            ),
            Span::styled(act.label().to_string(), if hl { style } else { theme::text() }),
        ]);

        frame.render_widget(Paragraph::new(line).style(style), row);
        thread_list.hit_regions.act_rows.push((row, *act));
    }
}

fn draw_cards_msg(frame: &mut Frame<'_>, list: Rect, thread_list: &mut ThreadList) {
    let msg = if thread_list.card_reads.loading.is_some() && thread_list.cards.is_empty() {
        "reading threads…".to_string()
    } else if !thread_list.filter.text().trim().is_empty() {
        "0 matches".into()
    } else if thread_list.view == View::Archived {
        "0 archived threads".into()
    } else if thread_list.stat_tab() != StatusTab::All {
        format!("0 {} threads", thread_list.stat_tab().label())
    } else if thread_list.repo_tab() != RepoTab::All {
        "0 threads in this repo".into()
    } else {
        "0 threads".into()
    };

    frame.render_widget(
        Paragraph::new(Span::styled(msg, theme::muted())),
        Rect { height: 1, ..list },
    );
}

struct CardRow<'cards, 'here, 'selected, 'rows> {
    tick: usize,
    focused: bool,
    pos: usize,
    cards: &'cards [crate::ui::thread_card::Card],
    offset: u16,
    cursor: usize,
    here: Option<&'here str>,
    mode: Mode,
    selected: &'selected std::collections::HashSet<String>,
    rows: &'rows mut Vec<(Rect, usize)>,
}

fn draw_cards_thread_list(state: &mut ThreadList, frame: &mut Frame, list: Rect, tick: usize) {
    let vis = state.visible();

    if vis.is_empty() {
        draw_cards_msg(frame, list, state);
        return;
    }

    state.cursor = state.cursor.min(vis.len() - 1);
    let cards = &state.cards;
    let card_height = |pos: usize| thread_card::card_h(&cards[vis[pos]]);

    // virtual y of each card
    let mut ys = Vec::with_capacity(vis.len());
    let mut y = 0u16;

    for pos in 0..vis.len() {
        ys.push(y);
        y += card_height(pos);
    }

    let total = y;
    let top = ys[state.cursor];
    let bottom = top + card_height(state.cursor);

    if top < state.offset {
        state.offset = top;
    } else if bottom > state.offset + list.height {
        state.offset = bottom.saturating_sub(list.height);
    }

    state.offset = state.offset.min(total.saturating_sub(list.height));
    let notes = crate::domain::notifications::unseen(state.here.as_deref());
    let focused = state.focus == Focus::List && !state.filtering;
    let fits = |pos: usize| ys[pos] >= state.offset && ys[pos] + card_height(pos) <= state.offset + list.height;

    for pos in (0..vis.len()).filter(|&pos| fits(pos)) {
        ThreadList::draw_card(
            frame,
            list,
            CardRow {
                tick,
                focused,
                pos,
                cards: &state.cards,
                offset: state.offset,
                cursor: state.cursor,
                here: state.here.as_deref(),
                mode: state.mode,
                selected: &state.sel,
                rows: &mut state.hit_regions.rows,
            },
            &vis,
            card_height,
            &ys,
            &notes,
        );
    }

    state.card_reads.fx.apply(frame, list, state.card_reads.version);
    let above = (0..vis.len()).filter(|&pos| ys[pos] < state.offset).count();
    let below = (0..vis.len())
        .filter(|&pos| ys[pos] + card_height(pos) > state.offset + list.height)
        .count();

    draw_overflow(frame, list, above, below);
}

fn draw_actions_thread_list(state: &mut ThreadList, frame: &mut Frame, list: Rect) {
    let Some(cur) = state.job_state.actions else { return };
    let acts = state.acts();

    if acts.is_empty() {
        state.job_state.actions = None;
        return;
    }

    let width = acts
        .iter()
        .map(|act| act.label().chars().count() as u16 + 10)
        .max()
        .unwrap_or(20)
        .min(list.width);
    let height = acts.len() as u16 + 2;
    let anchor = state
        .hit_regions
        .rows
        .iter()
        .find(|(_, pos)| *pos == state.cursor)
        .map_or(list.y, |(rect, _)| rect.y + 2);
    let y = anchor.min(list.bottom().saturating_sub(height)).max(list.y);
    let area = Rect {
        x: list.right().saturating_sub(width + 1),
        y,
        width,
        height: height.min(list.height),
    };

    frame.render_widget(Clear, area);
    let count = state.targets().len();

    let title = if count > 1 {
        format!(" {count} threads ")
    } else {
        " actions ".into()
    };

    draw_actions_list(frame, cur, &acts, area, title, state);
}

fn draw_overflow(frame: &mut Frame, list: Rect, above: usize, below: usize) {
    if above + below > 0 {
        let area = Rect {
            y: list.y.saturating_sub(1),
            height: 1,
            ..list
        };

        frame.render_widget(
            Paragraph::new(Span::styled(format!("{above}↑ · {below}↓"), theme::muted())).right_aligned(),
            area,
        );
    }
}
