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

    let fold = |thread: &Thread, mode: Thoughts, open: &HashSet<usize>| -> String {
        let folds = Fold {
            runs: &none,
            all_runs: false,
            thoughts: mode,
            open_thoughts: open,
            all_thoughts: false,
        };

        text(&draw(
            thread,
            Look::Bubbles,
            50,
            &Colors::new(),
            &[],
            &folds,
            &Picker::default(),
        ))
    };

    // hidden: 0 thought rows, 0 count
    let hidden = fold(&thread, Thoughts::Hidden, &none);

    assert!(!hidden.contains('∴') && !hidden.contains("thought"), "{hidden}");

    // folded: a count on the run, 1 folded line on the message (cut to the box)
    let folded = fold(&thread, Thoughts::Folded, &none);

    assert!(folded.contains("1 call · 1 thought · "), "{folded}");

    let line = folded.lines().find(|row| row.contains("▸ ∴")).expect(&folded);

    assert!(
        line.starts_with("│ ▸ ∴ Cause found") && line.contains('…') && line.ends_with('│'),
        "{folded}"
    );
    thoughts_fold_into_runs_and_messages_verify_open(thread, &none, fold, &folded);
}

fn thoughts_fold_into_runs_and_messages_verify_open(
    mut thread: Thread,
    none: &HashSet<usize>,
    fold: impl Fn(&Thread, Thoughts, &HashSet<usize>) -> String,
    folded: &str,
) {
    assert!(
        folded.lines().position(|row| row.contains("▸ ∴")) < folded.lines().position(|row| row.contains("Fixed.")),
        "{folded}"
    );

    // a click opens it: the full text, wrapped
    let open: HashSet<usize> = [1].into();
    let clicked = fold(&thread, Thoughts::Folded, &open);

    assert!(
        clicked.contains("▾ ∴ Cause found") && clicked.contains("now."),
        "{clicked}"
    );

    // open: every thought, the run's too (under its folded row)
    let all_open = fold(&thread, Thoughts::Open, none);

    assert!(
        all_open.contains("∴ Look at the logs first.") && all_open.contains("now."),
        "{all_open}"
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
