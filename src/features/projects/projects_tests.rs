use super::*;
use crate::domain;

fn key_event(code: KeyCode) -> KeyEvent {
    KeyEvent::from(code)
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
    let mut tab = ProjectsTab::default();

    tab.enter(vec![repo.clone()]);

    assert_eq!(tab.items, [] as [domain::projects::Project; 0]);

    tab.key(key_event(KeyCode::Char('n')));

    assert!(
        tab.form.as_ref().is_some_and(|form| form.on_title),
        "n: the form, on the title"
    );

    for character in "Bug fixes".chars() {
        tab.key(key_event(KeyCode::Char(character)));
    }

    tab.key(key_event(KeyCode::Enter));

    assert!(tab.form.as_ref().is_some_and(|form| !form.on_title), "enter: the brief");

    for character in "ship it".chars() {
        tab.key(key_event(KeyCode::Char(character)));
    }

    assert!(matches!(
        tab.key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL)),
        Out::Toast(_)
    ));
    assert!(tab.form.is_none());
    assert_eq!(tab.items.len(), 1);

    let text = std::fs::read_to_string(&tab.items[0].path).unwrap();

    assert!(
        text.starts_with("---\ntitle: Bug fixes\n---\n\nGoal: ship it\n"),
        "{text}"
    );

    tab.key(key_event(KeyCode::Enter));

    assert_eq!(
        tab.form.as_ref().map(|form| form.title.text()),
        Some("Bug fixes".into())
    );

    tab.key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL));

    assert!(tab.form.is_none());

    tab.key(key_event(KeyCode::Char('x')));

    assert!(tab.items[0].done);

    tab.key(key_event(KeyCode::Char('d')));
    tab.key(key_event(KeyCode::Char('d')));

    assert_eq!(tab.items, [] as [domain::projects::Project; 0]);
    assert_eq!(tab.key(key_event(KeyCode::Up)), Out::None);
    assert_eq!(tab.key(key_event(KeyCode::Up)), Out::Up);

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
