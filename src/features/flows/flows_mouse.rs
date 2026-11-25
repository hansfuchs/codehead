use super::{FlowsTab, Focus, MouseButton, MouseEvent, MouseEventKind, Out, Position};

impl FlowsTab {
    pub(crate) fn mouse(&mut self, event: MouseEvent) -> Out {
        mouse_flows_tab(self, event)
    }

    fn switch_sub(&mut self, sub: super::Sub) {
        self.editor = None;
        self.sub = sub;
        self.at = 0;
        self.focus = Focus::List;
        self.reload();
    }
}

fn select_list_row(i: usize, tab: &mut FlowsTab) {
    tab.editor = None;
    tab.at = i;
    tab.focus = Focus::List;
    tab.load_flow();
}

fn mouse_flows_tab(state: &mut FlowsTab, event: MouseEvent) -> Out {
    let at = Position {
        x: event.column,
        y: event.row,
    };
    let click = matches!(event.kind, MouseEventKind::Down(MouseButton::Left));

    if let Some(editor) = state.editor.as_mut()
        && let Some(&(field_rect, i)) = state.hit_regions.field_rects.iter().find(|(area, _)| area.contains(at))
    {
        let _ = field_rect;

        if click {
            editor.field = i;
        }

        editor.fields[i].1.mouse(event);
        return Out::None;
    }

    if !click {
        return Out::None;
    }

    if let Some(&(_, sub)) = state.hit_regions.sub_rects.iter().find(|(area, _)| area.contains(at)) {
        if sub != state.sub && state.store_editor() {
            state.switch_sub(sub);
        }

        return Out::None;
    }

    if let Some(&(_, i)) = state.hit_regions.rows.iter().find(|(area, _)| area.contains(at)) {
        if state.store_editor() {
            select_list_row(i, state);
        }

        return Out::None;
    }

    if let Some(&(_, i)) = state.hit_regions.rail_rows.iter().find(|(area, _)| area.contains(at))
        && state.store_editor()
    {
        state.editor = None;
        state.node = i;
        state.focus = Focus::Rail;
    }

    Out::None
}
