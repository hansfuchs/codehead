use super::{Action, Level, Menu, MouseButton, MouseEvent, MouseEventKind, Position, Tab, thread_list};

impl Menu {
    /// A click: a tab label switches, a row runs (agents: selects; remove needs enter). Outside: close.
    pub(crate) fn mouse(&mut self, event: MouseEvent) -> Action {
        dispatch_mouse(self, event)
    }
}

fn dispatch_mouse(widget_state: &mut Menu, event: MouseEvent) -> Action {
    widget_state.poll();
    let at = Position {
        x: event.column,
        y: event.row,
    };
    let down = matches!(event.kind, MouseEventKind::Down(MouseButton::Left));

    // the threads tab: the thread list's own clicks (filter, tabs, cards, actions)
    if widget_state.tab == Tab::Threads
        && widget_state.layout.frame.contains(at)
        && !(down
            && widget_state
                .layout
                .tab_rects
                .iter()
                .any(|(result, _)| result.contains(at)))
    {
        let out = widget_state.list.mouse(event);

        if out != thread_list::Out::Pass {
            widget_state.level = Level::Body;
            return widget_state.list_out(out);
        }
    }

    // the search input: its own mouse (cursor, selection + copy)
    if widget_state.tab == Tab::Search && widget_state.search_state.search.mouse(event) {
        widget_state.search_state.search.copied = None;
        widget_state.level = Level::Body;
        return Action::None;
    }

    match event.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            if let Some(value) = dispatch_click(widget_state, event, at) {
                return value;
            }
        }
        MouseEventKind::ScrollDown | MouseEventKind::ScrollUp if widget_state.tab == Tab::Merge => {
            widget_state.prs.mouse(event);
        }
        MouseEventKind::ScrollDown if widget_state.tab == Tab::Search => {
            widget_state.search_state.scursor =
                (widget_state.search_state.scursor + 1).min(widget_state.search_state.hits.len().saturating_sub(1));
        }
        MouseEventKind::ScrollUp if widget_state.tab == Tab::Search => {
            widget_state.search_state.scursor = widget_state.search_state.scursor.saturating_sub(1);
        }
        MouseEventKind::ScrollDown if widget_state.tab == Tab::Tokens => {
            widget_state.tokens.dscroll = widget_state.tokens.dscroll.saturating_add(1);
        }
        MouseEventKind::ScrollUp if widget_state.tab == Tab::Tokens => {
            widget_state.tokens.dscroll = widget_state.tokens.dscroll.saturating_sub(1);
        }
        _ => {}
    }

    Action::None
}

fn dispatch_click(widget_state: &mut Menu, event: MouseEvent, at: Position) -> Option<Action> {
    if !widget_state.layout.frame.contains(at) {
        return Some(Action::Close);
    }

    if let Some((_, thread)) = widget_state
        .layout
        .tab_rects
        .iter()
        .find(|(result_value, _)| result_value.contains(at))
    {
        widget_state.tab = *thread;
        widget_state.level = Level::Bar;
        return Some(Action::None);
    }

    if widget_state.tab == Tab::Merge {
        widget_state.level = if widget_state.prs.tab_at(at) {
            Level::Group
        } else {
            Level::Body
        };

        return Some(match widget_state.prs.mouse(event) {
            crate::screens::merge::pr_list::Pick::Open(error) => Action::Pr(error),
            _ => Action::None,
        });
    }

    let Some(&(_, i)) = widget_state
        .layout
        .rows
        .iter()
        .find(|(result_2, _)| result_2.contains(at))
    else {
        return Some(Action::None);
    };

    widget_state.level = Level::Body;

    match widget_state.tab {
        Tab::Quick => {
            widget_state.qcursor = i;
            return Some(widget_state.run_item(i));
        }
        Tab::Search => {
            widget_state.search_state.scursor = i;
            return Some(widget_state.jump(i));
        }
        Tab::Tokens | Tab::Merge | Tab::Threads => {}
    }

    None
}
