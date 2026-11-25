use super::*;

#[test]
fn merge_is_the_3rd_view_its_keys_its_context() {
    let (tx, _rx) = crate::infra::bus::channel();
    let mut view = View::new("mergetest-0000", Path::new("/tmp"), "t", tx);

    view.key(KeyEvent::new(KeyCode::Char('3'), KeyModifiers::ALT));

    assert_eq!(view.panes.pane, Pane::Merge);

    view.merge_state.merge = crate::screens::merge::tests::with(crate::screens::merge::tests::pr());

    // letters: the tab's keys (0 prompt text, i too); shift+tab: the prompt
    view.key(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::NONE));
    view.key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE));

    assert_eq!(view.prompt.input.text(), "");
    assert!(!view.panes.prompt_focus);

    view.key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT));

    assert!(view.panes.prompt_focus);

    for character in "fix the lint".chars() {
        view.key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
    }

    view.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL));
    let sent = view
        .prompt
        .pending
        .last()
        .map(|(text, ..)| text.clone())
        .unwrap_or_default();

    assert!(
        sent.starts_with("fix the lint")
            && sent.contains("[codehead: context from the merge tab]")
            && sent.contains("CI / lint"),
        "{sent}"
    );
}

#[test]
fn the_files_filter_takes_every_letter_and_esc() {
    let (tx, _rx) = crate::infra::bus::channel();
    let mut view = View::new("filesfilter-0000", Path::new("/tmp"), "t", tx);

    view.key(KeyEvent::new(KeyCode::Char('4'), KeyModifiers::ALT));

    assert_eq!(view.panes.pane, Pane::Files);

    view.key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL));

    for character in "ix".chars() {
        view.key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
    }

    // `i` went to the filter, not to the prompt focus
    assert!(view.panes.files.typing() && !view.panes.prompt_focus && view.prompt.input.text().is_empty());

    view.key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

    assert!(
        !view.panes.files.typing() && view.last_esc.is_none(),
        "esc: the filter only"
    );
}

#[test]
fn files_is_the_4th_view_with_its_keys() {
    let (tx, _rx) = crate::infra::bus::channel();
    let mut view = View::new("filestest-0000", Path::new("/tmp"), "t", tx);

    view.key(KeyEvent::new(KeyCode::Char('4'), KeyModifiers::ALT));

    assert_eq!(view.panes.pane, Pane::Files);

    // letters: view keys, 0 prompt text
    view.key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));

    assert_eq!(view.prompt.input.text(), "");

    // the menu: yazi on the worktree, for the view loop
    view.menu_action(crate::screens::quick_menu::Action::Command(14));

    assert_eq!(view.panes.yazi.as_deref(), Some(Path::new("/tmp")));

    // a pick: its path into the prompt, relative to the worktree
    view.yazi_picked(&["/tmp/src/a.rs".to_string()]);

    assert_eq!(view.prompt.input.text(), "src/a.rs ");

    // alt+1 from files: back to chat
    view.key(KeyEvent::new(KeyCode::Char('1'), KeyModifiers::ALT));

    assert_eq!(view.panes.pane, Pane::Chat);
}
