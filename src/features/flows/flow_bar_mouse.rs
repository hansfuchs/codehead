use super::{FlowBar, MouseButton, MouseEvent, MouseEventKind, Position};

impl FlowBar {
    pub(crate) fn mouse(&mut self, event: MouseEvent) -> bool {
        mouse_flow_bar(self, event)
    }
}

fn mouse_mouseeventkind_down_mousebutton_left(at: Position, view_state: &mut FlowBar) {
    if let Some(&(_, i)) = view_state
        .popup_state
        .rows
        .iter()
        .find(|(result, _)| result.contains(at))
    {
        view_state.popup_state.cur = i;
    }
}

fn mouse_body(event: MouseEvent, at: Position, view_state: &mut FlowBar) -> bool {
    match event.kind {
        MouseEventKind::ScrollDown => {
            view_state.popup_state.cur = (view_state.popup_state.cur + 1).min(view_state.len().saturating_sub(1));
        }
        MouseEventKind::ScrollUp => view_state.popup_state.cur = view_state.popup_state.cur.saturating_sub(1),
        MouseEventKind::Down(MouseButton::Left) => {
            mouse_mouseeventkind_down_mousebutton_left(at, view_state);
        }
        _ => {}
    }

    true
}

fn mouse_branch(view_state: &mut FlowBar) -> bool {
    if view_state.popup_state.open {
        view_state.popup_state.open = false;
    } else {
        view_state.open();
    }

    true
}

fn mouse_flow_bar(state: &mut FlowBar, event: MouseEvent) -> bool {
    let at = Position {
        x: event.column,
        y: event.row,
    };

    state.popup_state.hover = state.popup_state.bar.contains(at);

    if matches!(event.kind, MouseEventKind::Down(MouseButton::Left)) && state.popup_state.bar.contains(at) {
        return mouse_branch(state);
    }

    if state.popup_state.open && state.popup_state.popup.contains(at) {
        return mouse_body(event, at, state);
    }

    false
}
