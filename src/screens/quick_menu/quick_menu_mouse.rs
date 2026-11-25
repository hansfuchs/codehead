use super::{Action, Level, Menu, MouseButton, MouseEvent, MouseEventKind, Position, Tab, thread_list};

impl Menu {
    /// A click: a tab label switches, a row runs (agents: selects; remove needs enter). Outside: close.
    pub(crate) fn mouse(&mut self, event: MouseEvent) -> Action {
        dispatch_mouse(self, event)
    }
}

fn dispatch_mouse(menu: &mut Menu, event: MouseEvent) -> Action {
    menu.poll();
    let at = Position {
        x: event.column,
        y: event.row,
    };
    let down = matches!(event.kind, MouseEventKind::Down(MouseButton::Left));

    // the threads tab: the thread list's own clicks (filter, tabs, cards, actions)
    if menu.tab == Tab::Threads
        && menu.layout.frame.contains(at)
        && !(down && menu.layout.tab_rects.iter().any(|(rect, _)| rect.contains(at)))
    {
        let out = menu.list.mouse(event);

        if out != thread_list::Out::Pass {
            menu.level = Level::Body;
            return menu.list_out(out);
        }
    }

    // the search input: its own mouse (cursor, selection + copy)
    if menu.tab == Tab::Search && menu.search_state.search.mouse(event) {
        menu.search_state.search.copied = None;
        menu.level = Level::Body;
        return Action::None;
    }

    match event.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            if let Some(action) = dispatch_click(menu, event, at) {
                return action;
            }
        }
        MouseEventKind::ScrollDown | MouseEventKind::ScrollUp if menu.tab == Tab::Merge => {
            menu.prs.mouse(event);
        }
        MouseEventKind::ScrollDown if menu.tab == Tab::Search => {
            menu.search_state.scursor =
                (menu.search_state.scursor + 1).min(menu.search_state.hits.len().saturating_sub(1));
        }
        MouseEventKind::ScrollUp if menu.tab == Tab::Search => {
            menu.search_state.scursor = menu.search_state.scursor.saturating_sub(1);
        }
        MouseEventKind::ScrollDown if menu.tab == Tab::Tokens => {
            menu.tokens.dscroll = menu.tokens.dscroll.saturating_add(1);
        }
        MouseEventKind::ScrollUp if menu.tab == Tab::Tokens => {
            menu.tokens.dscroll = menu.tokens.dscroll.saturating_sub(1);
        }
        _ => {}
    }

    Action::None
}

fn dispatch_click(menu: &mut Menu, event: MouseEvent, at: Position) -> Option<Action> {
    if !menu.layout.frame.contains(at) {
        return Some(Action::Close);
    }

    if let Some((_, tab)) = menu.layout.tab_rects.iter().find(|(rect, _)| rect.contains(at)) {
        menu.tab = *tab;
        menu.level = Level::Bar;
        return Some(Action::None);
    }

    if menu.tab == Tab::Merge {
        menu.level = if menu.prs.tab_at(at) { Level::Group } else { Level::Body };

        return Some(match menu.prs.mouse(event) {
            crate::screens::merge::pr_list::Pick::Open(entry) => Action::Pr(entry),
            _ => Action::None,
        });
    }

    let Some(&(_, i)) = menu.layout.rows.iter().find(|(rect, _)| rect.contains(at)) else {
        return Some(Action::None);
    };

    menu.level = Level::Body;

    match menu.tab {
        Tab::Quick => {
            menu.qcursor = i;
            return Some(menu.run_item(i));
        }
        Tab::Search => {
            menu.search_state.scursor = i;
            return Some(menu.jump(i));
        }
        Tab::Tokens | Tab::Merge | Tab::Threads => {}
    }

    None
}
