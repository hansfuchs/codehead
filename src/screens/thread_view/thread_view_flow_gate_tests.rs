use super::*;
use ratatui::backend::TestBackend;

fn press(view: &mut View, codes: &[KeyCode]) {
    for &code in codes {
        view.key(KeyEvent::new(code, KeyModifiers::NONE));
    }
}

fn screen(view: &mut View) -> Vec<String> {
    let mut term = Terminal::new(TestBackend::new(90, 40)).unwrap();

    term.draw(|frame| view.draw(frame)).unwrap();
    let buf = term.backend().buffer().clone();

    (0..buf.area.height)
        .map(|y| {
            (0..buf.area.width)
                .map(|x| buf[(x, y)].symbol().to_string())
                .collect::<String>()
        })
        .collect()
}

#[test]
fn a_gate_never_blocks_the_owners_message() {
    let _g = crate::engine::ENV_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut thread = store::Thread::new("gate input", "task", Path::new("/tmp"), vec!["ada".into()]);
    let lib = crate::engine::flows::Library::load(None);
    let mut st = crate::engine::flows::run::FlowState::new(lib.flow("new-feature").unwrap(), &lib, 3, 0);

    st.wait = Some(crate::engine::flows::run::Wait::Gate);
    thread.flow = Some(st);
    std::fs::create_dir_all(store::thread_dir(&thread.id)).unwrap();
    thread.save().unwrap();
    let (tx, _rx) = crate::infra::bus::channel();
    let mut view = View::new(&thread.id, Path::new("/tmp"), "gate input", tx);

    view.agents.members = thread.agents.clone();
    view.sync_flowbars();
    view.sync_flow_ask();

    let sends = |view: &View| {
        view.ops
            .borrow()
            .iter()
            .filter(|recorded| recorded["op"] == "prompt")
            .count()
    };

    let answers = |view: &View| {
        view.ops
            .borrow()
            .iter()
            .filter(|recorded| recorded["op"] == "flow_answer")
            .count()
    };

    // typed text: the prompt's; the hint says how to send and how to answer
    let text = screen(&mut view).join("\n");

    assert!(
        text.contains("ctrl+enter: your message (the wait stays) · tab: the choices"),
        "{text}"
    );

    assert_owner_gate(&thread, view, sends, answers);
}

fn assert_owner_gate(
    thread: &store::Thread,
    mut view: View,
    sends: impl Fn(&View) -> usize,
    answers: impl Fn(&View) -> usize,
) {
    for character in "1 more".chars() {
        view.key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
    }

    assert_eq!(
        view.prompt.input.text(),
        "1 more",
        "digits and letters: the prompt's, 0 pick"
    );
    assert_eq!(answers(&view), 0);

    // ctrl+enter: a message, the gate stays
    view.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL));

    assert_eq!((sends(&view), answers(&view)), (1, 0));
    assert!(
        view.thread
            .asks
            .iter()
            .any(|ask| ask.tool == crate::engine::flows::run::WAIT_TOOL),
        "the gate stays"
    );

    // changes (2) with an empty prompt: 0 answer, the keys back to the prompt
    press(&mut view, &[KeyCode::Tab, KeyCode::Char('2')]);

    assert_eq!(answers(&view), 0, "{:?}", view.ops.borrow());
    assert!(!view.ask.focus);

    // reply (3): empty → 0 send; with text → a message, the gate stays
    press(&mut view, &[KeyCode::Tab, KeyCode::Char('3')]);

    assert_eq!(sends(&view), 1);

    view.prompt.input.set_text("also split the plan");
    press(&mut view, &[KeyCode::Tab, KeyCode::Char('3')]);

    assert_eq!((sends(&view), answers(&view)), (2, 0));
    assert!(
        view.thread
            .asks
            .iter()
            .any(|ask| ask.tool == crate::engine::flows::run::WAIT_TOOL)
    );

    let _ = store::delete(&thread.id);
}

#[test]
fn the_flow_row_shows_a_gate_and_its_box_answers_and_switches() {
    let _g = crate::engine::ENV_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut thread = store::Thread::new("flow view", "task", Path::new("/tmp"), vec!["ada".into()]);
    let lib = crate::engine::flows::Library::load(None);
    let mut st = crate::engine::flows::run::FlowState::new(lib.flow("new-feature").unwrap(), &lib, 3, 0);

    st.wait = Some(crate::engine::flows::run::Wait::Gate);
    thread.flow = Some(st);
    std::fs::create_dir_all(store::thread_dir(&thread.id)).unwrap();
    thread.save().unwrap();
    let (tx, _rx) = crate::infra::bus::channel();
    let mut view = View::new(&thread.id, Path::new("/tmp"), "flow view", tx);

    view.agents.members = thread.agents.clone();
    view.sync_flowbars();

    // closed: 0 flow row, the card's 4th row has the step; a click on it opens the box (the
    // row of before on its bottom), a 2nd click closes it
    let text = screen(&mut view).join("\n");

    assert!(
        text.contains("⏸ 4/9 plan approval") && !text.contains("◆ new feature · 4/9"),
        "{text}"
    );

    let card = text
        .lines()
        .position(|line| line.contains("⏸ 4/9 plan approval"))
        .unwrap();

    assert!(
        text.lines().nth(card - 3).is_some_and(|line| line.contains("ada")),
        "name 3 rows above the step: {text}"
    );

    let row = view.agents.flow_slots.borrow()[0].1;
    let click = MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: row.x + 3,
        row: row.y,
        modifiers: KeyModifiers::NONE,
    };

    view.mouse(click);
    let text = screen(&mut view).join("\n");

    assert!(
        text.contains("◆ new feature · 4/9 plan approval · @ada")
            && text.contains("⏸ gate plan approval: answer below"),
        "{text}"
    );

    view.mouse(click);
    let text = screen(&mut view).join("\n");

    assert!(!text.contains("◆ new feature · 4/9"), "{text}");

    // the gate: a question in the picker under the stream; it never takes the keys (the owner
    // may write first), tab gives them to it; the prompt's text goes along as the note
    view.sync_flow_ask();

    assert!(!view.ask.focus, "an empty prompt, 0 keys: the gate still waits for tab");

    assert_flow_gate_completion(&thread, view);
}

fn assert_flow_gate_completion(thread: &store::Thread, mut view: View) {
    view.prompt.input.set_text("split the plan");
    let text = screen(&mut view).join("\n");

    assert!(
        text.contains("waits on you")
            && text.contains("gate after “plan approval”")
            && text.contains("approve · go on")
            && text.contains("changes · back to plan")
            && text.contains("reply · the prompt's text"),
        "{text}"
    );

    press(&mut view, &[KeyCode::Tab]);

    assert!(view.ask.focus);

    press(&mut view, &[KeyCode::Down, KeyCode::Enter]);
    let op = view
        .ops
        .borrow()
        .iter()
        .find(|recorded| recorded["op"] == "flow_answer")
        .cloned()
        .unwrap();

    assert_eq!(
        (op["choice"].as_str(), op["note"].as_str()),
        (Some("changes"), Some("split the plan"))
    );
    assert_eq!(view.prompt.input.text(), "");
    assert!(view.thread.asks.is_empty(), "answered: the flow's ask goes");
    assert!(
        !view.ops.borrow().iter().any(|recorded| recorded["op"] == "answer"),
        "0 host ask answer"
    );

    // change the flow: the box asks now / after at once, then the start step
    view.open_flow("ada", false);
    view.stream.dirty = true;
    let text = screen(&mut view).join("\n");

    println!("{text}");

    assert_flow_gate(thread, view, &text);
}

fn assert_flow_gate(thread: &store::Thread, mut view: View, text: &String) {
    assert!(text.contains("every step runs on @ada") && text.contains("◀"), "{text}");

    press(
        &mut view,
        &[
            KeyCode::Char('c'),
            KeyCode::Char('r'),
            KeyCode::Char('e'),
            KeyCode::Char('f'),
            KeyCode::Enter,
        ],
    );
    let text = screen(&mut view).join("\n");

    println!("{text}");

    assert!(
        text.contains("switch now") && text.contains("after current flow"),
        "{text}"
    );

    press(&mut view, &[KeyCode::Enter, KeyCode::Down, KeyCode::Enter]);
    let op = view
        .ops
        .borrow()
        .iter()
        .find(|recorded| recorded["op"] == "flow_set")
        .cloned()
        .unwrap();

    assert_eq!(
        (op["flow"].as_str(), op["start"].as_u64(), op["mode"].as_str()),
        (Some("refactor"), Some(1), Some("now"))
    );

    let text = screen(&mut view).join("\n");

    assert!(
        text.contains("→ refactor at the next tool boundary") || text.contains("4/9 plan approval → refactor"),
        "{text}"
    );

    let _ = std::fs::remove_dir_all(store::thread_dir(&thread.id));
}
