use super::*;
use crate::domain;

fn key_event(character: KeyCode) -> KeyEvent {
    KeyEvent::from(character)
}

#[test]
fn new_edit_done_delete() {
    let _g = crate::engine::ENV_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let repo = std::env::temp_dir()
        .join(format!("ch-projtab-{}", std::process::id()))
        .join("demo");
    let _ = std::fs::remove_dir_all(projects::dir(&repo));
    let mut thread = ProjectsTab::default();

    thread.enter(vec![repo.clone()]);

    assert_eq!(thread.items, [] as [domain::projects::Project; 0]);

    thread.key(key_event(KeyCode::Char('n')));

    assert!(
        thread.form.as_ref().is_some_and(|file| file.on_title),
        "n: the form, on the title"
    );

    for character in "Bug fixes".chars() {
        thread.key(key_event(KeyCode::Char(character)));
    }

    thread.key(key_event(KeyCode::Enter));

    assert!(
        thread.form.as_ref().is_some_and(|file_value| !file_value.on_title),
        "enter: the brief"
    );

    for character_value in "ship it".chars() {
        thread.key(key_event(KeyCode::Char(character_value)));
    }

    assert!(matches!(
        thread.key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL)),
        Out::Toast(_)
    ));
    assert!(thread.form.is_none());
    assert_eq!(thread.items.len(), 1);

    let text = std::fs::read_to_string(&thread.items[0].path).unwrap();

    assert!(
        text.starts_with("---\ntitle: Bug fixes\n---\n\nGoal: ship it\n"),
        "{text}"
    );

    thread.key(key_event(KeyCode::Enter));

    assert_eq!(
        thread.form.as_ref().map(|file_2| file_2.title.text()),
        Some("Bug fixes".into())
    );

    thread.key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL));

    assert!(thread.form.is_none());

    thread.key(key_event(KeyCode::Char('x')));

    assert!(thread.items[0].done);

    thread.key(key_event(KeyCode::Char('d')));
    thread.key(key_event(KeyCode::Char('d')));

    assert_eq!(thread.items, [] as [domain::projects::Project; 0]);
    assert_eq!(thread.key(key_event(KeyCode::Up)), Out::None);
    assert_eq!(thread.key(key_event(KeyCode::Up)), Out::Up);

    let _ = std::fs::remove_dir_all(projects::dir(&repo));
}

#[test]
fn drawn_form_keeps_title_when_mouse_selects_brief() {
    let _guard = crate::engine::ENV_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut tab = ProjectsTab::default();

    tab.enter(vec![
        std::env::temp_dir().join(format!("ch-project-form-{}", std::process::id())),
    ]);
    tab.key(key_event(KeyCode::Char('n')));

    for character in "Visible title".chars() {
        tab.key(key_event(KeyCode::Char(character)));
    }

    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 28)).unwrap();

    terminal.draw(|frame| tab.draw(frame, frame.area(), true)).unwrap();
    let drawn: String = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(ratatui::buffer::Cell::symbol)
        .collect();

    assert!(drawn.contains("Visible title"));
    assert!(drawn.contains("brief"));

    let brief = tab.form.as_ref().unwrap().body.area();

    tab.mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: brief.x + 1,
        row: brief.y,
        modifiers: KeyModifiers::NONE,
    });
    tab.key(key_event(KeyCode::Char('x')));
    let form = tab.form.as_ref().unwrap();

    assert!(!form.on_title);
    assert_eq!(form.title.text(), "Visible title");
    assert!(form.body.text().contains('x'));
}
