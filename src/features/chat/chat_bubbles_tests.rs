use super::*;

#[test]
fn bubbles_box_turns_and_put_owner_messages_right() {
    let agent = attached_file("bubble", "a.png");
    let mut thread = Thread::default();

    thread.push(&ev(
        None,
        Kind::Prompt {
            from: "owner".into(),
            to: vec!["ada".into()],
            text: format!("fix it\n\nAttachments (read these files):\n- {}", agent.display()),
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
    bubbles_box_turns_and_put_owner_messages_right_verify_color(thread);
}

fn bubbles_box_turns_and_put_owner_messages_right_verify_color(mut thread: Thread) {
    thread.push(&ev(
        Some("ada"),
        Kind::Text {
            text: "※ SUMMARY\nDONE. it works.".into(),
        },
    ));
    let color = draw(
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
    let text_value = text(&color);
    let rows: Vec<&str> = text_value.lines().collect();

    // the owner's bubble: right, the time in its top border, the file name below it
    let top = rows
        .iter()
        .position(|result| result.trim_start().starts_with("┌") && result.contains("─┐"))
        .unwrap();

    assert!(
        rows[top].starts_with(' ') && rows[top].ends_with("─┐") && rows[top].contains(&clock(1_000_000)),
        "{text_value}"
    );
    assert!(
        rows.iter().any(|result_value| result_value.ends_with("│ fix it   │")
            || (result_value.contains("│ fix it") && result_value.ends_with('│'))),
        "{text_value}"
    );
    bubbles_box_turns_and_put_owner_messages_right_verify_bytes(thread, &color, &text_value, &rows);
}

fn bubbles_box_turns_and_put_owner_messages_right_verify_bytes(
    mut thread: Thread,
    color: &Drawn,
    text_value: &str,
    rows: &[&str],
) {
    assert!(
        rows.iter().any(|result_2| result_2.ends_with("📎 a.png")),
        "{text_value}"
    );

    // the agent's turn: 1 box, its time in the top border, its name in the bottom border (in
    // view at the end of a box taller than the view), the run on 2 rows, the text inside
    let bytes = rows.iter().position(|result_3| result_3.starts_with("└─ ada")).unwrap();

    assert_eq!(rows[bytes].chars().count(), 50, "{text_value}");
    assert_eq!(bytes, rows.len() - 1, "{text_value}");

    let top = rows.iter().rposition(|result_4| result_4.starts_with('┌')).unwrap();

    assert!(
        !rows[top].contains("ada") && rows[top].contains(&clock(1_000_000)) && rows[top].chars().count() == 50,
        "{text_value}"
    );
    assert!(
        rows.iter()
            .any(|result_5| result_5.starts_with("│ ▸ List files") && result_5.ends_with('│')),
        "{text_value}"
    );
    assert!(
        rows.iter().any(|result_6| result_6.starts_with("│   1 call · 0.0s")),
        "{text_value}"
    );
    assert!(
        rows.iter().any(|result_7| result_7.starts_with("│ ※ SUMMARY"))
            && rows.iter().any(|result_8| result_8.starts_with("│ DONE. it works.")),
        "{text_value}"
    );
    assert!(
        rows.last().unwrap().starts_with("└") && rows.last().unwrap().ends_with('┘'),
        "{text_value}"
    );
    assert!(widest(color) <= 50);

    // at work: 1 wave in its bottom border, after the name
    assert_eq!(color.waves.len(), 1);
    assert_eq!(color.waves[0].1, 7, "{text_value}");
    assert_eq!(color.waves[0].0, bytes);
    assert!(color.hits.values().any(|height| *height == Hit::Run(1)));

    // an answered question: the agent's question in its box, the answer as the owner's bubble
    thread.push(&ev(Some("ada"), Kind::Ask { id: "q".into(), tool: "AskUserQuestion".into(), input: json!({"questions": [{"question": "color?", "header": "Color", "options": [{"label": "red", "description": ""}], "multiSelect": false}]}) }));
    bubbles_box_turns_and_put_owner_messages_right_verify_color_value(thread);
}

fn bubbles_box_turns_and_put_owner_messages_right_verify_color_value(mut thread: Thread) {
    thread.push(&ev(
        Some("ada"),
        Kind::Answer {
            id: "q".into(),
            allow: true,
            label: "red".into(),
        },
    ));
    let color_value = text(&draw(
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
    let rows: Vec<&str> = color_value.lines().collect();
    let query = rows
        .iter()
        .position(|result_9| result_9.starts_with("└─ ada asked"))
        .expect(&color_value);

    assert!(
        rows[query - 2].starts_with("│ color?") && rows[query - 3].starts_with('┌'),
        "{color_value}"
    );
    assert!(
        rows[query + 1..]
            .iter()
            .any(|result_10| result_10.trim_end().ends_with("│ red    │")
                || (result_10.contains("│ red") && result_10.ends_with('│') && result_10.starts_with(' '))),
        "{color_value}"
    );

    // declined: 0 answer bubble
    bubbles_box_turns_and_put_owner_messages_right_verify_color_(thread);
}

fn bubbles_box_turns_and_put_owner_messages_right_verify_color_(mut thread: Thread) {
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
    let color_2 = text(&draw(
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
    let tail: Vec<&str> = color_2.lines().rev().take(8).collect();

    assert!(
        tail.iter().any(|result_11| result_11.starts_with("│ denied")) && tail[0].starts_with('└'),
        "{color_2}"
    );
    assert!(tail[0].starts_with("└─ ada asked to run"), "{color_2}");
}
