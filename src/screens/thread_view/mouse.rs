use super::{MouseButton, MouseEvent, MouseEventKind, Pane, Position, View};
impl View {
    pub(super) fn mouse(&mut self, event: MouseEvent) {
        let at = Position::new(event.column, event.row);
        let click = matches!(event.kind, MouseEventKind::Down(MouseButton::Left));

        if self.mouse_overlays(event) || self.mouse_cost_box(event, at, click) || self.mouse_activity(event) {
            return;
        }

        self.selection.hover = Some(at);

        if self.mouse_agents(event, at, click) || self.mouse_navigation(at, click) {
            return;
        }

        if self.panes.pane != Pane::Chat && self.mouse_act(event, at, click).is_break() {
            return;
        }

        self.mouse_stream(event, at);
    }
}

impl View {
    /// The tokens box: a click toggles compact ↔ expanded; the wheel scrolls the expanded box.
    fn mouse_cost_box(&mut self, event: MouseEvent, at: Position, click: bool) -> bool {
        if !self.cost_box.rect.contains(at) {
            return false;
        }

        match event.kind {
            _ if click => self.cost_box.toggle(),
            MouseEventKind::ScrollDown if self.cost_box.expanded => {
                self.cost_box.scroll = self.cost_box.scroll.saturating_add(1);
            }
            MouseEventKind::ScrollUp if self.cost_box.expanded => {
                self.cost_box.scroll = self.cost_box.scroll.saturating_sub(1);
            }
            _ => return self.cost_box.expanded,
        }

        true
    }
}
