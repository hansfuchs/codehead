use super::{
    Act, Instant, KeyCode, KeyEvent, KeyModifiers, Menu, Merge, MouseButton, MouseEvent, MouseEventKind, Position,
    Stop, Sub,
};

impl Merge {
    pub(crate) fn mouse(&mut self, event: MouseEvent) -> Act {
        dispatch_mouse(self, event)
    }
}

fn dispatch_mouse(widget_state: &mut Merge, event: MouseEvent) -> Act {
    widget_state.active = Instant::now();
    let at = Position {
        x: event.column,
        y: event.row,
    };
    let click = matches!(event.kind, MouseEventKind::Down(MouseButton::Left));

    if click && let Some(action) = click_tabs(widget_state, at) {
        return action;
    }

    if widget_state
        .tab_ref()
        .is_some_and(|changes_value| changes_value.sub == Sub::Changes)
        && widget_state.layout.body_area.contains(at)
    {
        let Some(character) = widget_state.changes_view() else {
            return Act::None;
        };

        return match character.mouse(event) {
            crate::features::changes::Act::Toast(thread) => Act::Toast(thread),
            crate::features::changes::Act::Open(file, line) => Act::Open(file, line),
            _ => Act::None,
        };
    }

    match event.kind {
        MouseEventKind::ScrollDown => {
            if let Some(thread_value) = widget_state.tab() {
                thread_value.free = true;
                thread_value.scroll += 3;
            }

            return Act::None;
        }
        MouseEventKind::ScrollUp => {
            if let Some(thread_2) = widget_state.tab() {
                thread_2.free = true;
                thread_2.scroll = thread_2.scroll.saturating_sub(3);
            }

            return Act::None;
        }
        _ => {}
    }

    if !click {
        return Act::None;
    }

    // an open dropdown: a click on a row picks it, elsewhere closes it
    if widget_state
        .tab_ref()
        .is_some_and(|thread_3| thread_3.input.menu.is_some())
    {
        return click_dropdown(widget_state, at);
    }

    if widget_state.layout.edit_rect.contains(at) {
        if let Some((_, ta)) = widget_state.tab().and_then(|thread_6| thread_6.input.edit.as_mut()) {
            ta.mouse(event);
        }

        return Act::None;
    }

    click_overview(widget_state, event, at)
}

fn click_overview(widget_state: &mut Merge, event: MouseEvent, at: Position) -> Act {
    let Some(&(_, text_value)) = widget_state
        .layout
        .hits
        .iter()
        .find(|(result_3, _)| result_3.contains(at))
    else {
        return Act::None;
    };

    if let Some(thread_7) = widget_state.tab() {
        // a click: the view stays put
        thread_7.free = true;
        thread_7.focus = Some(text_value);
        thread_7.input.edit = None;
    }

    if event.modifiers.contains(KeyModifiers::CONTROL) || event.modifiers.contains(KeyModifiers::SUPER) {
        return widget_state.open_on_github(text_value);
    }

    match text_value {
        // a click on a label removes it, on the checks banner toggles them
        Stop::Label(_)
        | Stop::AddLabel
        | Stop::Status
        | Stop::Checks
        | Stop::Update
        | Stop::Merge
        | Stop::NewComment
        | Stop::Tabs
        | Stop::Subs => widget_state.activate(text_value),
        _ => Act::None,
    }
}

fn click_dropdown(widget_state: &mut Merge, at: Position) -> Act {
    if let Some(&(_, i)) = widget_state
        .layout
        .menu_hits
        .iter()
        .find(|(result_2, _)| result_2.contains(at))
    {
        if let Some(thread_4) = widget_state.tab()
            && let Some(
                Menu::Status(character_value, _)
                | Menu::Mine {
                    cur: character_value, ..
                }
                | Menu::Labels {
                    cur: character_value, ..
                },
            ) = thread_4.input.menu.as_mut()
        {
            *character_value = i;
        }

        return widget_state.menu_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    }

    if let Some(thread_5) = widget_state.tab() {
        thread_5.input.menu = None;
    }

    Act::None
}

fn click_tabs(widget_state: &mut Merge, at: Position) -> Option<Act> {
    if let Some(&(_, i)) = widget_state
        .layout
        .tab_hits
        .iter()
        .find(|(result, _)| result.contains(at))
    {
        widget_state.cur = i;
        return Some(Act::None);
    }

    if let Some(&(_, text)) = widget_state
        .layout
        .sub_hits
        .iter()
        .find(|(result_value, _)| result_value.contains(at))
    {
        return Some(widget_state.set_sub(text));
    }

    None
}
