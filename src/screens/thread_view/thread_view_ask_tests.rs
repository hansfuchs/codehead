use super::*;
use ratatui::backend::TestBackend;

fn view(multi: bool) -> View {
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
    for &character in codes {
        view.key(KeyEvent::new(character, KeyModifiers::NONE));
    }
}

fn screen(view: &mut View) -> Vec<String> {
    let mut term = Terminal::new(TestBackend::new(90, 40)).unwrap();

    term.draw(|file| view.draw(file)).unwrap();
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
        .filter(|output| output["op"] == "answer")
        .cloned()
        .collect()
}

#[test]
fn text_in_the_prompt_does_not_block_the_ask() {
    let mut value = view(true);

    value.prompt.input.set_text("a kept draft");
    press(
        &mut value,
        &[
            KeyCode::Down,
            KeyCode::Char(' '),
            KeyCode::Down,
            KeyCode::Char(' '),
            KeyCode::Enter,
        ],
    );
    let agent = answers(&value);

    assert_eq!(agent.len(), 1);
    assert_eq!(agent[0]["answers"]["Which keys?"], "beta, gamma");

    // the draft: untouched
    assert_eq!(value.prompt.input.text(), "a kept draft");

    // sent: the next keys go to the prompt, 0 2nd answer
    press(&mut value, &[KeyCode::Enter, KeyCode::Char('!')]);

    assert_eq!(answers(&value).len(), 1);
    assert!(value.prompt.input.text().ends_with('!'));
}

#[test]
fn tab_gives_the_keys_to_the_prompt_and_back() {
    let mut value = view(true);

    press(
        &mut value,
        &[KeyCode::Tab, KeyCode::Char('h'), KeyCode::Char('i'), KeyCode::Char(' ')],
    );

    assert_eq!(value.prompt.input.text(), "hi ");
    assert!(
        value
            .ask
            .rows(value.thread.asks.first().unwrap())
            .iter()
            .all(|result| !result.picked)
    );

    // tab: back to the ask (prompt text or not); space picks there
    press(&mut value, &[KeyCode::Tab, KeyCode::Char(' ')]);

    assert!(value.ask.rows(value.thread.asks.first().unwrap())[0].picked);
    assert_eq!(value.prompt.input.text(), "hi ");
    assert!(value.menu.is_none(), "tab with an open ask: 0 menu");
}

#[test]
fn the_ask_box_shows_boxes_cursor_other_and_hint() {
    let mut value = view(true);

    value.prompt.input.set_text("draft");
    press(&mut value, &[KeyCode::Down, KeyCode::Char(' ')]);
    let text = screen(&mut value).join("\n");

    assert!(text.contains("[ ] 1. alpha"), "{text}");
    assert!(text.contains("› [x] 2. beta"), "{text}");
    assert!(text.contains("[ ] 4. Other: type an answer"), "{text}");
    assert!(
        text.contains("space: pick · enter: answer · type: other · tab: prompt"),
        "{text}"
    );

    // letters: into Other, the cursor there
    press(&mut value, &[KeyCode::Char('z'), KeyCode::Char('z')]);
    let text = screen(&mut value).join("\n");

    assert!(text.contains("› [x] 4. Other: zz▏"), "{text}");

    // focus on the prompt: 0 cursor row, the hint says how back
    press(&mut value, &[KeyCode::Tab]);
    let text = screen(&mut value).join("\n");

    assert!(
        !text.contains("› [x]") && text.contains("tab: answer the question"),
        "{text}"
    );
}

#[test]
fn single_pick_answers_with_space_enter_digits_or_a_click() {
    let mut value = view(false);

    press(&mut value, &[KeyCode::Down, KeyCode::Down, KeyCode::Char(' ')]);

    assert_eq!(answers(&value)[0]["answers"]["Which keys?"], "gamma");

    let mut value = view(false);

    press(&mut value, &[KeyCode::Char('2')]);

    assert_eq!(answers(&value)[0]["answers"]["Which keys?"], "beta");

    // a click on the row of "alpha"
    let mut value = view(false);
    let rows = screen(&mut value);
    let y = rows.iter().position(|result| result.contains("1. alpha")).unwrap() as u16;

    value.mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 10,
        row: y,
        modifiers: KeyModifiers::NONE,
    });

    assert_eq!(answers(&value)[0]["answers"]["Which keys?"], "alpha");
}

#[test]
fn a_pending_send_colors_its_mentions() {
    let (tx, _rx) = crate::infra::bus::channel();
    let mut view = View::new("asktest-0002", Path::new("/tmp"), "t", tx);

    view.agents.names = vec![("ada".into(), Color::Rgb(200, 10, 10))];
    view.prompt.pending.push(("@ada go on".into(), Instant::now(), None));
    let mut term = Terminal::new(TestBackend::new(60, 20)).unwrap();

    term.draw(|file| view.draw(file)).unwrap();
    let bytes = term.backend().buffer().clone();

    let (x, y) = (0..20u16)
        .find_map(|y| {
            (0..60u16)
                .find(|&x| bytes[(x, y)].symbol() == "@" && bytes[(x + 1, y)].symbol() == "a")
                .map(|x| (x, y))
        })
        .expect("0 @ada on screen");

    assert!((x..x + 4).all(|x| bytes[(x, y)].fg == Color::Rgb(200, 10, 10)));
    assert_ne!(bytes[(x + 5, y)].fg, Color::Rgb(200, 10, 10));
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
    let key = store::load(&kid).unwrap();

    assert_eq!(
        (
            key.of.as_deref(),
            key.agents.as_slice(),
            key.worktree.as_deref(),
            key.branch.as_deref()
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
    assert!(!store::all().iter().any(|x| x.id == kid));

    // its view: esc esc goes back to this thread, not to the start screen
    assert!(matches!(to_main(Exit::Back, key.of.as_deref()), Exit::Open { id, .. } if id == thread.id));
    assert!(matches!(to_main(Exit::Back, None), Exit::Back));
    assert!(matches!(to_main(Exit::Quit, key.of.as_deref()), Exit::Quit));

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
        .find(|output| output["op"] == "prompt")
        .and_then(|output| output["text"].as_str().map(String::from))
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
            .is_some_and(|(message, _)| message.contains("ctrl+enter"))
    );

    view.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL));

    assert!(
        view.ops.borrow().iter().any(|output| output["op"] == "prompt"
            && output["text"]
                .as_str()
                .is_some_and(|thread| thread.contains("hello agents"))),
        "{:?}",
        view.ops.borrow()
    );
}

#[test]
fn esc_and_the_menu_key_stay_the_views() {
    let mut value = view(false);

    press(&mut value, &[KeyCode::Esc]);

    assert!(value.last_esc.is_some());
    assert_eq!(answers(&value), [] as [serde_json::Value; 0]);

    // ctrl+c on an empty prompt: the quit arm, 0 ask key
    value.key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));

    assert!(value.last_ctrl_c.is_some());
}
