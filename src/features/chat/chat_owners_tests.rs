use super::*;

#[test]
fn owner_messages_have_a_copy_button_in_both_looks() {
    let mut thread = Thread::default();
    let msg = "fix **the deploy**\nsecond line";

    thread.push(&ev(
        None,
        Kind::Prompt {
            from: "owner".into(),
            to: vec!["ada".into()],
            text: msg.into(),
        },
    ));
    let none = HashSet::new();
    let fold = Fold {
        runs: &none,
        all_runs: false,
        thoughts: Thoughts::Hidden,
        open_thoughts: &none,
        all_thoughts: false,
    };

    for look in [Look::Bubbles, Look::Lines] {
        let drawn = draw(&thread, look, 70, &Colors::new(), &[], &fold, &Picker::default());
        let row = drawn
            .rows
            .iter()
            .find(|row| row.copies.iter().any(|copy| copy.2 == msg))
            .unwrap_or_else(|| panic!("{look:?}: 0 copy button"));
        let (start, end, _) = row.copies.iter().find(|copy| copy.2 == msg).unwrap().clone();
        let cells: String = row.line.spans.iter().flat_map(|span| span.content.chars()).collect();
        let hit: String = cells
            .chars()
            .skip(usize::from(start))
            .take(usize::from(end - start))
            .collect();

        assert_eq!(
            hit.trim(),
            "copy",
            "{look:?}: the copy range sits on the button: {cells:?}"
        );
    }
}

#[test]
fn owner_messages_render_markdown_in_both_looks() {
    let mut thread = Thread::default();

    thread.push(&ev(
        None,
        Kind::Prompt {
            from: "owner".into(),
            to: vec!["ada".into()],
            text: "fix **the deploy** now\n- 1st `step`\n- 2nd step\nkeep this line".into(),
        },
    ));
    let none = HashSet::new();
    let fold = Fold {
        runs: &none,
        all_runs: false,
        thoughts: Thoughts::Hidden,
        open_thoughts: &none,
        all_thoughts: false,
    };

    for look in [Look::Bubbles, Look::Lines] {
        let drawn = draw(&thread, look, 70, &Colors::new(), &[], &fold, &Picker::default());
        let dump = text(&drawn);

        assert!(
            dump.contains("fix the deploy now") && !dump.contains("**"),
            "{look:?} bold: 0 asterisks\n{dump}"
        );
        assert!(
            dump.contains("• 1st step") && dump.contains("• 2nd step") && !dump.contains('`'),
            "{look:?} list + inline code\n{dump}"
        );

        // a line break of the source stays
        assert!(
            dump.lines()
                .any(|line| line.trim_end().trim_end_matches('│').trim().ends_with("keep this line")),
            "{look:?}\n{dump}"
        );

        let bold = drawn
            .rows
            .iter()
            .flat_map(|row| row.line.spans.iter())
            .find(|span| span.content.contains("deploy"))
            .unwrap();

        assert!(bold.style.add_modifier.contains(Modifier::BOLD), "{look:?} bold span");
    }
}
