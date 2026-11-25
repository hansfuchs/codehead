use super::start_screen_tests::{app_with, dump};
use super::*;

fn click(app: &mut App, area: Rect) {
    app.mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: area.x,
        row: area.y,
        modifiers: KeyModifiers::NONE,
    });
}

#[test]
fn repo_chip_click_opens_picker_without_dropping_prompt() {
    let mut app = app_with(None);

    app.compose.prompt.set_text("retain this task");
    let _ = dump(&mut app, 120, 42);
    let area = app.preview.regions.repo_row;

    click(&mut app, area);

    assert!(app.repository.repo_pick.is_some());
    assert_eq!(app.compose.prompt.text(), "retain this task");

    let _ = std::fs::remove_dir_all(&app.cfg.drafts);
}

#[test]
fn notes_label_click_opens_notes_without_dropping_prompt() {
    let mut app = app_with(None);

    app.compose.prompt.set_text("retain this task");
    let _ = dump(&mut app, 120, 42);
    let area = app.preview.regions.notes_label;

    click(&mut app, area);

    assert!(app.compose.screen == Screen::Pick);
    assert_eq!(app.tab, Tab::Notes);
    assert_eq!(app.compose.prompt.text(), "retain this task");

    let _ = std::fs::remove_dir_all(&app.cfg.drafts);
}

#[test]
fn save_draft_click_keeps_task() {
    let mut app = app_with(None);

    app.compose.title.set_text("kept task");
    app.compose.prompt.set_text("retain this task");
    let _ = dump(&mut app, 120, 42);
    let area = app.preview.regions.save_draft;

    click(&mut app, area);
    let saved = drafts::load(&app.cfg.drafts, "");

    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0].title, "kept task");
    assert_eq!(saved[0].prompt, "retain this task");
    assert!(saved[0].kept);
    assert_eq!(app.compose.prompt.text(), "");

    let _ = std::fs::remove_dir_all(&app.cfg.drafts);
}

#[test]
fn checkout_click_changes_selected_option() {
    let mut app = app_with(None);

    app.compose.prompt.set_text("retain this task");
    app.cfg.repo = Some(app.cfg.cwd.clone());
    app.repository.wheres = vec![Where::Branch("feature".into())];
    app.repository.where_i = 0;
    app.repository.here_branch = "main".into();
    app.repository.repo_branch = "main".into();
    let _ = dump(&mut app, 120, 42);
    let area = app
        .preview
        .regions
        .opts
        .iter()
        .find(|(_, index)| *index == 0)
        .unwrap()
        .0;

    assert!(app.checkout_box().1);
    assert!(app.repository.checkout);

    click(&mut app, area);

    assert!(!app.repository.checkout);
    assert_eq!(app.compose.prompt.text(), "retain this task");

    let _ = std::fs::remove_dir_all(&app.cfg.drafts);
}
