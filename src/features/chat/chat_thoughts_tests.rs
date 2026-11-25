use super::*;

#[test]
fn thoughts_fold_into_runs_and_messages() {
    let mut thread = Thread::default();

    thread.push(&ev(Some("ada"), Kind::TurnStart));
    thread.push(&ev(
        Some("ada"),
        Kind::Thought {
            text: "Look at the logs first.".into(),
        },
    ));
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
        Kind::Thought {
            text: "Cause found: CI=true missing, so I am fixing it now.".into(),
        },
    ));
    thread.push(&ev(Some("ada"), Kind::Text { text: "Fixed.".into() }));

    // the 1st thought: in the run before its call; the 2nd: on the message
    let Item::Run { thoughts, .. } = &thread.items[0] else {
        panic!()
    };

    assert_eq!(thoughts, &vec![(0, "Look at the logs first.".to_string())]);

    thoughts_fold_into_runs_and_messages_verify_none(thread);
}

fn thoughts_fold_into_runs_and_messages_verify_none(thread: Thread) {
    let Item::Text { thoughts, key, .. } = &thread.items[1] else {
        panic!()
    };

    assert_eq!((thoughts.len(), *key), (1, 1));

    let none = HashSet::new();

    let fold = |thread_value: &Thread, mode: Thoughts, open: &HashSet<usize>| -> String {
        let file = Fold {
            runs: &none,
            all_runs: false,
            thoughts: mode,
            open_thoughts: open,
            all_thoughts: false,
        };

        text(&draw(
            thread_value,
            Look::Bubbles,
            50,
            &Colors::new(),
            &[],
            &file,
            &Picker::default(),
        ))
    };

    // hidden: 0 thought rows, 0 count
    let text_value = fold(&thread, Thoughts::Hidden, &none);

    assert!(
        !text_value.contains('∴') && !text_value.contains("thought"),
        "{text_value}"
    );

    // folded: a count on the run, 1 folded line on the message (cut to the box)
    let text_2 = fold(&thread, Thoughts::Folded, &none);

    assert!(text_2.contains("1 call · 1 thought · "), "{text_2}");

    let line = text_2.lines().find(|result| result.contains("▸ ∴")).expect(&text_2);

    assert!(
        line.starts_with("│ ▸ ∴ Cause found") && line.contains('…') && line.ends_with('│'),
        "{text_2}"
    );
    thoughts_fold_into_runs_and_messages_verify_open(thread, &none, fold, &text_2);
}

fn thoughts_fold_into_runs_and_messages_verify_open(
    mut thread: Thread,
    none: &HashSet<usize>,
    fold: impl Fn(&Thread, Thoughts, &HashSet<usize>) -> String,
    text_2: &str,
) {
    assert!(
        text_2.lines().position(|result_value| result_value.contains("▸ ∴"))
            < text_2.lines().position(|result_2| result_2.contains("Fixed.")),
        "{text_2}"
    );

    // a click opens it: the full text, wrapped
    let open: HashSet<usize> = [1].into();
    let text_3 = fold(&thread, Thoughts::Folded, &open);

    assert!(
        text_3.contains("▾ ∴ Cause found") && text_3.contains("now."),
        "{text_3}"
    );

    // open: every thought, the run's too (under its folded row)
    let text_4 = fold(&thread, Thoughts::Open, none);

    assert!(
        text_4.contains("∴ Look at the logs first.") && text_4.contains("now."),
        "{text_4}"
    );

    // a turn that ends on a thought, 0 text: a message of its own; hidden: 0 rows
    thread.push(&ev(
        Some("ada"),
        Kind::Thought {
            text: "Nothing left.".into(),
        },
    ));
    thread.push(&ev(
        Some("ada"),
        Kind::TurnEnd {
            cost: 0.0,
            input: 0,
            output: 0,
            cache_read: 0,
            cache_write: 0,
            ctx: 0,
            window: 0,
            ms: 1,
            error: None,
        },
    ));

    assert!(matches!(&thread.items[2], Item::Text { text, .. } if text.is_empty()));
    assert!(!fold(&thread, Thoughts::Hidden, none).contains("Nothing left"));
    assert!(fold(&thread, Thoughts::Folded, none).contains("▸ ∴ Nothing left."));
}
