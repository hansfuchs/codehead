use super::{MouseEvent, MouseEventKind, Row, View};

impl View {
    pub(crate) fn mouse(&mut self, event: MouseEvent) {
        mouse_view(self, event);
    }

    fn mouse_body(&mut self, event: MouseEvent) {
        self.filter_input.filter.copied = None;

        if matches!(event.kind, MouseEventKind::Down(_)) {
            self.filter_input.filtering = true;
        }
    }
}

fn mouse_mouseeventkind_down__(event: MouseEvent, view_state: &mut View) {
    if let Some((_, Row::File(key))) = view_state.drawn.iter().find(|(y, _)| *y == event.row) {
        view_state.at = *key;
        view_state.text_preview.scroll = 0;
        view_state.filter_input.filtering = false;
    }
}

fn mouse_view(state: &mut View, event: MouseEvent) {
    let at = ratatui::layout::Position {
        x: event.column,
        y: event.row,
    };

    // the filter input: its own mouse (cursor, selection + copy)
    if !state.files.is_empty() && state.filter_input.filter.mouse(event) {
        state.mouse_body(event);
        return;
    }

    match event.kind {
        MouseEventKind::ScrollDown if state.preview.contains(at) => state.text_preview.scroll += 3,
        MouseEventKind::ScrollUp if state.preview.contains(at) => {
            state.text_preview.scroll = state.text_preview.scroll.saturating_sub(3);
        }
        MouseEventKind::ScrollDown if state.list.contains(at) => state.step(1),
        MouseEventKind::ScrollUp if state.list.contains(at) => state.step(-1),
        MouseEventKind::Down(_) if state.list.contains(at) => {
            mouse_mouseeventkind_down__(event, state);
        }
        MouseEventKind::Down(_) if at.y + 2 == state.list.y && at.x < state.list.right() => {
            state.filter_input.filtering = true;
        }
        _ => {}
    }
}
