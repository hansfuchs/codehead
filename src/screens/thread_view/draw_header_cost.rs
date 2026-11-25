use super::{
    Card, Duration, Frame, Instant, Line, MAX_COST_BOX, MIN_COST_BOX, MIN_COST_WIDTH, Msg, Pane, Paragraph, Rect,
    SQUEEZED_COST_BOX, Span, View, cards_w, theme,
};

impl View {
    pub(super) fn draw_prompt_status(&mut self, frame: &mut Frame, title: Rect) {
        let note = if self.stream.back > 0 && self.panes.pane == Pane::Chat {
            format!("↑{} · esc: live ", self.stream.back)
        } else {
            String::new()
        };

        let mut low: Vec<Span<'static>> = Vec::new();

        if !note.is_empty() {
            low.push(Span::styled(note, theme::muted()));
        }

        if let Some((new, old)) = &self.prompt.replace {
            low.push(Span::styled(format!(" on send: {new} replaces {old} "), theme::pill()));
        }

        frame.render_widget(
            Paragraph::new(Line::from(low)),
            Rect {
                x: title.x + 2,
                width: title.width.saturating_sub(2),
                ..title
            },
        );
    }
    pub(super) fn draw_header_cost(&mut self, frame: &mut Frame, header: Rect) {
        // header: agents box + tokens box (the cost pane inside)
        let show_cost = header.height > 0 && header.width >= MIN_COST_WIDTH;

        let cost = if show_cost {
            let cards: Vec<Card> = self.agents.order.iter().map(|name| self.card(name)).collect();
            let (all, first) = (cards_w(&cards), cards_w(&cards[..cards.len().min(1)]));
            let room = (header.width + 1).saturating_sub(all);
            let min = MIN_COST_BOX
                .min((header.width + 1).saturating_sub(first))
                .max(SQUEEZED_COST_BOX);
            let width = room.clamp(min, MAX_COST_BOX).min(header.width);

            Rect {
                x: header.right() - width + 2,
                y: header.y + 1,
                width: width.saturating_sub(4),
                height: header.height.saturating_sub(2),
            }
        } else {
            Rect { width: 0, ..header }
        };

        self.draw_header(frame, header, cost);

        if show_cost {
            draw_cost(self, frame, cost);
        }
    }
}

fn draw_cost(view: &mut View, frame: &mut Frame<'_>, cost: Rect) {
    if view.cost_at.elapsed() > Duration::from_secs(3) {
        read_cost(view);
    }

    if let Some(snap) = &view.cost {
        let next = crate::features::cost::Next {
            to: view.recipients(),
            chars: view.prompt.input.text().chars().count(),
        };

        crate::features::cost::draw_in(frame, cost, snap, view.cost_at.elapsed(), &next);
    } else {
        // a new thread: the last box until this one's numbers come (then it morphs)
        crate::features::cost::draw_last(frame, cost);
    }
}

fn read_cost(view: &mut View) {
    view.cost_at = Instant::now();
    let (id, tx) = (view.id.clone(), view.tx.clone());

    std::thread::spawn(move || {
        let _ = tx.send(Msg::Cost(Box::new(crate::features::cost::snapshot(&id))));
    });
}
