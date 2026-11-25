use super::*;
use ratatui::backend::TestBackend;

fn screen(view: &mut View) -> Vec<String> {
    let mut thread = Terminal::new(TestBackend::new(60, 30)).unwrap();

    thread.draw(|file| view.draw(file)).unwrap();
    let bytes = thread.backend().buffer().clone();

    (0..30)
        .map(|y| (0..60).map(|x| bytes[(x, y)].symbol().to_string()).collect::<String>())
        .collect()
}

fn mouse(view: &mut View, kind: MouseEventKind, x: u16, y: u16) {
    view.mouse(MouseEvent {
        kind,
        column: x,
        row: y,
        modifiers: KeyModifiers::NONE,
    });
}

#[test]
fn a_drag_stays_in_its_message_and_copies_unwrapped_lines() {
    let (tx, _rx) = crate::infra::bus::channel();
    let mut view = View::new("selecttest-0000", Path::new("/tmp"), "t", tx);

    view.agents.members = vec!["ada".into()];
    let long = "alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu nu xi omicron pi rho";

    view.thread.push(&store::Event {
        ts: 1,
        agent: None,
        kind: store::Kind::Prompt {
            from: "owner".into(),
            to: vec!["ada".into()],
            text: format!("{long}\nsecond line"),
        },
    });
    view.thread.push(&store::Event {
        ts: 2,
        agent: Some("ada".into()),
        kind: store::Kind::Text {
            text: "the agent answers here".into(),
        },
    });
    let text = screen(&mut view);
    let y = text.iter().position(|result| result.contains("alpha")).unwrap() as u16;
    let x = text[y as usize]
        .find("alpha")
        .map(|bytes| text[y as usize][..bytes].chars().count())
        .unwrap() as u16;
    let agent = text.iter().position(|result| result.contains("agent answers")).unwrap() as u16;

    assert!(agent > y + 2, "{text:#?}");

    mouse(&mut view, MouseEventKind::Down(MouseButton::Left), x, y);

    // past the message into the agent's: clamped to the owner's message
    mouse(&mut view, MouseEventKind::Drag(MouseButton::Left), 0, agent);
    mouse(&mut view, MouseEventKind::Up(MouseButton::Left), 0, agent);
    let got = crate::infra::clip::LAST.with_borrow(std::clone::Clone::clone);

    assert_eq!(got, format!("{long}\nsecond line"));

    // the highlight: 0 cells left of the bubble's text
    let text = screen(&mut view);
    let mut thread = Terminal::new(TestBackend::new(60, 30)).unwrap();

    thread.draw(|file| view.draw(file)).unwrap();
    let bytes = thread.backend().buffer().clone();

    let lit = |yy: u16| {
        (0..60)
            .filter(|&xx| bytes[(xx, yy)].bg == theme::pal().accent)
            .collect::<Vec<u16>>()
    };

    let row2 = lit(y + 1);

    assert!(!row2.is_empty() && row2[0] >= x, "row {}: {row2:?}\n{text:#?}", y + 1);
    assert!(lit(agent).is_empty(), "agent row unlit");
}

/// The chat beside the changes view (wide split): the same selection + copy.
#[test]
fn the_chat_of_the_split_selects_and_copies() {
    let (tx, _rx) = crate::infra::bus::channel();
    let mut view = View::new("selecttest-0001", Path::new("/tmp"), "t", tx);

    view.agents.members = vec!["ada".into()];
    view.thread.push(&store::Event {
        ts: 2,
        agent: Some("ada".into()),
        kind: store::Kind::Text {
            text: "copy me from the split".into(),
        },
    });
    view.set_pane(Pane::Changes);

    let draw = |view: &mut View| {
        let mut thread = Terminal::new(TestBackend::new(200, 40)).unwrap();

        thread.draw(|file| view.draw(file)).unwrap();
        let bytes = thread.backend().buffer().clone();

        (0..40)
            .map(|y| (0..200).map(|x| bytes[(x, y)].symbol().to_string()).collect::<String>())
            .collect::<Vec<String>>()
    };

    let text = draw(&mut view);

    assert!(
        view.panes.pane == Pane::Changes && view.layout.stream_rect.width > 0,
        "{text:#?}"
    );

    let y = text
        .iter()
        .position(|result| result.contains("copy me"))
        .expect("chat in the split") as u16;
    let x = text[y as usize]
        .find("copy me")
        .map(|bytes| text[y as usize][..bytes].chars().count())
        .unwrap() as u16;

    // 3 clicks: the row
    for _ in 0..3 {
        mouse(&mut view, MouseEventKind::Down(MouseButton::Left), x + 1, y);
        mouse(&mut view, MouseEventKind::Up(MouseButton::Left), x + 1, y);
    }

    let got = crate::infra::clip::LAST.with_borrow(std::clone::Clone::clone);

    assert_eq!(got, "copy me from the split");
}
