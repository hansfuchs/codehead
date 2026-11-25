use super::*;

#[test]
fn folds_runs_and_asks() {
    let mut thread = Thread::default();

    thread.push(&ev(
        None,
        Kind::Prompt {
            from: "owner".into(),
            to: vec!["ada".into()],
            text: "fix it\n\nAttachments (read these files):\n- /tmp/a.png".into(),
        },
    ));
    thread.push(&ev(Some("ada"), Kind::TurnStart));
    thread.push(&ev(
        Some("ada"),
        Kind::Tool {
            id: "1".into(),
            name: "Bash".into(),
            input: json!({"command": "ls", "description": "list"}),
        },
    ));
    thread.push(&ev(
        Some("ada"),
        Kind::ToolResult {
            id: "1".into(),
            ok: true,
            output: "a\nb".into(),
        },
    ));
    thread.push(&ev(
        Some("ada"),
        Kind::Tool {
            id: "2".into(),
            name: "Read".into(),
            input: json!({"file_path": "x.rs"}),
        },
    ));
    folds_runs_and_asks_verify_ask(thread);
}

fn folds_runs_and_asks_verify_ask(mut thread: Thread) {
    thread.push(&ev(
        Some("ada"),
        Kind::Text {
            text: "done **now**".into(),
        },
    ));
    thread.push(&ev(Some("ada"), Kind::Ask { id: "q".into(), tool: "AskUserQuestion".into(), input: json!({"questions": [{"question": "color?", "header": "Color", "options": [{"label": "red", "description": ""}, {"label": "blue", "description": ""}], "multiSelect": false}]}) }));

    assert_eq!(thread.items.len(), 3);
    assert!(thread.busy.contains("ada"));

    let mut ask = Picker::default();

    ask.sync(thread.asks.first());
    ask.at = 1;
    let drawn = draw(
        &thread,
        Look::Lines,
        60,
        &Colors::new(),
        &[],
        &Fold {
            runs: &HashSet::new(),
            all_runs: false,
            thoughts: Thoughts::Hidden,
            open_thoughts: &HashSet::new(),
            all_thoughts: false,
        },
        &ask,
    );
    let dump = text(&drawn);

    assert!(dump.contains("  fix it"), "{dump}");
    assert!(dump.contains("📎 /tmp/a.png"), "{dump}");
    assert!(dump.contains("Bash: list · Read: x.rs · 2 calls"), "{dump}");
    assert!(dump.contains("│ done now"), "{dump}");
    folds_runs_and_asks_verify_open(thread, &drawn, &dump);
}

fn folds_runs_and_asks_verify_open(mut thread: Thread, drawn: &Drawn, dump: &str) {
    assert!(dump.contains("┃ › 2. blue") && dump.contains("┃   1. red"), "{dump}");
    assert!(drawn.hits.values().any(|hit| *hit == Hit::Run(1)));
    assert!(widest(drawn) <= 60);

    let open: HashSet<usize> = [1].into();
    let open_dump = text(&draw(
        &thread,
        Look::Lines,
        60,
        &Colors::new(),
        &[],
        &Fold {
            runs: &open,
            all_runs: false,
            thoughts: Thoughts::Hidden,
            open_thoughts: &HashSet::new(),
            all_thoughts: false,
        },
        &Picker::default(),
    ));

    assert!(
        open_dump.contains("│      sh") && open_dump.contains("│      ls") && open_dump.contains("│      b"),
        "{open_dump}"
    );

    thread.push(&ev(
        Some("ada"),
        Kind::Answer {
            id: "q".into(),
            allow: true,
            label: "blue".into(),
        },
    ));
    folds_runs_and_asks_verify_state(thread);
}

fn folds_runs_and_asks_verify_state(mut thread: Thread) {
    thread.push(&ev(
        Some("ada"),
        Kind::TurnEnd {
            cost: 0.1,
            input: 1,
            output: 1,
            cache_read: 0,
            cache_write: 0,
            ctx: 0,
            window: 0,
            ms: 1,
            error: None,
        },
    ));

    assert!(thread.asks.is_empty() && thread.busy.is_empty());
    assert!(
        text(&draw(
            &thread,
            Look::Lines,
            60,
            &Colors::new(),
            &[],
            &Fold {
                runs: &HashSet::new(),
                all_runs: false,
                thoughts: Thoughts::Hidden,
                open_thoughts: &HashSet::new(),
                all_thoughts: false
            },
            &Picker::default()
        ))
        .contains("answered: blue")
    );
    assert_eq!(thread.sent().len(), 1);
}

#[test]
fn open_runs_show_code_blocks() {
    let mut thread = Thread::default();

    thread.push(&ev(Some("ada"), Kind::TurnStart));
    let cmd =
        "bun -e 'const d = await Bun.file(\"/very/long/path/to/some/report/data/file.json\").json(); console.log(d)'";

    thread.push(&ev(
        Some("ada"),
        Kind::Tool {
            id: "1".into(),
            name: "Bash".into(),
            input: json!({"command": cmd, "description": "Show data"}),
        },
    ));
    thread.push(&ev(
        Some("ada"),
        Kind::ToolResult {
            id: "1".into(),
            ok: true,
            output: "{\"a\": 1}".into(),
        },
    ));
    thread.push(&ev(
        Some("ada"),
        Kind::Tool {
            id: "2".into(),
            name: "Edit".into(),
            input: json!({"file_path": "src/x.rs", "old_string": "let a = 1;", "new_string": "let a = 2;"}),
        },
    ));
    thread.push(&ev(
        Some("ada"),
        Kind::ToolResult {
            id: "2".into(),
            ok: false,
            output: "no match".into(),
        },
    ));
    let open: HashSet<usize> = [1].into();

    assert_code_blocks(&thread, cmd, &open);
}

fn assert_code_blocks(thread: &Thread, cmd: &str, open: &HashSet<usize>) {
    for look in [Look::Bubbles, Look::Lines] {
        let drawn = draw(
            thread,
            look,
            60,
            &Colors::new(),
            &[],
            &Fold {
                runs: open,
                all_runs: false,
                thoughts: Thoughts::Hidden,
                open_thoughts: &HashSet::new(),
                all_thoughts: false,
            },
            &Picker::default(),
        );
        let dump = text(&drawn);

        if std::env::var_os("SHOW").is_some() {
            eprintln!("{dump}");
        }

        // the command: a sh block, wrapped (0 chars cut), with a copy button
        assert!(
            dump.contains(" sh ")
                && dump.contains("copy")
                && dump.contains("Bun.file")
                && dump.contains("data/file.json"),
            "{dump}"
        );
        assert!(
            dump.contains("-let a = 1;") && dump.contains("+let a = 2;") && dump.contains(" diff "),
            "{dump}"
        );
        assert!(dump.contains(" json ") && dump.contains("no match"), "{dump}");
        assert!(widest(&drawn) <= 60, "{dump}");

        // the copy button holds the whole command
        assert!(drawn.rows.iter().any(|row| row.copies.iter().any(|copy| copy.2 == cmd)));
    }
}
