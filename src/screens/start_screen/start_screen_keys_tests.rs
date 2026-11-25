use super::start_screen_tests::app_with;
use super::*;
use ratatui::backend::TestBackend;

fn key(app: &mut App, code: KeyCode, message: KeyModifiers) {
    app.key(KeyEvent::new(code, message));
}

#[test]
fn q_on_the_tabs_row_never_quits() {
    let mut app = app_with(None);

    app.tabs_focus = true;
    key(&mut app, KeyCode::Char('q'), KeyModifiers::NONE);

    assert!(app.outcome.is_none());

    // esc on the tabs row: 0 move into the content
    app.tabs_focus = true;
    app.overlays.last_esc = None;
    key(&mut app, KeyCode::Esc, KeyModifiers::NONE);

    assert!(app.tabs_focus && app.outcome.is_none());
}

#[test]
fn ctrl_c_clears_the_field_then_quits_on_a_2nd_press() {
    let mut app = app_with(None);

    app.compose.field = Field::Prompt;
    app.compose.prompt.set_text("a task");
    key(&mut app, KeyCode::Char('c'), KeyModifiers::CONTROL);

    assert_eq!(app.compose.prompt.text(), "");
    assert!(app.outcome.is_none());

    key(&mut app, KeyCode::Char('z'), KeyModifiers::CONTROL);

    assert_eq!(app.compose.prompt.text(), "a task", "ctrl+z: the text back");

    app.compose.prompt.set_text("");
    key(&mut app, KeyCode::Char('c'), KeyModifiers::CONTROL);

    assert!(app.outcome.is_none(), "1st ctrl+c on an empty field: a hint");

    key(&mut app, KeyCode::Char('c'), KeyModifiers::CONTROL);

    assert!(matches!(app.outcome, Some(Outcome::Quit)));
}

#[test]
fn a_click_on_no_brings_the_agent_form_back() {
    let mut app = app_with(None);
    let form = AgentForm::new(None, &app.agent_list.styles, None);

    app.overlays.modal = Some(Modal::Confirm {
        title: "discard".into(),
        question: "drop the new agent?".into(),
        yes: false,
        action: ConfirmAction::DiscardAgent(Box::new(form)),
    });
    let mut thread = ratatui::Terminal::new(TestBackend::new(110, 40)).unwrap();

    thread.draw(|file| app.draw(file)).unwrap();
    let (result, _) = *app
        .preview
        .regions
        .buttons
        .iter()
        .find(|(_, yes)| !yes)
        .expect("a no button");

    app.mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: result.x + 1,
        row: result.y,
        modifiers: KeyModifiers::NONE,
    });

    assert!(
        matches!(app.overlays.modal, Some(Modal::Agent(_))),
        "no: the form with its edits"
    );
}

#[test]
fn a_capital_letter_is_not_its_small_command() {
    let shift_d = KeyEvent::new(KeyCode::Char('D'), KeyModifiers::SHIFT);

    assert_eq!(key_name(&shift_d), "D");
    assert_eq!(key_name(&KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE)), "d");
}
