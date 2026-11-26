use super::{ActivityLayout, Frame, Rect, View};

impl View {
    /// Every row renders, also at 0 height: a render resets the row's click rects, so a row
    /// that just closed never keeps a stale rect over the rows that move into its place.
    pub(super) fn draw_activity(&mut self, frame: &mut Frame, layout: &ActivityLayout) {
        let tick = self.tick();

        self.monitors.render(frame, layout.monitors, tick);
        draw_flowbars(self, frame, layout);
        self.subagents.render(frame, layout.subagents, tick, &self.thread.subs);
        self.subthreads.render(frame, layout.subthreads, tick);
    }
}

fn draw_flowbars(view: &mut View, frame: &mut Frame<'_>, layout: &ActivityLayout) {
    let mut y = layout.flow.y;

    for (bar, height) in view.flowbars.iter_mut().zip(&layout.flow_rows) {
        let height = (*height).min(layout.flow.bottom().saturating_sub(y));

        bar.render(
            frame,
            Rect {
                y,
                height,
                ..layout.flow
            },
            true,
        );
        y += height;
    }
}
