use super::*;

#[test]
fn the_prompt_and_update_open_a_new_thread() {
    let mut tab = MergeTab {
        detail: Some(crate::screens::merge::tests::with(crate::screens::merge::tests::pr())),
        ..Default::default()
    };

    // tab (the start screen's focus ring): the PR's prompt
    tab.input_focus = true;

    for character in "fix it".chars() {
        tab.key(KeyEvent::from(KeyCode::Char(character)));
    }

    let out = tab.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL));

    assert!(
        matches!(out, Out::Compose(ref text) if text.starts_with("fix it\n\n[codehead: context from the merge tab]")),
        "{out:?}"
    );

    // esc: back to the list
    assert_eq!(tab.key(KeyEvent::from(KeyCode::Esc)), Out::Back);
    assert!(!tab.is_detail());
}
