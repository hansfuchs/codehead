use super::*;

fn key_event(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn typed(tab: &mut FlowsTab, text: &str) {
    for character in text.chars() {
        tab.key(key_event(KeyCode::Char(character)));
    }
}

#[test]
fn a_new_flow_gets_steps_a_loop_and_a_promoted_one_off() {
    let _g = crate::engine::ENV_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _ = std::fs::remove_dir_all(flows::user_dir("flows").parent().unwrap());
    let mut tab = FlowsTab::new(None);

    // n: name → the rail with the step picker open
    tab.key(key_event(KeyCode::Char('n')));
    typed(&mut tab, "my flow");
    tab.key(key_event(KeyCode::Enter));

    assert!(tab.picker.is_some() && tab.focus == Focus::Rail);

    typed(&mut tab, "plan");
    tab.key(key_event(KeyCode::Enter));

    // the 2nd step: a one-off, its editor opens
    tab.key(key_event(KeyCode::Char('a')));
    typed(&mut tab, "check the docs");
    tab.key(key_event(KeyCode::Enter));

    assert!(matches!(
        tab.editor.as_ref().map(|editor| &editor.what),
        Some(Edit::Node(1, None))
    ));

    tab.key(key_event(KeyCode::Esc));

    // L on the one-off: back to step 1, until text
    tab.key(key_event(KeyCode::Char('L')));
    tab.key(key_event(KeyCode::Up));
    tab.key(key_event(KeyCode::Enter));
    typed(&mut tab, "docs match");
    tab.key(key_event(KeyCode::Enter));
    let lib = Library::load(None);
    let flow = lib.flow("my-flow").unwrap();

    assert_eq!(flow.layer, Layer::User);

    let resolved = flows::resolve(flow, &lib);

    assert_eq!(resolved.len(), 2);
    assert_eq!(
        resolved[1]
            .loops
            .as_ref()
            .map(|loops| (loops.0, loops.1.as_str(), loops.2)),
        Some((0, "docs match", 3))
    );

    // s: the one-off becomes a shared step
    tab.key(key_event(KeyCode::Char('s')));
    tab.key(key_event(KeyCode::Enter));
    let lib = Library::load(None);

    assert_eq!(
        lib.flow("my-flow").unwrap().steps[1].uses.as_deref(),
        Some("check-the-docs")
    );
    assert_eq!(lib.step("check-the-docs").unwrap().text, "check the docs");

    let _ = std::fs::remove_dir_all(flows::user_dir("flows").parent().unwrap());
}

#[test]
fn editing_a_shared_step_from_a_flow_changes_it_for_every_flow() {
    let _g = crate::engine::ENV_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _ = std::fs::remove_dir_all(flows::user_dir("flows").parent().unwrap());
    let mut tab = FlowsTab::new(None);

    tab.open("chore");

    // enter on step 1 (implement): the text field has the focus
    tab.key(key_event(KeyCode::Enter));

    assert!(matches!(
        tab.editor.as_ref().map(|editor| &editor.what),
        Some(Edit::Node(0, Some(_)))
    ));

    typed(&mut tab, " ZZ");

    // esc: 0 save, the editor stays (edits); ctrl+s saves
    tab.key(key_event(KeyCode::Esc));

    assert!(tab.editor.is_some() && !Library::load(None).step("implement").unwrap().text.ends_with(" ZZ"));

    tab.key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL));
    let lib = Library::load(None);

    assert!(lib.step("implement").unwrap().text.ends_with(" ZZ"));
    assert!(lib.step("implement").unwrap().edited);

    // the other flows see it too
    let new_feature = lib.flow("new-feature").unwrap();

    assert!(
        flows::resolve(new_feature, &lib)
            .iter()
            .any(|node| node.text.ends_with(" ZZ"))
    );

    let _ = std::fs::remove_dir_all(flows::user_dir("flows").parent().unwrap());
}
