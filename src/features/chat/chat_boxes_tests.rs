use super::*;

#[test]
fn a_box_of_one_agent_splits_by_minute_with_a_timed_divider() {
    let mut thread = Thread::default();
    let at = |ts: u64, text: &str| Event {
        ts,
        agent: Some("ada".into()),
        kind: Kind::Text { text: text.into() },
    };
    let base = 1_795_090_200_000;

    thread.push(&at(base, "first"));
    thread.push(&at(base + 20_000, "same minute"));
    thread.push(&at(base + 65_000, "next minute"));
    let none = HashSet::new();
    let fold = Fold {
        runs: &none,
        all_runs: false,
        thoughts: Thoughts::Hidden,
        open_thoughts: &none,
        all_thoughts: false,
    };

    a_box_of_one_agent_splits_by_minute_with_a_timed_divider_verify_color(&thread, base, fold);
}

fn a_box_of_one_agent_splits_by_minute_with_a_timed_divider_verify_color(thread: &Thread, base: u64, fold: Fold<'_>) {
    for look in [Look::Bubbles, Look::Lines] {
        let color = text(&draw(thread, look, 60, &Colors::new(), &[], &fold, &Picker::default()));
        let rows: Vec<&str> = color.lines().collect();
        let (m0, m1) = (clock(base), clock(base + 65_000));

        // 1 box (bubbles: 1 top border), its time the 1st item's; 1 divider: the 2nd minute
        if look == Look::Bubbles {
            assert_eq!(
                rows.iter().filter(|result| result.starts_with('┌')).count(),
                1,
                "{color}"
            );
            assert!(rows[0].contains(&m0), "{color}");
        }

        let div: Vec<usize> = rows
            .iter()
            .enumerate()
            .filter(|(_, result_value)| {
                result_value.contains("──") && !result_value.starts_with('┌') && !result_value.starts_with('└')
            })
            .map(|(i, _)| i)
            .collect();

        assert_eq!(div.len(), 1, "{look:?}\n{color}");

        let i = div[0];

        // bubbles: the divider joins both borders, as wide as the box
        if look == Look::Bubbles {
            assert!(
                rows[i].starts_with('├')
                    && rows[i].ends_with("─┤")
                    && rows[i].chars().count() == rows[0].chars().count(),
                "{color}"
            );
        }

        assert!(rows[i].contains(&m1), "{look:?}\n{color}");

        // blank rows above and below the divider
        let blank = |result_2: &str| {
            result_2
                .trim_matches(|character: char| character == '│' || character == ' ')
                .is_empty()
        };

        assert!(blank(rows[i - 1]) && blank(rows[i + 1]), "{look:?}\n{color}");
        assert!(
            rows[..i].iter().any(|result_3| result_3.contains("same minute"))
                && rows[i..].iter().any(|result_4| result_4.contains("next minute")),
            "{look:?}\n{color}"
        );
    }
}

#[test]
fn a_new_turn_shows_no_box_until_its_1st_output() {
    let mut thread = Thread::default();

    thread.push(&ev(Some("ada"), Kind::TurnStart));
    thread.push(&ev(
        Some("ada"),
        Kind::Text {
            text: "first answer".into(),
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
    thread.push(&ev(
        None,
        Kind::Prompt {
            from: "owner".into(),
            to: vec!["ada".into()],
            text: "next".into(),
        },
    ));
    thread.push(&ev(Some("ada"), Kind::TurnStart));
    let fold = Fold {
        runs: &HashSet::new(),
        all_runs: false,
        thoughts: Thoughts::Hidden,
        open_thoughts: &HashSet::new(),
        all_thoughts: false,
    };
    let color = draw(
        &thread,
        Look::Bubbles,
        50,
        &Colors::new(),
        &[],
        &fold,
        &Picker::default(),
    );

    a_new_turn_shows_no_box_until_its_1st_output_verify_text_value(thread, fold, &color);
}

fn a_new_turn_shows_no_box_until_its_1st_output_verify_text_value(mut thread: Thread, fold: Fold<'_>, color: &Drawn) {
    let text_value = text(color);
    let rows: Vec<&str> = text_value.lines().collect();
    let bubble = rows.iter().position(|result| result.contains("next")).unwrap();

    // 0 output yet: 0 empty box below the owner's message, 0 wave on the answer above it
    assert!(color.waves.is_empty(), "{text_value}");
    assert!(
        !rows[bubble..]
            .iter()
            .any(|result_value| result_value.starts_with("└─ ada")),
        "{text_value}"
    );
    assert_eq!(
        rows.iter().filter(|result_2| result_2.starts_with("└─ ada")).count(),
        1,
        "{text_value}"
    );

    // the 1st output of the turn: the wave moves into its box (bottom border), 0 empty box
    thread.push(&ev(Some("ada"), Kind::Text { text: "second".into() }));
    let color_value = draw(
        &thread,
        Look::Bubbles,
        50,
        &Colors::new(),
        &[],
        &fold,
        &Picker::default(),
    );
    let text_2 = text(&color_value);
    let rows: Vec<&str> = text_2.lines().collect();

    assert_eq!(color_value.waves.len(), 1, "{text_2}");
    assert!(
        rows[color_value.waves[0].0 - 2].contains("second") && rows[color_value.waves[0].0].starts_with("└─ ada"),
        "{text_2}"
    );
}

/// An agent's name: in the bottom border of its box (agent → agent too), cut to fit; the
/// open ask keeps its label on top; a box's bottom border is its last item's row.
#[test]
fn the_agent_name_sits_in_the_bottom_border_of_its_box() {
    let mut thread = Thread::default();

    thread.push(&ev(Some("ada"), Kind::Text { text: "one".into() }));
    thread.push(&ev(
        None,
        Kind::Prompt {
            from: "ada".into(),
            to: vec!["bob".into()],
            text: "take it".into(),
        },
    ));
    thread.push(&ev(
        None,
        Kind::Prompt {
            from: "owner".into(),
            to: vec!["bob".into()],
            text: "go".into(),
        },
    ));
    thread.push(&ev(Some("bob"), Kind::Ask { id: "q".into(), tool: "AskUserQuestion".into(), input: json!({"questions": [{"question": "color?", "header": "Color", "options": [{"label": "red", "description": ""}], "multiSelect": false}]}) }));
    let fold = Fold {
        runs: &HashSet::new(),
        all_runs: false,
        thoughts: Thoughts::Hidden,
        open_thoughts: &HashSet::new(),
        all_thoughts: false,
    };
    let color = draw(
        &thread,
        Look::Bubbles,
        40,
        &Colors::new(),
        &[],
        &fold,
        &Picker::default(),
    );
    let text_value = text(&color);
    let rows: Vec<&str> = text_value.lines().collect();

    let ada = rows
        .iter()
        .position(|result| result.starts_with("└─ ada ─"))
        .expect(&text_value);

    assert!(
        rows[ada - 2].starts_with("│ one") && !rows[..ada].iter().any(|result_value| result_value.contains("ada")),
        "{text_value}"
    );

    // padding: 0 blank row below a top border, 1 above a bottom border (owner bubbles too)
    let blank = |result_2: &str| result_2.trim().trim_matches('│').trim().is_empty();

    the_agent_name_sits_in_the_bottom_border_of_its_box_verify_thread_value(
        fold,
        &color,
        &text_value,
        &rows,
        ada,
        blank,
    );
}

fn the_agent_name_sits_in_the_bottom_border_of_its_box_verify_thread_value(
    fold: Fold<'_>,
    color: &Drawn,
    text_value: &str,
    rows: &[&str],
    ada: usize,
    blank: impl Fn(&str) -> bool,
) {
    for (i, row) in rows.iter().enumerate() {
        if row.trim_start().starts_with('┌') {
            assert!(!blank(rows[i + 1]), "row {}: {text_value}", i + 1);
        }

        if row.trim_start().starts_with('└') {
            assert!(
                blank(rows[i - 1]) && rows[i - 1].contains('│'),
                "row {}: {text_value}",
                i - 1
            );
        }
    }

    assert!(
        rows.iter().any(|result_3| result_3.starts_with("└─ ada → bob ─"))
            && !rows.iter().any(|result_4| result_4.starts_with("┌─ ada")),
        "{text_value}"
    );
    assert!(
        rows.iter().any(|result_5| result_5.starts_with("┌─ bob asks ")),
        "{text_value}"
    );
    assert!(
        rows.iter().all(|result_6| result_6.chars().count() <= 40),
        "{text_value}"
    );

    // the bottom border: its box's last item's row
    assert!(color.starts[1] > ada, "{:?} {text_value}", color.starts);

    // a name wider than the border: cut with …, the border W wide
    let mut thread_value = Thread::default();

    thread_value.push(&ev(Some("a-very-long-agent-name"), Kind::Text { text: "hi".into() }));
    let color_value = text(&draw(
        &thread_value,
        Look::Bubbles,
        14,
        &Colors::new(),
        &[],
        &fold,
        &Picker::default(),
    ));
    let last = color_value.lines().last().unwrap();

    assert_eq!(last, "└─ a-very-… ─┘", "{color_value}");
}
