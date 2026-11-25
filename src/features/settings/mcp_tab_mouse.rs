use super::{McpTab, Modal, MouseButton, MouseEvent, MouseEventKind, Position};

impl McpTab {
    pub(crate) fn mouse(&mut self, event: MouseEvent) {
        mouse_mcp_tab(self, event);
    }
}

fn left_click(event: MouseEvent, at: Position, mcp_tab: &mut McpTab) {
    if let Some(Modal::Agents(pick)) = &mut mcp_tab.modal {
        if let Some(&(_, i)) = mcp_tab.modal_hits.iter().find(|(rect, _)| rect.contains(at)) {
            pick.cursor = i;

            toggle_selected_agent(i, pick);
        }

        return;
    }

    if mcp_tab.modal.is_some() {
        return;
    }

    if mcp_tab.filter.area().contains(at) {
        mcp_tab.filtering = true;
        mcp_tab.filter.mouse(event);
        return;
    }

    if let Some(&(_, i)) = mcp_tab.hits.iter().find(|(rect, _)| rect.contains(at)) {
        mcp_tab.filtering = false;

        if mcp_tab.cursor == i {
            super::open_current(mcp_tab);
        }

        mcp_tab.cursor = i;
    }
}

fn toggle_selected_agent(i: usize, selection: &mut super::Pick) {
    if let Some(row) = selection.rows.get_mut(i) {
        row.1 = !row.1;
    }
}

fn mouse_mcp_tab(mcp_tab: &mut McpTab, event: MouseEvent) {
    let at = Position {
        x: event.column,
        y: event.row,
    };

    // the edit form: a click on a text field focuses it; the field takes its mouse
    // (cursor, selection + copy)
    if let Some(Modal::Edit(form)) = &mut mcp_tab.modal {
        edit_form_mouse(form, event, at);

        return;
    }

    // the filter: its mouse while it has the keys or a drag runs in it
    if mcp_tab.modal.is_none()
        && mcp_tab.filtering
        && !matches!(event.kind, MouseEventKind::Down(_))
        && mcp_tab.filter.mouse(event)
    {
        mcp_tab.filter.copied = None;
        return;
    }

    match event.kind {
        MouseEventKind::ScrollUp => mcp_tab.cursor = mcp_tab.cursor.saturating_sub(3),
        MouseEventKind::ScrollDown => {
            mcp_tab.cursor = (mcp_tab.cursor + 3).min(mcp_tab.items().0.len().saturating_sub(1));
        }
        MouseEventKind::Down(MouseButton::Left) => {
            self::left_click(event, at, mcp_tab);
        }
        _ => {}
    }
}

fn edit_form_mouse(edit_form: &mut super::EditForm, event: MouseEvent, at: Position) {
    let down = matches!(event.kind, MouseEventKind::Down(_));

    for (index, field) in [
        (0, &mut edit_form.name),
        (2, &mut edit_form.target),
        (3, &mut edit_form.secrets),
    ] {
        if text_field_mouse(event, at, down, index, field, &mut edit_form.field) {
            return;
        }
    }
}

fn text_field_mouse(
    event: MouseEvent,
    at: Position,
    down: bool,
    index: usize,
    field: &mut crate::ui::textarea::TextArea,
    selected: &mut usize,
) -> bool {
    if (*selected == index || (down && field.area().contains(at))) && field.mouse(event) {
        field.copied = None;
        *selected = index;
        return true;
    }

    false
}
