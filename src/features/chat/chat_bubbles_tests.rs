use super::*;

#[test]
fn bubbles_box_turns_and_put_owner_messages_right() {
    let file = attached_file("bubble", "a.png");
    let mut thread = Thread::default();

    thread.push(&ev(
        None,
        Kind::Prompt {
            from: "owner".into(),
            to: vec!["ada".into()],
            text: format!("fix it\n\nAttachments (read these files):\n- {}", file.display()),
        },
    ));
    thread.push(&ev(Some("ada"), Kind::TurnStart));
    thread.push(&ev(
        Some("ada"),
        Kind::Tool {
            id: "1".into(),
            name: "Bash".into(),
            input: json!({"command": "ls", "description": "List files"}),
        },
    ));
    thread.push(&ev(
        Some("ada"),
        Kind::ToolResult {
            id: "1".into(),
            ok: true,
            output: "a".into(),
        },
    ));
    assert_owner_bubble(thread);
}

fn assert_owner_bubble(mut thread: Thread) {
    thread.push(&ev(
        Some("ada"),
        Kind::Text {
            text: "※ SUMMARY\nDONE. it works.".into(),
        },
    ));
    let drawn = draw(
        &thread,
        Look::Bubbles,
        50,
        &Colors::new(),
        &[],
        &Fold {
            runs: &HashSet::new(),
            all_runs: false,
            thoughts: Thoughts::Hidden,
            open_thoughts: &HashSet::new(),
            all_thoughts: false,
        },
        &Picker::default(),
    );
    let dump = text(&drawn);
    let rows: Vec<&str> = dump.lines().collect();

    // the owner's bubble: right, the time in its top border, the file name below it
    let top = rows
        .iter()
        .position(|row| row.trim_start().starts_with("┌") && row.contains("─┐"))
        .unwrap();

    assert!(
        rows[top].starts_with(' ') && rows[top].ends_with("─┐") && rows[top].contains(&clock(1_000_000)),
        "{dump}"
    );
    assert!(
        rows.iter()
            .any(|row| row.ends_with("│ fix it   │") || (row.contains("│ fix it") && row.ends_with('│'))),
        "{dump}"
    );
    assert_agent_box(thread, &drawn, &dump, &rows);
}

fn assert_agent_box(mut thread: Thread, drawn: &Drawn, dump: &str, rows: &[&str]) {
    assert!(rows.iter().any(|row| row.ends_with("📎 a.png")), "{dump}");

    // the agent's turn: 1 box, its time in the top border, its name in the bottom border (in
    // view at the end of a box taller than the view), the run on 2 rows, the text inside
    let bottom = rows.iter().position(|row| row.starts_with("└─ ada")).unwrap();

    assert_eq!(rows[bottom].chars().count(), 50, "{dump}");
    assert_eq!(bottom, rows.len() - 1, "{dump}");

    let top = rows.iter().rposition(|row| row.starts_with('┌')).unwrap();

    assert!(
        !rows[top].contains("ada") && rows[top].contains(&clock(1_000_000)) && rows[top].chars().count() == 50,
        "{dump}"
    );
    assert!(
        rows.iter()
            .any(|row| row.starts_with("│ ▸ List files") && row.ends_with('│')),
        "{dump}"
    );
    assert!(rows.iter().any(|row| row.starts_with("│   1 call · 0.0s")), "{dump}");
    assert!(
        rows.iter().any(|row| row.starts_with("│ ※ SUMMARY"))
            && rows.iter().any(|row| row.starts_with("│ DONE. it works.")),
        "{dump}"
    );
    assert!(
        rows.last().unwrap().starts_with("└") && rows.last().unwrap().ends_with('┘'),
        "{dump}"
    );
    assert!(widest(drawn) <= 50);

    // at work: 1 wave in its bottom border, after the name
    assert_eq!(drawn.waves.len(), 1);
    assert_eq!(drawn.waves[0].1, 7, "{dump}");
    assert_eq!(drawn.waves[0].0, bottom);
    assert!(drawn.hits.values().any(|hit| *hit == Hit::Run(1)));

    // an answered question: the agent's question in its box, the answer as the owner's bubble
    thread.push(&ev(Some("ada"), Kind::Ask { id: "q".into(), tool: "AskUserQuestion".into(), input: json!({"questions": [{"question": "color?", "header": "Color", "options": [{"label": "red", "description": ""}], "multiSelect": false}]}) }));
    assert_answered_question(thread);
}

fn assert_answered_question(mut thread: Thread) {
    thread.push(&ev(
        Some("ada"),
        Kind::Answer {
            id: "q".into(),
            allow: true,
            label: "red".into(),
        },
    ));
    let dump = text(&draw(
        &thread,
        Look::Bubbles,
        50,
        &Colors::new(),
        &[],
        &Fold {
            runs: &HashSet::new(),
            all_runs: false,
            thoughts: Thoughts::Hidden,
            open_thoughts: &HashSet::new(),
            all_thoughts: false,
        },
        &Picker::default(),
    ));
    let rows: Vec<&str> = dump.lines().collect();
    let asked = rows
        .iter()
        .position(|row| row.starts_with("└─ ada asked"))
        .expect(&dump);

    assert!(
        rows[asked - 2].starts_with("│ color?") && rows[asked - 3].starts_with('┌'),
        "{dump}"
    );
    assert!(
        rows[asked + 1..]
            .iter()
            .any(|row| row.trim_end().ends_with("│ red    │")
                || (row.contains("│ red") && row.ends_with('│') && row.starts_with(' '))),
        "{dump}"
    );

    // declined: 0 answer bubble
    assert_declined_ask(thread);
}

fn assert_declined_ask(mut thread: Thread) {
    thread.push(&ev(
        Some("ada"),
        Kind::Ask {
            id: "p".into(),
            tool: "Bash".into(),
            input: json!({"command": "rm x"}),
        },
    ));
    thread.push(&ev(
        Some("ada"),
        Kind::Answer {
            id: "p".into(),
            allow: false,
            label: "deny".into(),
        },
    ));
    let dump = text(&draw(
        &thread,
        Look::Bubbles,
        50,
        &Colors::new(),
        &[],
        &Fold {
            runs: &HashSet::new(),
            all_runs: false,
            thoughts: Thoughts::Hidden,
            open_thoughts: &HashSet::new(),
            all_thoughts: false,
        },
        &Picker::default(),
    ));

    // ada still at work, 0 output since: 0 empty box below the declined ask
    let tail: Vec<&str> = dump.lines().rev().take(8).collect();

    assert!(
        tail.iter().any(|row| row.starts_with("│ denied")) && tail[0].starts_with('└'),
        "{dump}"
    );
    assert!(tail[0].starts_with("└─ ada asked to run"), "{dump}");
}
