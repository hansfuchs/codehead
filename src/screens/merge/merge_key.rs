use super::{Act, Instant, KeyCode, KeyEvent, KeyModifiers, Merge, Stop, Sub, Tab};

impl Merge {
    pub(crate) fn key(&mut self, key_value: KeyEvent) -> Act {
        dispatch_key(self, key_value)
    }
}

fn dispatch_key(widget_state: &mut Merge, key_value: KeyEvent) -> Act {
    widget_state.active = Instant::now();
    let ctrl = key_value.modifiers.contains(KeyModifiers::CONTROL);

    if widget_state.prs.is_empty() {
        return match key_value.code {
            KeyCode::Char('r') => widget_state.manual(),
            _ => Act::None,
        };
    }

    // an inline input: its keys
    if widget_state.tab_ref().is_some_and(|thread| thread.input.edit.is_some()) {
        return widget_state.edit_key(key_value);
    }

    if widget_state
        .tab_ref()
        .is_some_and(|thread_value| thread_value.input.menu.is_some())
    {
        return widget_state.menu_key(key_value);
    }

    // [ ] < >: PR and sub-view, never while the PR diff's filter / comment takes text
    if !widget_state.typing() {
        match key_value.code {
            KeyCode::Char('[') => return widget_state.step_pr(-1),
            KeyCode::Char(']') => return widget_state.step_pr(1),
            KeyCode::Char('<') => return widget_state.set_sub(Sub::Overview),
            KeyCode::Char('>') => return widget_state.set_sub(Sub::Changes),
            _ => {}
        }
    }

    if widget_state
        .tab_ref()
        .is_some_and(|changes_value| changes_value.sub == Sub::Changes)
    {
        return key_changes(widget_state, key_value);
    }

    let stops = widget_state.stops();

    let Some(tab) = widget_state.tab() else {
        return Act::None;
    };

    match navigate_overview(tab, key_value, ctrl, &stops) {
        OverviewAction::None => Act::None,
        OverviewAction::Navigate(action) => apply_navigation(widget_state, action),
        OverviewAction::Refresh => widget_state.manual(),
        OverviewAction::Open(stop) => widget_state.open_on_github(stop),
        OverviewAction::Activate(stop) => widget_state.activate(stop),
    }
}

enum OverviewAction {
    None,
    Navigate(Navigation),
    Refresh,
    Open(Stop),
    Activate(Stop),
}

fn navigate_overview(tab: &mut Tab, key_value: KeyEvent, ctrl: bool, stops: &[Stop]) -> OverviewAction {
    tab.free = false;
    let cur = tab
        .focus
        .filter(|text_2| stops.contains(text_2))
        .or(stops.get(2).copied())
        .or(stops.first().copied());

    tab.focus = cur;
    let pos = cur
        .and_then(|character_value| stops.iter().position(|text_3| *text_3 == character_value))
        .unwrap_or(0);
    let last = stops.len().saturating_sub(1);
    let mv = |tab: &mut Tab, i: usize| tab.focus = stops.get(i).copied();

    match key_value.code {
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
            if let Some(value) = navigate_horizontal(tab, key_value, stops, cur, pos) {
                return OverviewAction::Navigate(value);
            }
        }
        KeyCode::Char('r') => return OverviewAction::Refresh,
        // o: the stop on GitHub (open in the browser)
        KeyCode::Char('o') if !ctrl => {
            if let Some(text_7) = cur {
                return OverviewAction::Open(text_7);
            }
        }
        KeyCode::Enter => {
            if let Some(text_8) = cur {
                return OverviewAction::Activate(text_8);
            }
        }
        _ => {}
    }

    OverviewAction::None
}

fn navigate_horizontal(
    tab: &mut Tab,
    key_value: KeyEvent,
    stops: &[Stop],
    cur: Option<Stop>,
    pos: usize,
) -> Option<Navigation> {
    let right = key_value.code == KeyCode::Right;

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
fn apply_navigation(view: &mut Merge, navigation: Navigation) -> Act {
    match navigation {
        Navigation::Tabs(direction) => {
            let action = view.step_pr(direction);

            if let Some(tab) = view.tab() {
                tab.focus = Some(Stop::Tabs);
            }

            action
        }
        Navigation::Sub(sub) => view.set_sub(sub),
    }
}

fn move_down(tab: &mut Tab, stops: &[Stop], pos: usize, last: usize) {
    // the next stop of another row (a row of stops: its 1st)
    let row = stops.get(pos).map_or(0, |text_4| text_4.row());
    let mut i = pos + 1;

    while row != 0 && i <= last && stops[i].row() == row {
        i += 1;
    }

    tab.focus = stops.get(i.min(last)).copied();
}

fn move_up(tab: &mut Tab, stops: &[Stop], pos: usize) {
    let row = stops.get(pos).map_or(0, |text_5| text_5.row());
    let mut i = pos;

    while row != 0 && i > 0 && stops[i - 1].row() == row {
        i -= 1;
    }

    let i = i.saturating_sub(1);

    // into a row: its 1st stop
    let row_value = stops[i].row();
    let mut j = i;

    while row_value != 0 && j > 0 && stops[j - 1].row() == row_value {
        j -= 1;
    }

    tab.focus = stops.get(j).copied();
}

fn key_changes(widget_state: &mut Merge, key_value: KeyEvent) -> Act {
    // r (ctrl+r too): the PR read again, as on every screen
    if key_value.code == KeyCode::Char('r')
        && !key_value.modifiers.intersects(KeyModifiers::ALT | KeyModifiers::SUPER)
        && !widget_state.typing()
    {
        return widget_state.manual();
    }

    // the bar rows (PR tabs, views) with the focus: their keys, as on the overview
    if let Some(text_value) = widget_state
        .tab_ref()
        .and_then(|thread_2| thread_2.focus)
        .filter(|text| matches!(text, Stop::Tabs | Stop::Subs))
    {
        return widget_state.bar_key(text_value, key_value);
    }

    let Some(character) = widget_state.changes_view() else {
        return Act::None;
    };

    // ↑ on the diff's 1st row: the views row (as ↑ from the overview's 1st stop)
    if key_value.code == KeyCode::Up && key_value.modifiers.is_empty() && character.at_top() {
        if let Some(thread_3) = widget_state.tab() {
            thread_3.focus = Some(Stop::Subs);
        }

        return Act::None;
    }

    match character.key(key_value) {
        crate::features::changes::Act::Toast(thread_4) => Act::Toast(thread_4),
        crate::features::changes::Act::Open(file, line) => Act::Open(file, line),
        crate::features::changes::Act::Send(thread_5) => Act::Prompt(thread_5),
        _ => Act::None,
    }
}
