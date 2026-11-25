use super::*;
#[test]
fn panel_moves_and_jumps() {
    let note = |thread: &str| Note {
        thread: thread.into(),
        title: thread.into(),
        dir: PathBuf::new(),
        kind: Kind::Done,
        agents: vec![],
        ts: 1,
        text: String::new(),
        comment: String::new(),
    };
    let mut panel = Panel {
        notes: vec![note("a"), note("b")],
        ..Default::default()
    };

    assert!(matches!(panel.key(KeyEvent::from(KeyCode::Down)), Pick::Stay));
    assert!(matches!(panel.key(KeyEvent::from(KeyCode::Enter)), Pick::Jump(x) if x.thread == "b"));
    assert!(matches!(
        panel.key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::CONTROL)),
        Pick::Close
    ));
}
