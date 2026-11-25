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
        let color = draw(&thread, look, 70, &Colors::new(), &[], &fold, &Picker::default());
        let row = color
            .rows
            .iter()
            .find(|result| result.copies.iter().any(|character| character.2 == msg))
            .unwrap_or_else(|| panic!("{look:?}: 0 copy button"));
        let (agent, bytes, _) = row
            .copies
            .iter()
            .find(|character_value| character_value.2 == msg)
            .unwrap()
            .clone();
        let cells: String = row
            .line
            .spans
            .iter()
            .flat_map(|text_value| text_value.content.chars())
            .collect();
        let hit: String = cells
            .chars()
            .skip(usize::from(agent))
            .take(usize::from(bytes - agent))
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
        let color = draw(&thread, look, 70, &Colors::new(), &[], &fold, &Picker::default());
        let text_value = text(&color);

        assert!(
            text_value.contains("fix the deploy now") && !text_value.contains("**"),
            "{look:?} bold: 0 asterisks\n{text_value}"
        );
        assert!(
            text_value.contains("• 1st step") && text_value.contains("• 2nd step") && !text_value.contains('`'),
            "{look:?} list + inline code\n{text_value}"
        );

        // a line break of the source stays
        assert!(
            text_value.lines().any(|line_value| line_value
                .trim_end()
                .trim_end_matches('│')
                .trim()
                .ends_with("keep this line")),
            "{look:?}\n{text_value}"
        );

        let bold = color
            .rows
            .iter()
            .flat_map(|result| result.line.spans.iter())
            .find(|sp| sp.content.contains("deploy"))
            .unwrap();

        assert!(bold.style.add_modifier.contains(Modifier::BOLD), "{look:?} bold span");
    }
}
