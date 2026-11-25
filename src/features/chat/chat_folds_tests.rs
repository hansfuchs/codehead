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
    let color = draw(
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
    let text_value = text(&color);

    assert!(text_value.contains("  fix it"), "{text_value}");
    assert!(text_value.contains("📎 /tmp/a.png"), "{text_value}");
    assert!(text_value.contains("Bash: list · Read: x.rs · 2 calls"), "{text_value}");
    assert!(text_value.contains("│ done now"), "{text_value}");
    folds_runs_and_asks_verify_open(thread, &color, &text_value);
}

fn folds_runs_and_asks_verify_open(mut thread: Thread, color: &Drawn, text_value: &str) {
    assert!(
        text_value.contains("┃ › 2. blue") && text_value.contains("┃   1. red"),
        "{text_value}"
    );
    assert!(color.hits.values().any(|height| *height == Hit::Run(1)));
    assert!(widest(color) <= 60);

    let open: HashSet<usize> = [1].into();
    let color_value = text(&draw(
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
        color_value.contains("│      sh") && color_value.contains("│      ls") && color_value.contains("│      b"),
        "{color_value}"
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

    open_runs_show_code_blocks_verify_color(&thread, cmd, &open);
}

fn open_runs_show_code_blocks_verify_color(thread: &Thread, cmd: &str, open: &HashSet<usize>) {
    for look in [Look::Bubbles, Look::Lines] {
        let color = draw(
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
        let text_value = text(&color);

        if std::env::var_os("SHOW").is_some() {
            eprintln!("{text_value}");
        }

        // the command: a sh block, wrapped (0 chars cut), with a copy button
        assert!(
            text_value.contains(" sh ")
                && text_value.contains("copy")
                && text_value.contains("Bun.file")
                && text_value.contains("data/file.json"),
            "{text_value}"
        );
        assert!(
            text_value.contains("-let a = 1;") && text_value.contains("+let a = 2;") && text_value.contains(" diff "),
            "{text_value}"
        );
        assert!(
            text_value.contains(" json ") && text_value.contains("no match"),
            "{text_value}"
        );
        assert!(widest(&color) <= 60, "{text_value}");

        // the copy button holds the whole command
        assert!(
            color
                .rows
                .iter()
                .any(|result| result.copies.iter().any(|character| character.2 == cmd))
        );
    }
}
