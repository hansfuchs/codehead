use super::{Act, DOUBLE_CLICK, Instant, KeyModifiers, MouseButton, MouseEvent, MouseEventKind, View};

impl View {
    /// A click on the filter row: the filter takes the keys; on a row: the cursor there, its
    /// lines into the selection (a drag: the rows it passes; shift+click: the rows from the last
    /// click); the right button takes lines out (2 right clicks: the selection clears); a click
    /// on a comment opens it.
    pub(crate) fn mouse(&mut self, event: MouseEvent) -> Act {
        mouse_view(self, event)
    }
}

fn select_mouse_row(event: MouseEvent, last: usize, event_value: MouseButton, view: &mut View) -> Option<Act> {
    view.filter_state.filtering = false;
    view.scm.focus = false;
    let row = view
        .row_at_y(event.row)
        .unwrap_or(view.top + usize::from(event.row - view.layout.area.y))
        .min(last);

    view.at = row;

    if event_value == MouseButton::Right {
        // 2 right clicks in a row: the whole selection clears
        if view
            .selection_state
            .right_at
            .is_some_and(|thread| thread.elapsed() < DOUBLE_CLICK)
        {
            view.selection_state.right_at = None;
            view.selection_state.sel.clear();
            return Some(Act::None);
        }

        view.selection_state.right_at = Some(Instant::now());
        view.selection_state.drag = Some((row, view.selection_state.sel.clone(), false));
        view.select_row(row, false);
        return Some(Act::None);
    }

    if view.layout.rows.get(row).is_some_and(|result| result.comment.is_some()) {
        view.at = row;
        return Some(view.open_comment_row(row, true));
    }

    if event.modifiers.contains(KeyModifiers::SHIFT)
        && let Some(agent) = view.selection_state.anchor
    {
        for i in agent.min(row)..=agent.max(row) {
            view.select_row(i, true);
        }

        return Some(Act::None);
    }

    view.selection_state.anchor = Some(row);
    view.selection_state.drag = Some((row, view.selection_state.sel.clone(), true));
    view.select_row(row, true);

    None
}

fn scroll_mouse_down(last: usize, view: &mut View) {
    view.top = (view.top + 3).min(
        view.layout
            .rows
            .len()
            .saturating_sub(usize::from(view.layout.area.height)),
    );
    view.at = view.at.max(view.top + 1).min(last);
}

fn drag_mouse(event: MouseEvent, last: usize, view: &mut View) -> Option<Act> {
    if let (MouseEventKind::Drag(MouseButton::Left | MouseButton::Right), Some((anchor, base, add))) =
        (event.kind, view.selection_state.drag.clone())
    {
        let height = usize::from(view.layout.area.height);
        let row = if event.row < view.layout.area.y {
            view.top = view.top.saturating_sub(1);
            view.top
        } else if event.row >= view.layout.area.bottom() {
            view.top = (view.top + 1).min(view.layout.rows.len().saturating_sub(height));
            view.top + height.saturating_sub(1)
        } else {
            view.row_at_y(event.row)
                .unwrap_or(view.top + usize::from(event.row - view.layout.area.y))
        }
        .min(last);

        view.selection_state.sel = base;

        for i in anchor.min(row)..=anchor.max(row) {
            view.select_row(i, add);
        }

        view.at = row;
        return Some(Act::None);
    }

    None
}

fn mouse_view(view: &mut View, event: MouseEvent) -> Act {
    if view.modal_state.modal.is_some() {
        return view.modal_mouse(event);
    }

    let at = ratatui::layout::Position {
        x: event.column,
        y: event.row,
    };

    if view.selection_state.drag.is_none() && (view.scm.rect.contains(at) || view.scm.picking()) {
        return view.panel_mouse(event);
    }

    // the panel's commit in the review's place: the review's mouse is its
    if let Some(path) = view.peek.as_mut() {
        if matches!(event.kind, MouseEventKind::Down(_)) {
            view.scm.focus = false;
        }

        return path.mouse(event);
    }

    let click = matches!(event.kind, MouseEventKind::Down(MouseButton::Left));

    // the filter input: its own mouse (cursor, selection + copy); a click on the row opens it
    if view.filter_state.filtering && view.filter_state.filter_ta.mouse(event) {
        view.filter_state.filter_ta.copied = None;
        return Act::None;
    }

    if click && view.layout.filter_rect.contains(at) {
        view.filter_state.filtering = true;
        view.scm.focus = false;
        view.filter_state.filter_ta.move_end();
        return Act::None;
    }

    let last = view.layout.rows.len().saturating_sub(1);

    // a drag past the rows' top / bottom: the row there, the view scrolls 1
    if let Some(value) = drag_mouse(event, last, view) {
        return value;
    }

    if matches!(event.kind, MouseEventKind::Up(_)) {
        view.selection_state.drag = None;
        return Act::None;
    }

    if !view.layout.area.contains(at) {
        return Act::None;
    }

    match event.kind {
        MouseEventKind::ScrollDown => scroll_mouse_down(last, view),
        MouseEventKind::ScrollUp => {
            view.top = view.top.saturating_sub(3);
            view.at = view
                .at
                .min(view.top + usize::from(view.layout.area.height).saturating_sub(2));
        }
        MouseEventKind::Down(event_value @ (MouseButton::Left | MouseButton::Right)) => {
            if let Some(value) = select_mouse_row(event, last, event_value, view) {
                return value;
            }
        }
        _ => {}
    }

    Act::None
}
