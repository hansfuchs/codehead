use super::{MouseEvent, MouseEventKind, Row, View};

impl View {
    pub(crate) fn mouse(&mut self, event: MouseEvent) {
        mouse_view(self, event);
    }

    fn focus_filter(&mut self, event: MouseEvent) {
        self.filter_input.filter.copied = None;

        if matches!(event.kind, MouseEventKind::Down(_)) {
            self.filter_input.filtering = true;
        }
    }
}

fn click_file_row(event: MouseEvent, view: &mut View) {
    if let Some((_, Row::File(position))) = view.drawn.iter().find(|(y, _)| *y == event.row) {
        view.at = *position;
        view.text_preview.scroll = 0;
        view.filter_input.filtering = false;
    }
}

fn mouse_view(view: &mut View, event: MouseEvent) {
    let at = ratatui::layout::Position {
        x: event.column,
        y: event.row,
    };

    // the filter input: its own mouse (cursor, selection + copy)
    if !view.files.is_empty() && view.filter_input.filter.mouse(event) {
        view.focus_filter(event);
        return;
    }

    match event.kind {
        MouseEventKind::ScrollDown if view.preview.contains(at) => view.text_preview.scroll += 3,
        MouseEventKind::ScrollUp if view.preview.contains(at) => {
            view.text_preview.scroll = view.text_preview.scroll.saturating_sub(3);
        }
        MouseEventKind::ScrollDown if view.list.contains(at) => view.step(1),
        MouseEventKind::ScrollUp if view.list.contains(at) => view.step(-1),
        MouseEventKind::Down(_) if view.list.contains(at) => {
            click_file_row(event, view);
        }
        MouseEventKind::Down(_) if at.y + 2 == view.list.y && at.x < view.list.right() => {
            view.filter_input.filtering = true;
        }
        _ => {}
    }
}
