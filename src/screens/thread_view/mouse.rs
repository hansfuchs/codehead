use super::{MouseButton, MouseEvent, MouseEventKind, Pane, Position, View};
impl View {
    pub(super) fn mouse(&mut self, event: MouseEvent) {
        let at = Position::new(event.column, event.row);
        let click = matches!(event.kind, MouseEventKind::Down(MouseButton::Left));

        if self.mouse_overlays(event) || self.mouse_activity(event) {
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
