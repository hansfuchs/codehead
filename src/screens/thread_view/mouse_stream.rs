use super::{MouseButton, MouseEvent, MouseEventKind, Position, View};
impl View {
    pub(super) fn mouse_stream(&mut self, event: MouseEvent, at: Position) {
        let in_stream = self.layout.stream_rect.contains(at);

        match event.kind {
            MouseEventKind::ScrollUp if in_stream => self.scroll(3),
            MouseEventKind::ScrollDown if in_stream => self.scroll(-3),
            MouseEventKind::Down(MouseButton::Left) if in_stream => {
                self.mouse_row(at);
            }
            MouseEventKind::Drag(MouseButton::Left) if self.selection.dragging => mouse_drag(self, at),
            MouseEventKind::Up(MouseButton::Left) if self.selection.dragging => mouse_release(self),
            _ => mouse_prompt(self, event, at),
        }
    }
}

fn mouse_drag(view: &mut View, at: Position) {
    if at.y < view.layout.stream_rect.y {
        view.scroll(1);
    } else if at.y >= view.layout.stream_rect.bottom() {
        view.scroll(-1);
    }

    let y = at.y.clamp(
        view.layout.stream_rect.y,
        view.layout.stream_rect.bottom().saturating_sub(1),
    );

    let (Some(from), Some(row)) = (view.selection.drag_from, view.row_at(y)) else {
        return;
    };

    let (first, last) = view.item_rows(from.0);

    let to = if row < first {
        (first, 0)
    } else if row > last {
        (last, u16::MAX)
    } else {
        (row, at.x.saturating_sub(view.layout.stream_rect.x))
    };

    view.selection.sel = Some((from, to));
}

fn mouse_release(view: &mut View) {
    view.selection.dragging = false;

    if view.selection.sel.is_some_and(|(start, end)| start != end)
        && let Some(text) = view.selected()
    {
        view.copy(&text);
    }
}

fn mouse_prompt(view: &mut View, event: MouseEvent, at: Position) {
    if !view.layout.input_rect.contains(at) && !view.prompt.input.area().contains(at) {
        return;
    }

    if matches!(event.kind, MouseEventKind::Down(_)) && view.ask.focus {
        view.ask.focus = false;
        view.stream.dirty = true;
    }

    view.prompt.input.mouse(event);

    if let Some(count) = view.prompt.input.copied.take() {
        view.toast(format!("copied {count} chars"));
    }
}
