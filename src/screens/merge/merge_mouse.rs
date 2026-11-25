use super::{
    Act, Instant, KeyCode, KeyEvent, KeyModifiers, Menu, Merge, MouseButton, MouseEvent, MouseEventKind, Position,
    Stop, Sub,
};

impl Merge {
    pub(crate) fn mouse(&mut self, event: MouseEvent) -> Act {
        dispatch_mouse(self, event)
    }
}

fn dispatch_mouse(merge: &mut Merge, event: MouseEvent) -> Act {
    merge.active = Instant::now();
    let at = Position {
        x: event.column,
        y: event.row,
    };
    let click = matches!(event.kind, MouseEventKind::Down(MouseButton::Left));

    if click && let Some(action) = click_tabs(merge, at) {
        return action;
    }

    if merge.tab_ref().is_some_and(|tab| tab.sub == Sub::Changes) && merge.layout.body_area.contains(at) {
        let Some(view) = merge.changes_view() else {
            return Act::None;
        };

        return match view.mouse(event) {
            crate::features::changes::Act::Toast(toast) => Act::Toast(toast),
            crate::features::changes::Act::Open(file, line) => Act::Open(file, line),
            _ => Act::None,
        };
    }

    match event.kind {
        MouseEventKind::ScrollDown => {
            if let Some(tab) = merge.tab() {
                tab.free = true;
                tab.scroll += 3;
            }

            return Act::None;
        }
        MouseEventKind::ScrollUp => {
            if let Some(tab) = merge.tab() {
                tab.free = true;
                tab.scroll = tab.scroll.saturating_sub(3);
            }

            return Act::None;
        }
        _ => {}
    }

    if !click {
        return Act::None;
    }

    // an open dropdown: a click on a row picks it, elsewhere closes it
    if merge.tab_ref().is_some_and(|tab| tab.input.menu.is_some()) {
        return click_dropdown(merge, at);
    }

    if merge.layout.edit_rect.contains(at) {
        if let Some((_, textarea)) = merge.tab().and_then(|tab| tab.input.edit.as_mut()) {
            textarea.mouse(event);
        }

        return Act::None;
    }

    click_overview(merge, event, at)
}

fn click_overview(merge: &mut Merge, event: MouseEvent, at: Position) -> Act {
    let Some(&(_, stop)) = merge.layout.hits.iter().find(|(rect, _)| rect.contains(at)) else {
        return Act::None;
    };

    if let Some(tab) = merge.tab() {
        // a click: the view stays put
        tab.free = true;
        tab.focus = Some(stop);
        tab.input.edit = None;
    }

    if event.modifiers.contains(KeyModifiers::CONTROL) || event.modifiers.contains(KeyModifiers::SUPER) {
        return merge.open_on_github(stop);
    }

    match stop {
        // a click on a label removes it, on the checks banner toggles them
        Stop::Label(_)
        | Stop::AddLabel
        | Stop::Status
        | Stop::Checks
        | Stop::Update
        | Stop::Merge
        | Stop::NewComment
        | Stop::Tabs
        | Stop::Subs => merge.activate(stop),
        _ => Act::None,
    }
}

fn click_dropdown(merge: &mut Merge, at: Position) -> Act {
    if let Some(&(_, i)) = merge.layout.menu_hits.iter().find(|(rect, _)| rect.contains(at)) {
        if let Some(tab) = merge.tab()
            && let Some(Menu::Status(cur, _) | Menu::Mine { cur, .. } | Menu::Labels { cur, .. }) =
                tab.input.menu.as_mut()
        {
            *cur = i;
        }

        return merge.menu_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    }

    if let Some(tab) = merge.tab() {
        tab.input.menu = None;
    }

    Act::None
}

fn click_tabs(merge: &mut Merge, at: Position) -> Option<Act> {
    if let Some(&(_, i)) = merge.layout.tab_hits.iter().find(|(rect, _)| rect.contains(at)) {
        merge.cur = i;
        return Some(Act::None);
    }

    if let Some(&(_, sub)) = merge.layout.sub_hits.iter().find(|(rect, _)| rect.contains(at)) {
        return Some(merge.set_sub(sub));
    }

    None
}
