use super::{ActivityLayout, Frame, Rect, View};

impl View {
    pub(super) fn draw_activity(&mut self, frame: &mut Frame, layout: &ActivityLayout) {
        if layout.monitors.height > 0 {
            self.monitors.render(frame, layout.monitors, self.tick());
        }

        if layout.flow.height > 0 {
            draw_flowbars(self, frame, layout);
        }

        if layout.subagents.height > 0 {
            let tick = self.tick();

            self.subagents.render(frame, layout.subagents, tick, &self.thread.subs);
        }

        if layout.subthreads.height > 0 {
            let tick = self.tick();

            self.subthreads.render(frame, layout.subthreads, tick);
        }
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
