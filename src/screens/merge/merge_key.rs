use super::{Act, Instant, KeyCode, KeyEvent, KeyModifiers, Merge, Stop, Sub, Tab};

impl Merge {
    pub(crate) fn key(&mut self, key: KeyEvent) -> Act {
        dispatch_key(self, key)
    }
}

fn dispatch_key(merge: &mut Merge, key: KeyEvent) -> Act {
    merge.active = Instant::now();
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

    if merge.prs.is_empty() {
        return match key.code {
            KeyCode::Char('r') => merge.manual(),
            _ => Act::None,
        };
    }

    // an inline input: its keys
    if merge.tab_ref().is_some_and(|tab| tab.input.edit.is_some()) {
        return merge.edit_key(key);
    }

    if merge.tab_ref().is_some_and(|tab| tab.input.menu.is_some()) {
        return merge.menu_key(key);
    }

    // [ ] < >: PR and sub-view, never while the PR diff's filter / comment takes text
    if !merge.typing() {
        match key.code {
            KeyCode::Char('[') => return merge.step_pr(-1),
            KeyCode::Char(']') => return merge.step_pr(1),
            KeyCode::Char('<') => return merge.set_sub(Sub::Overview),
            KeyCode::Char('>') => return merge.set_sub(Sub::Changes),
            _ => {}
        }
    }

    if merge.tab_ref().is_some_and(|tab| tab.sub == Sub::Changes) {
        return key_changes(merge, key);
    }

    let stops = merge.stops();

    let Some(tab) = merge.tab() else {
        return Act::None;
    };

    match navigate_overview(tab, key, ctrl, &stops) {
        OverviewAction::None => Act::None,
        OverviewAction::Navigate(action) => apply_navigation(merge, action),
        OverviewAction::Refresh => merge.manual(),
        OverviewAction::Open(stop) => merge.open_on_github(stop),
        OverviewAction::Activate(stop) => merge.activate(stop),
    }
}

enum OverviewAction {
    None,
    Navigate(Navigation),
    Refresh,
    Open(Stop),
    Activate(Stop),
}

fn navigate_overview(tab: &mut Tab, key: KeyEvent, ctrl: bool, stops: &[Stop]) -> OverviewAction {
    tab.free = false;
    let cur = tab
        .focus
        .filter(|stop| stops.contains(stop))
        .or(stops.get(2).copied())
        .or(stops.first().copied());

    tab.focus = cur;
    let pos = cur
        .and_then(|stop| stops.iter().position(|other| *other == stop))
        .unwrap_or(0);
    let last = stops.len().saturating_sub(1);
    let mv = |tab: &mut Tab, i: usize| tab.focus = stops.get(i).copied();

    match key.code {
        // the last stop (the description): ↓ scrolls on to its end
        KeyCode::Down | KeyCode::Char('j') if pos == last && cur.is_some() => {
            tab.free = true;
            tab.scroll += 1;
        }
        KeyCode::Down | KeyCode::Char('j') => move_down(tab, stops, pos, last),
        KeyCode::Up | KeyCode::Char('k') if pos == last && tab.free && tab.scroll > 0 => {
            tab.free = true;
            tab.scroll -= 1;
        }
        KeyCode::Up | KeyCode::Char('k') => move_up(tab, stops, pos),
        KeyCode::Home | KeyCode::Char('g') => mv(tab, 0),
        KeyCode::End | KeyCode::Char('G') => mv(tab, last),
        KeyCode::PageDown => mv(tab, (pos + 6).min(last)),
        KeyCode::PageUp => mv(tab, pos.saturating_sub(6)),
        KeyCode::Left | KeyCode::Right => {
            if let Some(navigation) = navigate_horizontal(tab, key, stops, cur, pos) {
                return OverviewAction::Navigate(navigation);
            }
        }
        KeyCode::Char('r') => return OverviewAction::Refresh,
        // o: the stop on GitHub (open in the browser)
        KeyCode::Char('o') if !ctrl => {
            if let Some(stop) = cur {
                return OverviewAction::Open(stop);
            }
        }
        KeyCode::Enter => {
            if let Some(stop) = cur {
                return OverviewAction::Activate(stop);
            }
        }
        _ => {}
    }

    OverviewAction::None
}

fn navigate_horizontal(
    tab: &mut Tab,
    key: KeyEvent,
    stops: &[Stop],
    cur: Option<Stop>,
    pos: usize,
) -> Option<Navigation> {
    let right = key.code == KeyCode::Right;

    match cur {
        Some(Stop::Tabs) => return Some(Navigation::Tabs(if right { 1 } else { -1 })),
        Some(Stop::Subs) => return Some(Navigation::Sub(if right { Sub::Changes } else { Sub::Overview })),
        Some(Stop::Checks) => tab.checks_open = right,
        Some(Stop::Check(_)) if !right => {
            tab.checks_open = false;
            tab.focus = Some(Stop::Checks);
        }
        Some(stop) if stop.row() != 0 => {
            let index = if right { pos + 1 } else { pos.wrapping_sub(1) };

            if stops.get(index).is_some_and(|next| next.row() == stop.row()) {
                tab.focus = stops.get(index).copied();
            }
        }
        _ => {}
    }

    None
}

#[derive(Clone, Copy)]
enum Navigation {
    Tabs(isize),
    Sub(Sub),
}

fn apply_navigation(merge: &mut Merge, navigation: Navigation) -> Act {
    match navigation {
        Navigation::Tabs(direction) => {
            let action = merge.step_pr(direction);

            if let Some(tab) = merge.tab() {
                tab.focus = Some(Stop::Tabs);
            }

            action
        }
        Navigation::Sub(sub) => merge.set_sub(sub),
    }
}

fn move_down(tab: &mut Tab, stops: &[Stop], pos: usize, last: usize) {
    // the next stop of another row (a row of stops: its 1st)
    let row = stops.get(pos).map_or(0, |stop| stop.row());
    let mut i = pos + 1;

    while row != 0 && i <= last && stops[i].row() == row {
        i += 1;
    }

    tab.focus = stops.get(i.min(last)).copied();
}

fn move_up(tab: &mut Tab, stops: &[Stop], pos: usize) {
    let row = stops.get(pos).map_or(0, |stop| stop.row());
    let mut i = pos;

    while row != 0 && i > 0 && stops[i - 1].row() == row {
        i -= 1;
    }

    let i = i.saturating_sub(1);

    // into a row: its 1st stop
    let target_row = stops[i].row();
    let mut j = i;

    while target_row != 0 && j > 0 && stops[j - 1].row() == target_row {
        j -= 1;
    }

    tab.focus = stops.get(j).copied();
}

fn key_changes(merge: &mut Merge, key: KeyEvent) -> Act {
    // r (ctrl+r too): the PR read again, as on every screen
    if key.code == KeyCode::Char('r')
        && !key.modifiers.intersects(KeyModifiers::ALT | KeyModifiers::SUPER)
        && !merge.typing()
    {
        return merge.manual();
    }

    // the bar rows (PR tabs, views) with the focus: their keys, as on the overview
    if let Some(stop) = merge
        .tab_ref()
        .and_then(|tab| tab.focus)
        .filter(|stop| matches!(stop, Stop::Tabs | Stop::Subs))
    {
        return merge.bar_key(stop, key);
    }

    let Some(view) = merge.changes_view() else {
        return Act::None;
    };

    // ↑ on the diff's 1st row: the views row (as ↑ from the overview's 1st stop)
    if key.code == KeyCode::Up && key.modifiers.is_empty() && view.at_top() {
        if let Some(tab) = merge.tab() {
            tab.focus = Some(Stop::Subs);
        }

        return Act::None;
    }

    match view.key(key) {
        crate::features::changes::Act::Toast(toast) => Act::Toast(toast),
        crate::features::changes::Act::Open(file, line) => Act::Open(file, line),
        crate::features::changes::Act::Send(prompt) => Act::Prompt(prompt),
        _ => Act::None,
    }
}
