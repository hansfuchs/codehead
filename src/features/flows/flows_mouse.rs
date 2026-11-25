use super::{FlowsTab, Focus, MouseButton, MouseEvent, MouseEventKind, Out, Position};

impl FlowsTab {
    pub(crate) fn mouse(&mut self, event: MouseEvent) -> Out {
        mouse_flows_tab(self, event)
    }

    fn mouse_body(&mut self, text: super::Sub) {
        self.editor = None;
        self.sub = text;
        self.at = 0;
        self.focus = Focus::List;
        self.reload();
    }
}

fn mouse_body_state(i: usize, view_state: &mut FlowsTab) {
    view_state.editor = None;
    view_state.at = i;
    view_state.focus = Focus::List;
    view_state.load_flow();
}

fn mouse_flows_tab(state: &mut FlowsTab, event: MouseEvent) -> Out {
    let path = Position {
        x: event.column,
        y: event.row,
    };
    let click = matches!(event.kind, MouseEventKind::Down(MouseButton::Left));

    if let Some(ed) = state.editor.as_mut()
        && let Some(&(result_value, i)) = state
            .hit_regions
            .field_rects
            .iter()
            .find(|(result, _)| result.contains(path))
    {
        let _ = result_value;

        if click {
            ed.field = i;
        }

        ed.fields[i].1.mouse(event);
        return Out::None;
    }

    if !click {
        return Out::None;
    }

    if let Some(&(_, text)) = state
        .hit_regions
        .sub_rects
        .iter()
        .find(|(result_2, _)| result_2.contains(path))
    {
        if text != state.sub && state.store_editor() {
            state.mouse_body(text);
        }

        return Out::None;
    }

    if let Some(&(_, i)) = state
        .hit_regions
        .rows
        .iter()
        .find(|(result_3, _)| result_3.contains(path))
    {
        if state.store_editor() {
            mouse_body_state(i, state);
        }

        return Out::None;
    }

    if let Some(&(_, i)) = state
        .hit_regions
        .rail_rows
        .iter()
        .find(|(result_4, _)| result_4.contains(path))
        && state.store_editor()
    {
        state.editor = None;
        state.node = i;
        state.focus = Focus::Rail;
    }

    Out::None
}
