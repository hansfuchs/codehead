use super::*;

/// A `working` status: 0 chat row (state kept for the card); blocked / done: 1 row each.
#[test]
fn a_working_status_adds_0_chat_rows() {
    let mut thread = Thread::default();

    thread.push(&ev(
        Some("ada"),
        Kind::Status {
            state: "working".into(),
            note: "subthread display: chat → status line".into(),
        },
    ));

    assert!(
        thread.items.is_empty() && thread.status["ada"].0 == "working",
        "{:?}",
        thread.items
    );

    thread.push(&ev(
        Some("ada"),
        Kind::Status {
            state: "blocked".into(),
            note: "waits on @bob".into(),
        },
    ));
    thread.push(&ev(
        Some("ada"),
        Kind::Status {
            state: "done".into(),
            note: String::new(),
        },
    ));

    assert_eq!(thread.items.len(), 2);
}

/// Flow lines (an agent's, an older log's thread flow): 0 chat items (the agent card shows
/// the step).
#[test]
fn flow_lines_add_0_chat_items() {
    let mut thread = Thread::default();

    thread.push(&ev(
        Some("ada"),
        Kind::Text {
            text: "plan written".into(),
        },
    ));
    thread.push(&ev(
        Some("ada"),
        Kind::Flow {
            op: "step".into(),
            text: "▶ refactor · step 3/8 plan approval · @ada".into(),
        },
    ));
    thread.push(&ev(
        None,
        Kind::Flow {
            op: "wait".into(),
            text: "owner: approve · plan approval".into(),
        },
    ));
    thread.push(&ev(Some("ada"), Kind::Text { text: "waiting".into() }));

    assert_eq!(thread.items.len(), 2, "{:?}", thread.items);
    assert!(
        !thread
            .items
            .iter()
            .any(|it| matches!(it, Item::Note { agent, .. } if agent == "flow"))
    );
}

#[test]
fn the_edits_note_is_1_row_cut_with_an_ellipsis() {
    let none = HashSet::new();
    let fold = Fold {
        runs: &none,
        all_runs: false,
        thoughts: Thoughts::Hidden,
        open_thoughts: &none,
        all_thoughts: false,
    };
    let long = (0..30)
        .map(|i| format!("src/f{i}.rs:{i}-{}", i + 9))
        .collect::<Vec<_>>()
        .join(", ");

    for look in [Look::Bubbles, Look::Lines] {
        let mut thread = Thread::default();

        thread.push(&ev(
            Some("ada"),
            Kind::Edits {
                summary: format!("owner: {long}"),
            },
        ));
        thread.push(&ev(
            Some("bob"),
            Kind::Edits {
                summary: "owner: a.rs:4".into(),
            },
        ));
        let dump = text(&draw(&thread, look, 60, &Colors::new(), &[], &fold, &Picker::default()));
        let rows: Vec<&str> = dump.lines().filter(|row| row.contains("got the edits")).collect();

        assert_eq!(rows.len(), 2, "{dump}");
        assert!(
            rows[0].starts_with("● ada got the edits") && rows[0].ends_with('…') && rows[0].width() <= 60,
            "{dump}"
        );
        assert!(rows[1].ends_with("owner: a.rs:4"), "{dump}");
        assert!(!dump.contains("src/f29.rs"), "{dump}");
    }

    // other notes wrap as before; chat search reads the whole edits note
    assert_other_notes_wrap(fold, &long);
}

fn assert_other_notes_wrap(fold: Fold<'_>, long: &str) {
    let mut thread = Thread::default();

    thread.push(&ev(
        Some("ada"),
        Kind::Edits {
            summary: format!("owner: {long}"),
        },
    ));
    thread.push(&ev(
        Some("ada"),
        Kind::Error {
            message: format!("boom {long}"),
        },
    ));
    let dump = text(&draw(
        &thread,
        Look::Lines,
        60,
        &Colors::new(),
        &[],
        &fold,
        &Picker::default(),
    ));

    assert!(dump.contains("src/f29.rs"), "the error wraps in full: {dump}");
    assert!(matches!(&thread.items[0], Item::Edits { text, .. } if text.contains("src/f29.rs:29-38")));
}

#[test]
fn interrupted_turns_are_no_errors() {
    let mut thread = Thread::default();
    let end = |error: &str| Kind::TurnEnd {
        cost: 0.0,
        input: 0,
        output: 0,
        cache_read: 0,
        cache_write: 0,
        ctx: 0,
        window: 0,
        ms: 1,
        error: Some(error.into()),
    };

    thread.push(&ev(Some("ada"), Kind::TurnStart));
    thread.push(&ev(Some("ada"), end(crate::engine::host::INTERRUPTED)));

    assert!(
        matches!(thread.items.last(), Some(Item::Note { text, color, .. }) if text == "interrupted" && *color == theme::pal().subtext0)
    );

    thread.push(&ev(Some("ada"), Kind::TurnStart));
    thread.push(&ev(Some("ada"), end("error_during_execution")));

    assert!(matches!(thread.items.last(), Some(Item::Note { color, .. }) if *color == theme::pal().error()));
}
