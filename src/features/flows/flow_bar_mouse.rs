use super::{FlowBar, MouseButton, MouseEvent, MouseEventKind, Position};

impl FlowBar {
    pub(crate) fn mouse(&mut self, event: MouseEvent) -> bool {
        mouse_flow_bar(self, event)
    }
}

fn select_clicked_row(at: Position, bar: &mut FlowBar) {
    if let Some(&(_, i)) = bar.popup_state.rows.iter().find(|(area, _)| area.contains(at)) {
        bar.popup_state.cur = i;
    }
}

fn popup_mouse(event: MouseEvent, at: Position, bar: &mut FlowBar) -> bool {
    match event.kind {
        MouseEventKind::ScrollDown => {
            bar.popup_state.cur = (bar.popup_state.cur + 1).min(bar.len().saturating_sub(1));
        }
        MouseEventKind::ScrollUp => bar.popup_state.cur = bar.popup_state.cur.saturating_sub(1),
        MouseEventKind::Down(MouseButton::Left) => {
            select_clicked_row(at, bar);
        }
        _ => {}
    }

    true
}

fn toggle_popup(bar: &mut FlowBar) -> bool {
    if bar.popup_state.open {
        bar.popup_state.open = false;
    } else {
        bar.open();
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
        return toggle_popup(state);
    }

    if state.popup_state.open && state.popup_state.popup.contains(at) {
        return popup_mouse(event, at, state);
    }

    false
}
