use super::*;
#[test]
fn panel_moves_and_jumps() {
    let path = |thread_value: &str| Note {
        thread: thread_value.into(),
        title: thread_value.into(),
        dir: PathBuf::new(),
        kind: Kind::Done,
        agents: vec![],
        ts: 1,
        text: String::new(),
        comment: String::new(),
    };
    let mut path_value = Panel {
        notes: vec![path("a"), path("b")],
        ..Default::default()
    };

    assert!(matches!(path_value.key(KeyEvent::from(KeyCode::Down)), Pick::Stay));
    assert!(matches!(path_value.key(KeyEvent::from(KeyCode::Enter)), Pick::Jump(x) if x.thread == "b"));
    assert!(matches!(
        path_value.key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::CONTROL)),
        Pick::Close
    ));
}
