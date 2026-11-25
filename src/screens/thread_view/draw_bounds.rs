use super::{Frame, Pane, Paragraph, Rect, SPLIT_MIN, View, theme};

impl View {
    pub(super) fn prepare_draw(&mut self) {
        // a reply note's jump: the changes view on its comment, the thread unfolded
        if let Some(id) = crate::domain::changes::threads::take_jump(&self.id) {
            self.set_pane(Pane::Changes);
            self.panes.changes.show_comment(&id);
        }

        // members set elsewhere (a reload, tests): the box order follows
        if self.agents.order.len() != self.agents.members.len()
            || !self
                .agents
                .members
                .iter()
                .all(|message| self.agents.order.contains(message))
        {
            self.sync_order();
        }
    }
    pub(super) fn draw_bounds(&mut self, frame: &mut Frame) -> (Rect, Rect, Option<Rect>) {
        let full = frame.area();

        frame.render_widget(Paragraph::new("").style(theme::text()), full);

        // row 0: the view bar (views of the thread, the title)
        let top = Rect {
            height: full.height.min(1),
            ..full
        };
        let below = Rect {
            y: full.y + top.height,
            height: full.height - top.height,
            ..full
        };

        // a dotted line below the view bar (as the fold line above the prompt)
        let sep = u16::from(below.height > 8);

        if sep > 0 {
            draw_bounds_buf_state(frame, below);
        }

        let area = Rect {
            y: below.y + sep,
            height: below.height - sep,
            ..below
        };

        self.follow_width(area);

        // wide changes view: the changes left at full height; the chat column right of it (the
        // header, the stream, the prompt) is laid out below as the whole area
        let wide = self.panes.pane == Pane::Changes && self.layout.split && area.width >= SPLIT_MIN;

        self.layout.wide_split = wide;

        let (area, changes_left) = if wide {
            draw_bounds_buf(frame, area)
        } else {
            (area, None)
        };

        (top, area, changes_left)
    }
}

fn draw_bounds_buf_state(frame: &mut Frame<'_>, below: Rect) {
    let buf = frame.buffer_mut();

    for x in below.x..below.right() {
        buf[(x, below.y)].set_symbol("┄").set_style(theme::line());
    }
}

fn draw_bounds_buf(frame: &mut Frame<'_>, area: Rect) -> (Rect, Option<Rect>) {
    let lw = area.width * 3 / 5;
    let buf = frame.buffer_mut();

    for y in area.y..area.bottom() {
        buf[(area.x + lw, y)].set_symbol("│").set_style(theme::line());
    }

    (
        Rect {
            x: area.x + lw + 1,
            width: area.width - lw - 1,
            ..area
        },
        Some(Rect { width: lw, ..area }),
    )
}
