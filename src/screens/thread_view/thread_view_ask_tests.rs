use super::*;
use ratatui::backend::TestBackend;

fn view_with_ask(multi: bool) -> View {
    let (tx, _rx) = crate::infra::bus::channel();
    let mut view = View::new("asktest-0000", Path::new("/tmp"), "t", tx);
    let input = json!({"questions": [{"question": "Which keys?", "header": "Keys", "multiSelect": multi, "options": [
        {"label": "alpha", "description": "1st"}, {"label": "beta", "description": "2nd"}, {"label": "gamma", "description": "3rd"}]}]});

    view.thread.push(&store::Event {
        ts: 1,
        agent: Some("ada".into()),
        kind: store::Kind::Ask {
            id: "q1".into(),
            tool: "AskUserQuestion".into(),
            input,
        },
    });
    view.ask.sync(view.thread.asks.first());

    view
}

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

fn answers(view: &View) -> Vec<Value> {
    view.ops
        .borrow()
        .iter()
        .filter(|recorded| recorded["op"] == "answer")
        .cloned()
        .collect()
}

#[test]
fn text_in_the_prompt_does_not_block_the_ask() {
    let mut view = view_with_ask(true);

    view.prompt.input.set_text("a kept draft");
    press(
        &mut view,
        &[
            KeyCode::Down,
            KeyCode::Char(' '),
            KeyCode::Down,
            KeyCode::Char(' '),
            KeyCode::Enter,
        ],
    );
    let sent = answers(&view);

    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0]["answers"]["Which keys?"], "beta, gamma");

    // the draft: untouched
    assert_eq!(view.prompt.input.text(), "a kept draft");

    // sent: the next keys go to the prompt, 0 2nd answer
    press(&mut view, &[KeyCode::Enter, KeyCode::Char('!')]);

    assert_eq!(answers(&view).len(), 1);
    assert!(view.prompt.input.text().ends_with('!'));
}

#[test]
fn tab_gives_the_keys_to_the_prompt_and_back() {
    let mut view = view_with_ask(true);

    press(
        &mut view,
        &[KeyCode::Tab, KeyCode::Char('h'), KeyCode::Char('i'), KeyCode::Char(' ')],
    );

    assert_eq!(view.prompt.input.text(), "hi ");
    assert!(
        view.ask
            .rows(view.thread.asks.first().unwrap())
            .iter()
            .all(|row| !row.picked)
    );

    // tab: back to the ask (prompt text or not); space picks there
    press(&mut view, &[KeyCode::Tab, KeyCode::Char(' ')]);

    assert!(view.ask.rows(view.thread.asks.first().unwrap())[0].picked);
    assert_eq!(view.prompt.input.text(), "hi ");
    assert!(view.menu.is_none(), "tab with an open ask: 0 menu");
}

#[test]
fn the_ask_box_shows_boxes_cursor_other_and_hint() {
    let mut view = view_with_ask(true);

    view.prompt.input.set_text("draft");
    press(&mut view, &[KeyCode::Down, KeyCode::Char(' ')]);
    let text = screen(&mut view).join("\n");

    assert!(text.contains("[ ] 1. alpha"), "{text}");
    assert!(text.contains("› [x] 2. beta"), "{text}");
    assert!(text.contains("[ ] 4. Other: type an answer"), "{text}");
    assert!(
        text.contains("space: pick · enter: answer · type: other · tab: prompt"),
        "{text}"
    );

    // letters: into Other, the cursor there
    press(&mut view, &[KeyCode::Char('z'), KeyCode::Char('z')]);
    let text = screen(&mut view).join("\n");

    assert!(text.contains("› [x] 4. Other: zz▏"), "{text}");

    // focus on the prompt: 0 cursor row, the hint says how back
    press(&mut view, &[KeyCode::Tab]);
    let text = screen(&mut view).join("\n");

    assert!(
        !text.contains("› [x]") && text.contains("tab: answer the question"),
        "{text}"
    );
}

#[test]
fn single_pick_answers_with_space_enter_digits_or_a_click() {
    let mut view = view_with_ask(false);

    press(&mut view, &[KeyCode::Down, KeyCode::Down, KeyCode::Char(' ')]);

    assert_eq!(answers(&view)[0]["answers"]["Which keys?"], "gamma");

    let mut view = view_with_ask(false);

    press(&mut view, &[KeyCode::Char('2')]);

    assert_eq!(answers(&view)[0]["answers"]["Which keys?"], "beta");

    // a click on the row of "alpha"
    let mut view = view_with_ask(false);
    let rows = screen(&mut view);
    let y = rows.iter().position(|row| row.contains("1. alpha")).unwrap() as u16;

    view.mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 10,
        row: y,
        modifiers: KeyModifiers::NONE,
    });

    assert_eq!(answers(&view)[0]["answers"]["Which keys?"], "alpha");
}

#[test]
fn a_pending_send_colors_its_mentions() {
    let (tx, _rx) = crate::infra::bus::channel();
    let mut view = View::new("asktest-0002", Path::new("/tmp"), "t", tx);

    view.agents.names = vec![("ada".into(), Color::Rgb(200, 10, 10))];
    view.prompt.pending.push(("@ada go on".into(), Instant::now(), None));
    let mut term = Terminal::new(TestBackend::new(60, 20)).unwrap();

    term.draw(|frame| view.draw(frame)).unwrap();
    let buf = term.backend().buffer().clone();

    let (x, y) = (0..20u16)
        .find_map(|y| {
            (0..60u16)
                .find(|&x| buf[(x, y)].symbol() == "@" && buf[(x + 1, y)].symbol() == "a")
                .map(|x| (x, y))
        })
        .expect("0 @ada on screen");

    assert!((x..x + 4).all(|x| buf[(x, y)].fg == Color::Rgb(200, 10, 10)));
    assert_ne!(buf[(x + 5, y)].fg, Color::Rgb(200, 10, 10));
}

/// `>sub @agent task` + ctrl+enter: a sub-thread in the same checkout, 0 message to this
/// thread's agents, its row at once; `>sub` alone: a hint, the text stays.
#[test]
fn sub_starts_a_sub_thread_of_this_thread() {
    let mut thread = store::Thread::new("main", "task", Path::new("/tmp"), vec!["ada".into(), "bob".into()]);

    thread.worktree = Some("/tmp/wt".into());
    thread.branch = Some("task/x".into());
    std::fs::create_dir_all(store::thread_dir(&thread.id)).unwrap();
    thread.save().unwrap();
    let (tx, _rx) = crate::infra::bus::channel();
    let mut view = View::new(&thread.id, Path::new("/tmp"), "main", tx);

    view.agents.members = vec!["ada".into(), "bob".into()];

    view.prompt.input.set_text(">sub");
    view.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL));

    assert!(
        view.ops.borrow().is_empty() && view.prompt.input.text() == ">sub",
        "0 task: 0 sub-thread"
    );

    view.prompt.input.set_text(">sub @bob split the diff into commits");
    view.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL));
    let ops = view.ops.borrow().clone();

    assert_eq!(ops.len(), 1, "only the sub_start to this thread's host: {ops:?}");
    assert_eq!(ops[0]["op"], "sub_start");

    let kid = ops[0]["id"].as_str().unwrap().to_string();
    let sub = store::load(&kid).unwrap();

    assert_eq!(
        (
            sub.of.as_deref(),
            sub.agents.as_slice(),
            sub.worktree.as_deref(),
            sub.branch.as_deref()
        ),
        (
            Some(thread.id.as_str()),
            &["bob".to_string()][..],
            Some("/tmp/wt"),
            Some("task/x")
        )
    );
    assert!(view.prompt.input.is_empty());
    assert!(view.subthreads.active() && view.subthreads.kids[0].title.contains("split the diff"));

    // the thread lists leave it out
    assert!(!store::all().iter().any(|listed| listed.id == kid));

    // its view: esc esc goes back to this thread, not to the start screen
    assert!(matches!(to_main(Exit::Back, sub.of.as_deref()), Exit::Open { id, .. } if id == thread.id));
    assert!(matches!(to_main(Exit::Back, None), Exit::Back));
    assert!(matches!(to_main(Exit::Quit, sub.of.as_deref()), Exit::Quit));

    let _ = std::fs::remove_dir_all(store::thread_dir(&kid));
    let _ = std::fs::remove_dir_all(store::thread_dir(&thread.id));
}

#[test]
fn a_pasted_image_goes_out_with_its_marker_next_to_its_path() {
    let (tx, _rx) = crate::infra::bus::channel();
    let mut view = View::new("asktest-0002", Path::new("/tmp"), "t", tx);

    view.prompt.input.set_text("look at [Image #1]");
    view.prompt.attachments = vec![("[Image #1]".into(), PathBuf::from("/x/shot.png"))];

    view.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL));

    let text = view
        .ops
        .borrow()
        .iter()
        .find(|recorded| recorded["op"] == "prompt")
        .and_then(|recorded| recorded["text"].as_str().map(String::from))
        .unwrap();

    assert!(
        text.ends_with("Attachments (read these files):\n- [Image #1]: /x/shot.png"),
        "{text}"
    );
}

#[test]
fn ctrl_s_never_sends_ctrl_enter_does() {
    let (tx, _rx) = crate::infra::bus::channel();
    let mut view = View::new("asktest-0001", Path::new("/tmp"), "t", tx);

    view.prompt.input.set_text("hello agents");
    view.key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL));

    assert!(view.ops.borrow().is_empty());
    assert_eq!(view.prompt.input.text(), "hello agents");
    assert!(
        view.toast
            .as_ref()
            .is_some_and(|(toast, _)| toast.contains("ctrl+enter"))
    );

    view.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL));

    assert!(
        view.ops.borrow().iter().any(|recorded| recorded["op"] == "prompt"
            && recorded["text"]
                .as_str()
                .is_some_and(|text| text.contains("hello agents"))),
        "{:?}",
        view.ops.borrow()
    );
}

#[test]
fn esc_and_the_menu_key_stay_the_views() {
    let mut view = view_with_ask(false);

    press(&mut view, &[KeyCode::Esc]);

    assert!(view.last_esc.is_some());
    assert_eq!(answers(&view), [] as [serde_json::Value; 0]);

    // ctrl+c on an empty prompt: the quit arm, 0 ask key
    view.key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));

    assert!(view.last_ctrl_c.is_some());
}
