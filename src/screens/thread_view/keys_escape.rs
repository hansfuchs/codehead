use super::{DOUBLE_ESC, Exit, Instant, KeyCode, KeyEvent, Pane, View};

impl View {
    pub(super) fn key_escape(&mut self, key: KeyEvent) -> Option<bool> {
        match key.code {
            KeyCode::Esc
                if self.panes.pane != Pane::Chat
                    && self.panes.prompt_focus
                    && !(self.layout.wide_split
                        && (!self.thread.busy.is_empty() || self.prompt.queued.is_some() || self.stream.back > 0)) =>
            {
                self.panes.prompt_focus = false;
                return Some(false);
            }
            KeyCode::Esc => {
                return Some(press_escape(self));
            }

            _ => {}
        }

        None
    }
}

fn press_escape(view: &mut View) -> bool {
    if view.last_esc.is_some_and(|at| at.elapsed() < DOUBLE_ESC) {
        view.esc_interrupt = None;
        view.exit = Exit::Back;
        return true;
    }

    view.last_esc = Some(Instant::now());

    if view.selection.sel.take().is_some() {
    } else if view.stream.back > 0 {
        view.stream.back = 0;
    } else if !view.thread.busy.is_empty() || view.prompt.queued.is_some() {
        // a 2nd Esc within DOUBLE_ESC means back: the interrupt waits for it
        view.esc_interrupt = Some(Instant::now());
    }

    false
}
