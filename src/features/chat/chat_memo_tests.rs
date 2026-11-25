use super::*;

#[test]
fn memo_redraw_shows_new_tool_output_and_equals_a_cold_draw() {
    let mut thread = Thread::default();

    thread.push(&ev(
        Some("ada"),
        Kind::Tool {
            id: "1".into(),
            name: "Bash".into(),
            input: json!({"command": "ls"}),
        },
    ));
    let open: HashSet<usize> = [1].into();
    let none = HashSet::new();
    let fold = Fold {
        runs: &open,
        all_runs: false,
        thoughts: Thoughts::Hidden,
        open_thoughts: &none,
        all_thoughts: false,
    };
    let first = text(&draw(
        &thread,
        Look::Bubbles,
        60,
        &Colors::new(),
        &[],
        &fold,
        &Picker::default(),
    ));

    assert!(!first.contains("fresh-output"), "{first}");

    thread.push(&ev(
        Some("ada"),
        Kind::ToolResult {
            id: "1".into(),
            ok: true,
            output: "fresh-output".into(),
        },
    ));
    verify_memo_redraw_shows_new_tool_output_and_equals_a_cold_draw(thread, fold);
}

fn verify_memo_redraw_shows_new_tool_output_and_equals_a_cold_draw(mut thread: Thread, fold: Fold<'_>) {
    thread.push(&ev(Some("ada"), Kind::Text { text: "a **b**".into() }));
    let warm = text(&draw(
        &thread,
        Look::Bubbles,
        60,
        &Colors::new(),
        &[],
        &fold,
        &Picker::default(),
    ));

    assert!(warm.contains("fresh-output"), "{warm}");

    let again = text(&draw(
        &thread,
        Look::Bubbles,
        60,
        &Colors::new(),
        &[],
        &fold,
        &Picker::default(),
    ));

    assert_eq!(warm, again);

    // a new width: new rows, 0 stale ones
    let narrow = draw(
        &thread,
        Look::Bubbles,
        30,
        &Colors::new(),
        &[],
        &fold,
        &Picker::default(),
    );

    assert!(widest(&narrow) <= 30);
}
