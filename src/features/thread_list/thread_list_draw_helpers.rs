use super::{
    Act, Block, BorderType, Borders, Clear, Focus, Frame, Line, Mode, Modifier, Paragraph, Rect, RepoTab, Span,
    StatusTab, Style, ThreadList, View, theme, thread_card,
};

impl ThreadList {
    pub(super) fn draw_cards(&mut self, frame: &mut Frame, list: Rect, tick: usize) {
        draw_cards_thread_list(self, frame, list, tick);
    }

    fn draw_cards_rect(
        frame: &mut Frame<'_>,
        list: Rect,
        context: CardState<'_, '_, '_, '_>,
        vis: &[usize],
        height_value: impl Fn(usize) -> u16,
        ys: &[u16],
        notes: &[crate::domain::notifications::Note],
    ) {
        let CardState {
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
            height: height_value(pos),
            ..list
        };
        let character = &cards[vis[pos]];

        thread_card::draw_card(
            frame.buffer_mut(),
            rect,
            character,
            thread_card::Look {
                cur: pos == cursor && focused,
                here: here == Some(character.id.as_str()),
                age: true,
            },
            tick,
        );
        crate::ui::notification_badge::card_badge(frame.buffer_mut(), rect, notes, &character.id);

        if mode == Mode::Manage {
            draw_cards_rect_mark(frame, selected, rect, character);
        }

        rows.push((rect, pos));
    }

    /// The actions dropdown: right of the list, below the cursor's card.
    pub(super) fn draw_actions(&mut self, frame: &mut Frame, list: Rect) {
        draw_actions_thread_list(self, frame, list);
    }
}

fn draw_cards_rect_mark(
    frame: &mut Frame<'_>,
    selected: &std::collections::HashSet<String>,
    rect: Rect,
    character: &thread_card::Card,
) {
    let on = selected.contains(&character.id);

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

fn finish_draw_actions(
    frame: &mut Frame<'_>,
    cur: usize,
    acts: &[Act],
    area: Rect,
    title: String,
    view_state: &mut ThreadList,
) {
    let block = Block::new()
        .borders(Borders::ALL)
        .border_type(BorderType::Plain)
        .border_style(theme::accent())
        .title(Span::styled(title, theme::accent_bold()))
        .style(theme::text());
    let inner = block.inner(area);

    frame.render_widget(block, area);

    for (i, agent_value) in acts.iter().enumerate() {
        let rr = Rect {
            y: inner.y + i as u16,
            height: 1,
            ..inner
        };

        if rr.y >= inner.bottom() {
            break;
        }

        let hl = i == cur.min(acts.len() - 1);
        let st = if hl { theme::pill() } else { Style::default() };
        let line = Line::from(vec![
            Span::styled(
                format!(" {:<6}", agent_value.key()),
                if hl {
                    st.add_modifier(Modifier::BOLD)
                } else {
                    theme::accent()
                },
            ),
            Span::styled(agent_value.label().to_string(), if hl { st } else { theme::text() }),
        ]);

        frame.render_widget(Paragraph::new(line).style(st), rr);
        view_state.hit_regions.act_rows.push((rr, *agent_value));
    }
}

fn draw_cards_msg(frame: &mut Frame<'_>, list: Rect, view_state: &mut ThreadList) {
    let msg = if view_state.card_reads.loading.is_some() && view_state.cards.is_empty() {
        "reading threads…".to_string()
    } else if !view_state.filter.text().trim().is_empty() {
        "0 matches".into()
    } else if view_state.view == View::Archived {
        "0 archived threads".into()
    } else if view_state.stat_tab() != StatusTab::All {
        format!("0 {} threads", view_state.stat_tab().label())
    } else if view_state.repo_tab() != RepoTab::All {
        "0 threads in this repo".into()
    } else {
        "0 threads".into()
    };

    frame.render_widget(
        Paragraph::new(Span::styled(msg, theme::muted())),
        Rect { height: 1, ..list },
    );
}

struct CardState<'reference_0, 'reference_1, 'reference_2, 'reference_3> {
    tick: usize,
    focused: bool,
    pos: usize,
    cards: &'reference_0 [crate::ui::thread_card::Card],
    offset: u16,
    cursor: usize,
    here: Option<&'reference_1 str>,
    mode: Mode,
    selected: &'reference_2 std::collections::HashSet<String>,
    rows: &'reference_3 mut Vec<(Rect, usize)>,
}

fn draw_cards_thread_list(state: &mut ThreadList, frame: &mut Frame, list: Rect, tick: usize) {
    let vis = state.visible();

    if vis.is_empty() {
        draw_cards_msg(frame, list, state);
        return;
    }

    state.cursor = state.cursor.min(vis.len() - 1);
    let cards = &state.cards;
    let height_value = |pos: usize| thread_card::card_h(&cards[vis[pos]]);

    // virtual y of each card
    let mut ys = Vec::with_capacity(vis.len());
    let mut y = 0u16;

    for pos in 0..vis.len() {
        ys.push(y);
        y += height_value(pos);
    }

    let total = y;
    let top = ys[state.cursor];
    let bottom = top + height_value(state.cursor);

    if top < state.offset {
        state.offset = top;
    } else if bottom > state.offset + list.height {
        state.offset = bottom.saturating_sub(list.height);
    }

    state.offset = state.offset.min(total.saturating_sub(list.height));
    let notes = crate::domain::notifications::unseen(state.here.as_deref());
    let focused = state.focus == Focus::List && !state.filtering;
    let fits = |pos: usize| ys[pos] >= state.offset && ys[pos] + height_value(pos) <= state.offset + list.height;

    for pos in (0..vis.len()).filter(|&path| fits(path)) {
        ThreadList::draw_cards_rect(
            frame,
            list,
            CardState {
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
            height_value,
            &ys,
            &notes,
        );
    }

    state.card_reads.fx.apply(frame, list, state.card_reads.version);
    let above = (0..vis.len())
        .filter(|&path_value| ys[path_value] < state.offset)
        .count();
    let below = (0..vis.len())
        .filter(|&path_2| ys[path_2] + height_value(path_2) > state.offset + list.height)
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

    let width_value = acts
        .iter()
        .map(|agent| agent.label().chars().count() as u16 + 10)
        .max()
        .unwrap_or(20)
        .min(list.width);
    let height_value = acts.len() as u16 + 2;
    let anchor = state
        .hit_regions
        .rows
        .iter()
        .find(|(_, path)| *path == state.cursor)
        .map_or(list.y, |(result, _)| result.y + 2);
    let y = anchor.min(list.bottom().saturating_sub(height_value)).max(list.y);
    let area = Rect {
        x: list.right().saturating_sub(width_value + 1),
        y,
        width: width_value,
        height: height_value.min(list.height),
    };

    frame.render_widget(Clear, area);
    let count_value = state.targets().len();

    let title = if count_value > 1 {
        format!(" {count_value} threads ")
    } else {
        " actions ".into()
    };

    finish_draw_actions(frame, cur, &acts, area, title, state);
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
