use super::*;

#[test]
fn the_prompt_and_update_open_a_new_thread() {
    let mut thread = MergeTab {
        detail: Some(crate::screens::merge::tests::with(crate::screens::merge::tests::pr())),
        ..Default::default()
    };

    // tab (the start screen's focus ring): the PR's prompt
    thread.input_focus = true;

    for character in "fix it".chars() {
        thread.key(KeyEvent::from(KeyCode::Char(character)));
    }

    let key_value = thread.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL));

    assert!(
        matches!(key_value, Out::Compose(ref text) if text.starts_with("fix it\n\n[codehead: context from the merge tab]")),
        "{key_value:?}"
    );

    // esc: back to the list
    assert_eq!(thread.key(KeyEvent::from(KeyCode::Esc)), Out::Back);
    assert!(!thread.is_detail());
}
