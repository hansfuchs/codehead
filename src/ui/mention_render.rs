use super::{
    Agent, CARD_ROWS, Frame, Hit, Line, Mention, Modifier, Paragraph, Rect, Span, Style, TextArea, UnicodeWidthStr,
    cards, dropdown, order, theme, visible,
};
impl Mention {
    /// Dropdown right above the `@` of TA (TA rendered this frame), inside BOUNDS. Below it when 0
    /// room above. Replace mode: the 2nd dropdown beside it.
    pub(crate) fn render(&mut self, frame: &mut Frame, ta: &TextArea, agents: &[Agent], bounds: Rect) {
        self.rows.clear();
        self.sync(ta);

        let Some((at, query)) = self.query(ta) else {
            return;
        };

        let groups = visible(agents, &query);
        let shown = order(&groups);

        if shown.is_empty() {
            return;
        }

        let Some(cursor_pos) = ta.screen_pos(at) else { return };

        self.cursor = self.cursor.min(shown.len() - 1);
        let cur = shown[self.cursor];

        let grid = mention_grid(self, &groups, agents);
        let toggle = !self.replaceable.is_empty();
        let width = (grid.card_width + 3).max(if toggle { 22 } else { 16 });

        // as tall as the cards need, cut to the room of the screen
        let Some(area) = dropdown::place_max(
            Rect {
                x: cursor_pos.x,
                y: cursor_pos.y,
                width: 1,
                height: 1,
            },
            width,
            grid.height,
            usize::from(bounds.height),
            bounds,
            true,
        ) else {
            return;
        };

        let card = draw_cards(self, frame, agents, &grid, cur, area);

        // add │ replace in the bottom border
        if toggle {
            render_can(frame, agents, cur, area, self.replace, self);
        }

        // the 2nd dropdown: the agents of the thread the current card can replace
        if let Some(target) = self.replace {
            draw_replacements(
                self,
                frame,
                &MentionFrame {
                    agents,
                    cur,
                    area,
                    bounds,
                    card,
                },
                target,
            );
        }
    }
}

fn render_can(
    frame: &mut Frame<'_>,
    agents: &[Agent],
    cur: usize,
    area: Rect,
    replace: Option<usize>,
    mention: &mut Mention,
) {
    let can = !mention.targets(&agents[cur].name).is_empty();

    let on = |active: bool| {
        if active {
            theme::pill().add_modifier(Modifier::BOLD)
        } else {
            theme::muted()
        }
    };

    let spans = vec![
        Span::styled(" ‹ ", theme::accent()),
        Span::styled(" add ", on(replace.is_none())),
        Span::styled(" ", theme::muted()),
        Span::styled(" replace ", if can { on(replace.is_some()) } else { theme::line() }),
        Span::styled(" › ", theme::accent()),
    ];
    let lw = spans.iter().map(|span| span.content.width()).sum::<usize>() as u16;

    if area.width > lw + 2 {
        let row = Rect {
            x: area.right() - 1 - lw,
            y: area.bottom() - 1,
            width: lw,
            height: 1,
        };

        frame.render_widget(
            Paragraph::new(Line::from(spans)).style(Style::default().bg(theme::pal().on_accent)),
            row,
        );
    }
}

struct MentionFrame<'agents> {
    agents: &'agents [Agent],
    cur: usize,
    area: Rect,
    bounds: Rect,
    card: Option<Rect>,
}

fn card_text(mention: &Mention, agent: &Agent) -> [String; 3] {
    [
        agent.name.clone(),
        cards::tech(agent),
        if mention.replaceable.contains(&agent.name) {
            "on this thread".into()
        } else {
            String::new()
        },
    ]
}

fn mention_grid(mention: &Mention, groups: &[(String, Vec<usize>)], agents: &[Agent]) -> cards::Grid {
    let text_w = cards::text_width(
        order(groups)
            .iter()
            .flat_map(|i| card_text(mention, &agents[*i]))
            .collect::<Vec<_>>()
            .iter(),
    );
    let card_w = cards::card_width(text_w, false) + 1;

    cards::grid(groups, card_w, CARD_ROWS, card_w)
}

fn draw_cards(
    mention: &mut Mention,
    frame: &mut Frame,
    agents: &[Agent],
    group: &cards::Grid,
    cur: usize,
    area: Rect,
) -> Option<Rect> {
    let view = (area.height as usize).saturating_sub(2);
    let top = group.places.iter().find(|pl| pl.0 == cur).map_or(0, |pl| pl.2);

    mention.offset = cards::scroll(group, top, CARD_ROWS, mention.offset, view);
    let off = mention.offset;
    let (inner, body) = agent_frame(frame, area, off, view, group.height, mention.replace.is_none());
    let replace = mention.replace;
    let mut cur_rect = None;

    let drawn = cards::draw_grid(frame, body, group, CARD_ROWS, off, |frame, rr, i| {
        let agent = &agents[i];
        let hl = i == cur;
        let card = Rect {
            height: CARD_ROWS,
            width: body.width,
            ..rr
        };

        draw_mention_card(
            frame,
            card,
            agent,
            &card_text(mention, agent),
            inner.x,
            hl,
            replace.is_some(),
        );
    });

    for (rr, i) in drawn {
        if i == cur {
            cur_rect = Some(rr);
        }

        mention.rows.push((
            Rect {
                height: CARD_ROWS,
                width: body.width,
                ..rr
            },
            Hit::Card(agents[i].name.clone()),
        ));
    }

    cur_rect
}

fn draw_replacements(mention: &mut Mention, frame: &mut Frame, context: &MentionFrame, target: usize) {
    let targets = mention.targets(&context.agents[context.cur].name);

    if targets.is_empty() {
        return;
    }

    let target = target.min(targets.len() - 1);

    mention.replace = Some(target);
    let tw = (targets.iter().map(|name| name.width()).max().unwrap_or(0) + 6).max(18) as u16;
    let row_y = context.card.map_or(context.area.y + 1, |cr| cr.y);

    let Some(sr) = dropdown::place_beside(context.area, row_y, tw, targets.len(), context.bounds, context.area) else {
        return;
    };

    let shown = (sr.height as usize).saturating_sub(2);
    let soff = dropdown::fit(0, target, targets.len(), shown);
    let sin = dropdown::frame(
        frame,
        sr,
        &format!("{} replaces", context.agents[context.cur].name),
        "",
        dropdown::FrameState {
            up: soff > 0,
            down: soff + shown < targets.len(),
            active: true,
        },
    );

    for (i, name) in targets.iter().enumerate().skip(soff).take(shown) {
        let row = Rect {
            y: sin.y + (i - soff) as u16,
            height: 1,
            ..sin
        };
        let hl = i == target;

        let color = context
            .agents
            .iter()
            .find(|agent| agent.name == *name)
            .map_or(theme::pal().accent, |agent| {
                crate::domain::agents::color(agent.color.as_deref())
            });

        let st = if hl {
            theme::pill().add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme::pal().text)
        };

        let dot = if hl {
            Style::default().fg(theme::pal().on_accent)
        } else {
            Style::default().fg(color)
        };

        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(" ● ", dot),
                Span::styled(format!("@{name}"), st),
            ]))
            .style(st),
            row,
        );
        mention.rows.push((row, Hit::Target(name.clone())));
    }
}

fn draw_mention_card(
    frame: &mut Frame,
    card: Rect,
    agent: &Agent,
    text: &[String; 3],
    border_x: u16,
    hl: bool,
    replacing: bool,
) {
    let bg = hl.then_some(theme::pal().surface0);

    cards::draw(
        frame,
        card,
        agent,
        text,
        cards::Look {
            cur: hl,
            lit: false,
            border: None,
            title: None,
            bg,
        },
    );

    let symbol = if replacing { "┃" } else { "▌" };

    if hl {
        let bar = Style::default().fg(theme::pal().accent).bg(theme::pal().on_accent);

        for y in card.y..card.bottom() {
            frame.buffer_mut()[(border_x, y)].set_symbol(symbol).set_style(bar);
        }
    }
}

fn agent_frame(
    frame: &mut Frame,
    area: Rect,
    off: usize,
    view: usize,
    virtual_height: usize,
    active: bool,
) -> (Rect, Rect) {
    let inner = dropdown::frame(
        frame,
        area,
        "agents",
        "",
        dropdown::FrameState {
            up: off > 0,
            down: off + view < virtual_height,
            active,
        },
    );
    let body = Rect {
        x: inner.x + 1,
        width: inner.width.saturating_sub(1),
        ..inner
    };

    (inner, body)
}
