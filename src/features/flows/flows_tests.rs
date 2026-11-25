use super::*;

fn key_event(character: KeyCode) -> KeyEvent {
    KeyEvent::new(character, KeyModifiers::NONE)
}

fn typed(thread: &mut FlowsTab, text: &str) {
    for character in text.chars() {
        thread.key(key_event(KeyCode::Char(character)));
    }
}

#[test]
fn a_new_flow_gets_steps_a_loop_and_a_promoted_one_off() {
    let _g = crate::engine::ENV_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _ = std::fs::remove_dir_all(flows::user_dir("flows").parent().unwrap());
    let mut thread = FlowsTab::new(None);

    // n: name → the rail with the step picker open
    thread.key(key_event(KeyCode::Char('n')));
    typed(&mut thread, "my flow");
    thread.key(key_event(KeyCode::Enter));

    assert!(thread.picker.is_some() && thread.focus == Focus::Rail);

    typed(&mut thread, "plan");
    thread.key(key_event(KeyCode::Enter));

    // the 2nd step: a one-off, its editor opens
    thread.key(key_event(KeyCode::Char('a')));
    typed(&mut thread, "check the docs");
    thread.key(key_event(KeyCode::Enter));

    assert!(matches!(
        thread.editor.as_ref().map(|error_value| &error_value.what),
        Some(Edit::Node(1, None))
    ));

    thread.key(key_event(KeyCode::Esc));

    // L on the one-off: back to step 1, until text
    thread.key(key_event(KeyCode::Char('L')));
    thread.key(key_event(KeyCode::Up));
    thread.key(key_event(KeyCode::Enter));
    typed(&mut thread, "docs match");
    thread.key(key_event(KeyCode::Enter));
    let lib = Library::load(None);
    let file = lib.flow("my-flow").unwrap();

    assert_eq!(file.layer, Layer::User);

    let result = flows::resolve(file, &lib);

    assert_eq!(result.len(), 2);
    assert_eq!(
        result[1].loops.as_ref().map(|line| (line.0, line.1.as_str(), line.2)),
        Some((0, "docs match", 3))
    );

    // s: the one-off becomes a shared step
    thread.key(key_event(KeyCode::Char('s')));
    thread.key(key_event(KeyCode::Enter));
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
    let mut thread = FlowsTab::new(None);

    thread.open("chore");

    // enter on step 1 (implement): the text field has the focus
    thread.key(key_event(KeyCode::Enter));

    assert!(matches!(
        thread.editor.as_ref().map(|error_value| &error_value.what),
        Some(Edit::Node(0, Some(_)))
    ));

    typed(&mut thread, " ZZ");

    // esc: 0 save, the editor stays (edits); ctrl+s saves
    thread.key(key_event(KeyCode::Esc));

    assert!(thread.editor.is_some() && !Library::load(None).step("implement").unwrap().text.ends_with(" ZZ"));

    thread.key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL));
    let lib = Library::load(None);

    assert!(lib.step("implement").unwrap().text.ends_with(" ZZ"));
    assert!(lib.step("implement").unwrap().edited);

    // the other flows see it too
    let nf = lib.flow("new-feature").unwrap();

    assert!(flows::resolve(nf, &lib).iter().any(|count| count.text.ends_with(" ZZ")));

    let _ = std::fs::remove_dir_all(flows::user_dir("flows").parent().unwrap());
}
