use super::start_screen_tests::app_with;
use super::*;

fn press(app: &mut App, code: KeyCode) {
    app.key(KeyEvent::new(code, KeyModifiers::NONE));
}

/// The tabs row in focus: a hotkey of the tab works (ctrl+f: filter), the focus goes to the content.
#[test]
fn a_tab_hotkey_works_with_the_tabs_row_in_focus() {
    let mut app = app_with(None);

    app.switch_tab(Tab::Threads);
    app.tabs_focus = true;
    app.key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL));

    assert!(app.thread_list.tlist.filtering && !app.tabs_focus);

    press(&mut app, KeyCode::Esc);
    app.switch_tab(Tab::Agents);
    app.tabs_focus = true;
    app.key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL));

    assert!(app.agent_list.afilter_editing && !app.tabs_focus);

    // the tabs row keys stay: ←→ tabs, ↑ 0 effect, q quits
    app.agent_list.afilter_editing = false;
    app.tabs_focus = true;
    press(&mut app, KeyCode::Up);

    assert!(app.tabs_focus && app.tab == Tab::Agents);
}
