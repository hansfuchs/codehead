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

fn pick_click(pick: &mut Pick, event: MouseEvent, ta: &mut TextArea) -> bool {
    let at = Position {
        x: event.column,
        y: event.row,
    };

    if !pick.area.contains(at) {
        if matches!(event.kind, MouseEventKind::Moved) {
            pick.hover = None;
        }

        return false;
    }

    let Some(open) = pick.open(ta) else {
        return false;
    };

    let count = open.matches.len();
    let hit = pick.rows.iter().find(|(rect, _)| rect.contains(at)).map(|&(_, i)| i);

    match event.kind {
        MouseEventKind::ScrollUp => pick.cursor = pick.cursor.saturating_sub(1),
        MouseEventKind::ScrollDown => pick.cursor = (pick.cursor + 1).min(count - 1),
        MouseEventKind::Moved => pick.hover = hit,
        MouseEventKind::Down(MouseButton::Left) => {
            if let Some(i) = hit {
                pick.pick(ta, &open, i);
            }
        }
        _ => {}
    }

    true
}

fn tab_click(tab: &mut Tab, event: MouseEvent) {
    let at = Position {
        x: event.column,
        y: event.row,
    };
    let click = matches!(event.kind, MouseEventKind::Down(MouseButton::Left));

    if tab.text_rect.contains(at) {
        if click && tab.focus == Focus::List {
            tab.focus = Focus::Text;
        }

        tab.text.mouse(event);
        return;
    }

    if tab.name_rect.contains(at) {
        if click {
            tab.focus = Focus::Name;
        }

        tab.name.mouse(event);
        return;
    }

    if !click || !tab.list_rect.contains(at) {
        return;
    }

    if tab.focus != Focus::List && !tab.store() {
        return;
    }

    tab.focus = Focus::List;

    if let Some(&(_, i)) = tab.rows.iter().find(|(rect, _)| rect.contains(at)) {
        if i == tab.list.len() {
            tab.add();
        } else {
            tab.at = i;
            tab.load();
        }
    }
}
