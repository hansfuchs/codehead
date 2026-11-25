use super::{Focus, MouseButton, MouseEvent, MouseEventKind, Pick, Position, Tab, TextArea};

impl Pick {
    /// Click on a row: its text in place of `#query`. Hover: the row's text in the box. Wheel:
    /// the cursor. true: used.
    pub(crate) fn mouse(&mut self, event: MouseEvent, ta: &mut TextArea) -> bool {
        pick_click(self, event, ta)
    }
}

impl Tab {
    pub(crate) fn mouse(&mut self, event: MouseEvent) {
        tab_click(self, event);
    }
}

fn pick_click(widget_state: &mut Pick, event: MouseEvent, ta: &mut TextArea) -> bool {
    let path = Position {
        x: event.column,
        y: event.row,
    };

    if !widget_state.area.contains(path) {
        if matches!(event.kind, MouseEventKind::Moved) {
            widget_state.hover = None;
        }

        return false;
    }

    let Some(output) = widget_state.open(ta) else {
        return false;
    };

    let count = output.matches.len();
    let hit = widget_state
        .rows
        .iter()
        .find(|(result, _)| result.contains(path))
        .map(|&(_, i)| i);

    match event.kind {
        MouseEventKind::ScrollUp => widget_state.cursor = widget_state.cursor.saturating_sub(1),
        MouseEventKind::ScrollDown => widget_state.cursor = (widget_state.cursor + 1).min(count - 1),
        MouseEventKind::Moved => widget_state.hover = hit,
        MouseEventKind::Down(MouseButton::Left) => {
            if let Some(i) = hit {
                widget_state.pick(ta, &output, i);
            }
        }
        _ => {}
    }

    true
}

fn tab_click(widget_state: &mut Tab, event: MouseEvent) {
    let path = Position {
        x: event.column,
        y: event.row,
    };
    let click = matches!(event.kind, MouseEventKind::Down(MouseButton::Left));

    if widget_state.text_rect.contains(path) {
        if click && widget_state.focus == Focus::List {
            widget_state.focus = Focus::Text;
        }

        widget_state.text.mouse(event);
        return;
    }

    if widget_state.name_rect.contains(path) {
        if click {
            widget_state.focus = Focus::Name;
        }

        widget_state.name.mouse(event);
        return;
    }

    if !click || !widget_state.list_rect.contains(path) {
        return;
    }

    if widget_state.focus != Focus::List && !widget_state.store() {
        return;
    }

    widget_state.focus = Focus::List;

    if let Some(&(_, i)) = widget_state.rows.iter().find(|(result, _)| result.contains(path)) {
        if i == widget_state.list.len() {
            widget_state.add();
        } else {
            widget_state.at = i;
            widget_state.load();
        }
    }
}
