use super::{
    Block, Card, Clear, Duration, EXPANDED_WIDTH, Frame, Instant, Line, MAX_COST_BOX, MIN_COST_BOX, MIN_COST_WIDTH,
    Msg, Pane, Paragraph, Rect, SQUEEZED_COST_BOX, Span, View, cards_w, theme,
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
    pub(super) fn draw_header_cost(&mut self, frame: &mut Frame, header: Rect, top: u16) {
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
            draw_cost(self, frame, cost, top);
        } else {
            // 0 box: 0 click target (the chat takes the clicks)
            self.cost_box.rect = Rect::default();
        }
    }
}

fn draw_cost(view: &mut View, frame: &mut Frame<'_>, cost: Rect, top: u16) {
    if view.cost_box.read_at.elapsed() > Duration::from_secs(3) {
        read_cost(view);
    }

    // every thread's cost read ahead: the box expands at its full height at once
    if view.cost_box.wants_all() {
        read_all(view);
    }

    view.cost_box.rect = cost;

    // expanded: drawn after the prompt (its frame shares the prompt's top line)
    if view.cost_box.expanded {
        view.cost_box.anchor = Some((cost, top));
        return;
    }

    view.cost_box.anchor = None;
    let next = next_message(view);

    let Some(snap) = view.cost_box.snap.as_ref() else {
        // a new thread: the last box until this one's numbers come (then it morphs)
        crate::features::cost::draw_last(frame, cost);
        return;
    };

    let show = crate::features::cost::Show {
        next: &next,
        expanded: None,
    };

    crate::features::cost::draw_in(frame, cost, snap, show);
}

fn next_message(view: &View) -> crate::features::cost::Next {
    crate::features::cost::Next {
        to: view.recipients(),
        chars: view.prompt.input.text().chars().count(),
    }
}

impl View {
    /// The expanded tokens box over the chat, up from the compact box's place (after the prompt).
    pub(super) fn draw_cost_expanded(&mut self, frame: &mut Frame) {
        let Some((cost, top)) = self.cost_box.anchor.take() else {
            return;
        };

        let next = next_message(self);
        let empty = crate::features::cost::Snap::default();
        let snap = self.cost_box.snap.as_ref().unwrap_or(&empty);
        let show = crate::features::cost::Show {
            next: &next,
            expanded: Some(crate::features::cost::Expanded {
                all: self.cost_box.all.as_ref(),
                scroll: self.cost_box.scroll,
            }),
        };
        let outer = draw_expanded(frame, cost, top, snap, show);
        let rows = crate::features::cost::expanded_rows(snap, show, outer.width.saturating_sub(4));

        self.cost_box.scroll = self
            .cost_box
            .scroll
            .min(rows.saturating_sub(outer.height.saturating_sub(2)));
        self.cost_box.rect = outer;
    }
}

/// The expanded box: from the compact box's bottom up to TOP at most, over the chat, at least
/// `EXPANDED_WIDTH` wide (the screen allows), framed. Its outer rect.
fn draw_expanded(
    frame: &mut Frame<'_>,
    cost: Rect,
    top: u16,
    snap: &crate::features::cost::Snap,
    show: crate::features::cost::Show,
) -> Rect {
    let right = cost.right() + 2;
    let width = (cost.width + 4)
        .max(EXPANDED_WIDTH)
        .min(right.saturating_sub(frame.area().x));
    let inner_width = width.saturating_sub(4);

    // the bottom line: the prompt's top line, under the compact rows
    let bottom = cost.bottom() + 1;
    let height = (crate::features::cost::expanded_rows(snap, show, inner_width) + 2).min(bottom.saturating_sub(top));
    let outer = Rect {
        x: right - width,
        y: bottom - height,
        width,
        height,
    };
    let block = Block::bordered()
        .border_style(theme::line())
        .title(Span::styled(" tokens ", theme::muted()))
        .title_bottom(Line::styled(" ctrl+o · click: close ", theme::muted()).right_aligned());
    let inner = Rect {
        x: outer.x + 2,
        width: inner_width,
        ..block.inner(outer)
    };

    frame.render_widget(Clear, outer);
    frame.render_widget(block, outer);
    crate::features::cost::draw_in(frame, inner, snap, show);

    outer
}

fn read_cost(view: &mut View) {
    view.cost_box.read_at = Instant::now();
    let (id, tx) = (view.id.clone(), view.tx.clone());

    std::thread::spawn(move || {
        let _ = tx.send(Msg::Cost(Box::new(crate::features::cost::snapshot(&id))));
    });
}

/// Every thread's cost, off the UI thread (every log: up to MBs).
fn read_all(view: &mut View) {
    view.cost_box.all_reading = true;
    view.cost_box.all_due = false;
    let tx = view.tx.clone();

    std::thread::spawn(move || {
        let _ = tx.send(Msg::AllThreads(Box::new(crate::features::cost::all_threads())));
    });
}
