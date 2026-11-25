use super::*;
use ratatui::crossterm::event::KeyModifiers;

fn key(command: &mut Command, ta: &mut TextArea, code: KeyCode) -> bool {
    command.key(KeyEvent::new(code, KeyModifiers::NONE), ta)
}

fn item(label: &str, time: u64) -> Item {
    Item {
        label: label.into(),
        insert: label.into(),
        time,
        ..Default::default()
    }
}

fn screen(command: &mut Command, ta: &mut TextArea, width: u16, height: u16, input: Rect) -> String {
    let backend = ratatui::backend::TestBackend::new(width, height);
    let mut term = ratatui::Terminal::new(backend).unwrap();

    term.draw(|frame| {
        ta.render(frame, input, true);
        command.render(frame, ta, frame.area(), input);
    })
    .unwrap();

    let buf = term.backend().buffer().clone();

    (0..height)
        .map(|y| (0..width).map(|x| buf[(x, y)].symbol().to_string()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn fuzzy_like_a_file_picker() {
    let items = vec![
        item("src/app.rs", 5),
        item("src/textarea.rs", 2),
        item("docs/agent-ascii-art.md", 3),
        item("src/theme.rs", 4),
    ];

    // 0 text: by time, the latest last
    assert_eq!(matches(&items, ""), [0, 1, 2, 3]);

    // chars in order, across the path: the best match last
    let order = matches(&items, "txa");

    assert_eq!(order.last().map(|&i| items[i].label.as_str()), Some("src/textarea.rs"));

    let order = matches(&items, "srcth");

    assert_eq!(
        order.iter().map(|&i| items[i].label.as_str()).collect::<Vec<_>>(),
        ["src/theme.rs"]
    );

    // a match in the file name beats a match in the dirs
    let items = vec![item("app/x.rs", 9), item("src/app.rs", 1)];

    assert_eq!(matches(&items, "app").last(), Some(&1));
    assert_eq!(matches(&items, "zzz"), [] as [usize; 0]);
}

#[test]
fn levels_open_filter_insert_and_go_back() {
    let mut ta = TextArea::new(false, "");
    let mut command = Command::default();

    ta.set_text("see >");
    ta.move_end();

    assert_eq!(super::open_level(&ta, &mut command), Some(0));

    // typed text filters the 1st level; enter opens the kinds, the text after `>` goes
    ta.insert("me");

    assert!(key(&mut command, &mut ta, KeyCode::Enter));
    assert_eq!(
        (super::open_level(&ta, &mut command), ta.text().as_str()),
        (Some(1), "see >")
    );

    // esc: 1 level back
    assert!(key(&mut command, &mut ta, KeyCode::Esc));
    assert_eq!(super::open_level(&ta, &mut command), Some(0));
    assert!(key(&mut command, &mut ta, KeyCode::Enter));
    assert_eq!(super::menu(Level::Kinds, "thr", &command), [2]);

    // the items come from disk, replaced here
    super::set_test_items(
        Kind::Files,
        vec![item("src/app.rs", 1), item("src/view.rs", 2), item("README.md", 3)],
        &mut command,
    );

    // the latest (last) row is current
    command.settle("");

    assert_eq!(command.cursor[2], 2);
    assert!(key(&mut command, &mut ta, KeyCode::Up));
    assert_eq!(command.cursor[2], 1);

    // typed text: fuzzy, the best match current
    for ch in "app".chars() {
        assert!(!key(&mut command, &mut ta, KeyCode::Char(ch)));

        ta.insert(&ch.to_string());
    }

    assert!(key(&mut command, &mut ta, KeyCode::Enter));
    assert_eq!(ta.text(), "see src/app.rs ");
    assert_eq!(super::open_level(&ta, &mut command), None);

    // esc on the 1st level: closed for this `>` only
    assert_first_level_escapes(ta, command);
}

fn assert_first_level_escapes(mut ta: TextArea, mut command: Command) {
    ta.set_text(">");
    ta.move_end();

    assert!(key(&mut command, &mut ta, KeyCode::Esc));
    assert_eq!(super::open_level(&ta, &mut command), None);
    assert!(!key(&mut command, &mut ta, KeyCode::Enter));

    ta.insert(" >");

    assert_eq!(super::open_level(&ta, &mut command), Some(0));

    // `> ` (a quote), `->`, 0 matching commands: 0 dropdown, keys pass
    ta.set_text("> ");
    ta.move_end();

    assert_eq!(super::open_level(&ta, &mut command), None);

    ta.set_text("a->");
    ta.move_end();

    assert_eq!(super::open_level(&ta, &mut command), None);

    ta.set_text(">zz");
    ta.move_end();

    assert!(!key(&mut command, &mut ta, KeyCode::Enter));
}

#[test]
fn backspace_on_an_empty_filter_goes_back() {
    let mut ta = TextArea::new(false, "");
    let mut command = Command::default();

    ta.set_text(">");
    ta.move_end();

    assert!(key(&mut command, &mut ta, KeyCode::Enter));
    assert_eq!(super::open_level(&ta, &mut command), Some(1));
    assert!(key(&mut command, &mut ta, KeyCode::Backspace));
    assert_eq!(
        (super::open_level(&ta, &mut command), ta.text().as_str()),
        (Some(0), ">")
    );

    // 1st level: backspace is the input's (removes the `>`)
    assert!(!key(&mut command, &mut ta, KeyCode::Backspace));
}

#[test]
fn branch_only_with_a_thread_and_skills_insert_their_name() {
    let mut ta = TextArea::new(false, "");
    let mut command = Command::default();

    // 0 thread (task screen): 0 branch
    assert_eq!(super::menu(Level::Commands, "", &command), [MENTION, SKILLS]);

    command.can_branch = true;

    assert_eq!(
        super::menu(Level::Commands, "", &command),
        [MENTION, BRANCH, SKILLS, HANDOFF, RELOAD, FLOW, SUB]
    );

    // `>su` enter: `>sub ` stays, 0 action (the send starts the sub-thread)
    ta.set_text(">su");
    ta.move_end();

    assert!(key(&mut command, &mut ta, KeyCode::Enter));
    assert_eq!((ta.text().as_str(), command.action.take()), (">sub ", None));

    // `>rel` enter: the reload action
    ta.set_text(">rel");
    ta.move_end();

    assert!(key(&mut command, &mut ta, KeyCode::Enter));
    assert_eq!((ta.text().as_str(), command.action.take()), ("", Some(Action::Reload)));

    ta.set_text(">flo");
    ta.move_end();

    assert!(key(&mut command, &mut ta, KeyCode::Enter));
    assert_eq!((ta.text().as_str(), command.action.take()), ("", Some(Action::Flow)));

    // `>hand` enter: the handoff action
    ta.set_text(">hand");
    ta.move_end();

    assert!(key(&mut command, &mut ta, KeyCode::Enter));
    assert_eq!((ta.text().as_str(), command.action.take()), ("", Some(Action::Handoff)));

    // `>br` enter: the text goes, the host gets the action
    ta.set_text("x >br");
    ta.move_end();

    assert!(key(&mut command, &mut ta, KeyCode::Enter));
    assert_eq!(
        (ta.text().as_str(), command.action.take()),
        ("x ", Some(Action::Branch))
    );

    // skills: 1 level below the commands, top-down, the 1st row current
    assert_skill_insertion(ta, command);
}

fn assert_skill_insertion(mut ta: TextArea, mut command: Command) {
    ta.set_text(">");
    ta.move_end();

    assert_eq!(super::open_level(&ta, &mut command), Some(0));

    let skill = |name: &str, description: &str, i: u64| Item {
        label: name.into(),
        detail: description.into(),
        insert: format!("/{name}"),
        time: i,
        info: description.into(),
    };

    super::set_test_items(
        Kind::Skills,
        vec![
            skill("antislop", "Detect and fix AI writing patterns in text.", 0),
            skill("tmux", "Use the tmux CLI to manage sessions.", 1),
            skill("xlsx", "Spreadsheet files.", 2),
        ],
        &mut command,
    );

    assert_eq!((command.settle(""), command.cursor[1]), (3, 0));

    let area = screen(&mut command, &mut ta, 100, 24, Rect::new(0, 18, 100, 4));

    // name + description in the row, the full description of the current row in a box above
    assert!(
        area.contains("┌ skills ") && area.contains("tmux  Use the tmux CLI to manage sessions."),
        "{area}"
    );
    assert!(
        area.contains("┌ antislop ") && area.contains("│ Detect and fix AI writing patterns in text."),
        "{area}"
    );

    // typed text: the name first, a match in the description too; the best on top
    ta.insert("spread");

    assert_eq!(command.settle("spread"), 1);
    assert!(key(&mut command, &mut ta, KeyCode::Enter));
    assert_eq!(ta.text(), "/xlsx ");
}

#[test]
fn files_notes_artifacts_from_disk() {
    let dir = std::env::temp_dir().join(format!("codehead-cmd-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);

    std::fs::create_dir_all(dir.join("sub")).unwrap();
    std::fs::create_dir_all(dir.join("node_modules")).unwrap();
    std::fs::write(dir.join("sub/a.md"), "a").unwrap();
    std::fs::write(dir.join("b.txt"), "b").unwrap();
    std::fs::write(dir.join("node_modules/c.js"), "c").unwrap();

    // 0 git: the files below the dir, relative
    let mut paths: Vec<String> = files(&dir).into_iter().map(|item| item.insert).collect();

    paths.sort();

    assert_eq!(paths, ["b.txt", "sub/a.md"]);

    // notes: .md files, shown relative, inserted as the full path
    let notes = notes(&dir);

    assert_eq!(notes.len(), 1);
    assert_eq!(
        (notes[0].label.as_str(), notes[0].insert.as_str()),
        ("sub/a.md", dir.join("sub/a.md").display().to_string().as_str())
    );

    // artifacts: urls in transcripts, 1 row per url, 0 share key
    std::fs::create_dir_all(dir.join("-Users-me-app")).unwrap();
    std::fs::write(
        dir.join("-Users-me-app/s.jsonl"),
        "{\"t\":\"see https://claude.ai/artifact/DsbWsZWkLy5uQJbKuyXwue?sk=abc and https://claude.ai/code/artifact/683e1f81-0b14-4581-9bd1-d1f9a90b6eb5\"}\n{\"t\":\"https://claude.ai/artifact/DsbWsZWkLy5uQJbKuyXwue\"}",
    )
    .unwrap();
    let mut urls: Vec<String> = scan_artifacts(&dir).into_iter().map(|item| item.insert).collect();

    urls.sort();

    assert_eq!(
        urls,
        [
            "https://claude.ai/artifact/DsbWsZWkLy5uQJbKuyXwue",
            "https://claude.ai/code/artifact/683e1f81-0b14-4581-9bd1-d1f9a90b6eb5"
        ]
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn artifact_labels_use_titles_and_insert_urls() {
    let first = "https://claude.ai/artifact/first12345";
    let second = "https://claude.ai/code/artifact/second12345";
    let third = "https://claude.ai/artifact/third12345";
    let mut titles = HashMap::new();
    let nested = serde_json::json!({"message": {"content": [{"text": serde_json::json!({
        "description": format!("[Search field design]({first}?sk=secret) [https://claude.ai/artifact/third12345]({third})")
    }).to_string()}, {"title": "Release dashboard", "url": second}]}});

    transcript_titles(&nested, &mut titles, 0);

    assert_eq!(titles.get(first).map(String::as_str), Some("Search field design"));
    assert_eq!(titles.get(second).map(String::as_str), Some("Release dashboard"));
    assert!(!titles.contains_key(third));

    let mut command = Command::default();
    let mut ta = TextArea::new(false, "");

    ta.set_text(">");
    ta.move_end();
    key(&mut command, &mut ta, KeyCode::Enter);
    super::set_test_items(
        Kind::Artifacts,
        vec![Item {
            label: titles[first].clone(),
            insert: first.into(),
            ..Default::default()
        }],
        &mut command,
    );
    let screen = screen(&mut command, &mut ta, 100, 24, Rect::new(0, 18, 100, 4));

    assert!(screen.contains("Search field design"), "{screen}");
    assert!(!screen.contains("claude.ai/"), "{screen}");
    assert!(key(&mut command, &mut ta, KeyCode::Enter));
    assert_eq!(ta.text(), format!("{first} "));
}

#[test]
fn artifact_page_titles_skip_generic_or_blocked_pages() {
    assert_eq!(
        page_title("<title>Search &amp; filters - Claude</title>"),
        Some("Search & filters".into())
    );
    assert_eq!(page_title("<title>Claude Artifact</title>"), None);
    assert_eq!(page_title("<title>Just a moment...</title>"), None);
}

#[test]
fn dropdowns_stay_above_the_prompt_box() {
    let mut ta = TextArea::new(false, "");
    let mut command = Command::default();

    // a prompt of 3 rows, the `>` on its last row
    ta.set_text("line 1\nline 2\nx >");
    ta.move_end();
    key(&mut command, &mut ta, KeyCode::Enter);
    super::set_test_items(
        Kind::Files,
        (0..30).map(|i| item(&format!("src/file-{i:02}.rs"), i)).collect(),
        &mut command,
    );
    let input = Rect::new(0, 20, 100, 5);
    let all = screen(&mut command, &mut ta, 100, 30, input);

    for label in ["┌ commands ", "┌ mention ", "┌ mention › files "] {
        assert!(all.contains(label), "{label}:\n{all}");
    }

    // 30 files, 10 in view: the latest at the bottom, ▲ (rows above), 0 ▼
    assert!(
        all.contains("file-29") && all.contains("file-20") && !all.contains("file-19"),
        "{all}"
    );
    assert!(all.contains('▲') && !all.contains('▼'), "{all}");

    // 3 dropdowns, left to right, all above the prompt box (its 3 rows stay in view)
    assert_eq!(command.areas.len(), 3);
    assert!(
        command.areas[0].right() <= command.areas[1].x && command.areas[1].right() <= command.areas[2].x,
        "{:?}",
        command.areas
    );
    assert!(
        command.areas.iter().all(|area| area.bottom() <= input.y),
        "{:?}",
        command.areas
    );
    assert!(
        all.contains("line 1") && all.contains("line 2") && all.contains("x >"),
        "{all}"
    );

    // narrow screen (0 room beside): the files go above the 2 menus, not over them or the prompt
    assert_narrow_dropdowns(ta, command);
}

fn assert_narrow_dropdowns(mut ta: TextArea, mut command: Command) {
    let input = Rect::new(0, 20, 60, 5);
    let all = screen(&mut command, &mut ta, 60, 30, input);

    let [commands, kinds, files] = command.areas[..] else {
        panic!("{:?}", command.areas)
    };

    assert!(
        files.bottom() <= commands.y.min(kinds.y) && files.right() <= 60,
        "{:?}",
        command.areas
    );
    assert!(
        !files.intersects(commands) && !files.intersects(kinds) && kinds.bottom() <= input.y,
        "{:?}",
        command.areas
    );
    assert!(
        all.contains("┌ commands ") && all.contains("┌ mention ") && all.contains("x >"),
        "{all}"
    );

    // 0 room above the prompt box: below it
    let input = Rect::new(0, 1, 100, 3);

    ta.set_text(">");
    ta.move_end();
    screen(&mut command, &mut ta, 100, 30, input);

    assert!(command.areas[0].y >= input.bottom(), "{:?}", command.areas);
}
