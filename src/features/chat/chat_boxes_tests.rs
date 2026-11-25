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

    assert_minute_divider(&thread, base, fold);
}

fn assert_minute_divider(thread: &Thread, base: u64, fold: Fold<'_>) {
    for look in [Look::Bubbles, Look::Lines] {
        let dump = text(&draw(thread, look, 60, &Colors::new(), &[], &fold, &Picker::default()));
        let rows: Vec<&str> = dump.lines().collect();
        let (m0, m1) = (clock(base), clock(base + 65_000));

        // 1 box (bubbles: 1 top border), its time the 1st item's; 1 divider: the 2nd minute
        if look == Look::Bubbles {
            assert_eq!(rows.iter().filter(|row| row.starts_with('┌')).count(), 1, "{dump}");
            assert!(rows[0].contains(&m0), "{dump}");
        }

        let div: Vec<usize> = rows
            .iter()
            .enumerate()
            .filter(|(_, row)| row.contains("──") && !row.starts_with('┌') && !row.starts_with('└'))
            .map(|(i, _)| i)
            .collect();

        assert_eq!(div.len(), 1, "{look:?}\n{dump}");

        let i = div[0];

        // bubbles: the divider joins both borders, as wide as the box
        if look == Look::Bubbles {
            assert!(
                rows[i].starts_with('├')
                    && rows[i].ends_with("─┤")
                    && rows[i].chars().count() == rows[0].chars().count(),
                "{dump}"
            );
        }

        assert!(rows[i].contains(&m1), "{look:?}\n{dump}");

        // blank rows above and below the divider
        let blank = |row: &str| {
            row.trim_matches(|character: char| character == '│' || character == ' ')
                .is_empty()
        };

        assert!(blank(rows[i - 1]) && blank(rows[i + 1]), "{look:?}\n{dump}");
        assert!(
            rows[..i].iter().any(|row| row.contains("same minute"))
                && rows[i..].iter().any(|row| row.contains("next minute")),
            "{look:?}\n{dump}"
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
    let drawn = draw(
        &thread,
        Look::Bubbles,
        50,
        &Colors::new(),
        &[],
        &fold,
        &Picker::default(),
    );

    assert_wave_waits_for_output(thread, fold, &drawn);
}

fn assert_wave_waits_for_output(mut thread: Thread, fold: Fold<'_>, drawn: &Drawn) {
    let dump = text(drawn);
    let rows: Vec<&str> = dump.lines().collect();
    let bubble = rows.iter().position(|row| row.contains("next")).unwrap();

    // 0 output yet: 0 empty box below the owner's message, 0 wave on the answer above it
    assert!(drawn.waves.is_empty(), "{dump}");
    assert!(!rows[bubble..].iter().any(|row| row.starts_with("└─ ada")), "{dump}");
    assert_eq!(rows.iter().filter(|row| row.starts_with("└─ ada")).count(), 1, "{dump}");

    // the 1st output of the turn: the wave moves into its box (bottom border), 0 empty box
    thread.push(&ev(Some("ada"), Kind::Text { text: "second".into() }));
    let after = draw(
        &thread,
        Look::Bubbles,
        50,
        &Colors::new(),
        &[],
        &fold,
        &Picker::default(),
    );
    let after_dump = text(&after);
    let rows: Vec<&str> = after_dump.lines().collect();

    assert_eq!(after.waves.len(), 1, "{after_dump}");
    assert!(
        rows[after.waves[0].0 - 2].contains("second") && rows[after.waves[0].0].starts_with("└─ ada"),
        "{after_dump}"
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
    let drawn = draw(
        &thread,
        Look::Bubbles,
        40,
        &Colors::new(),
        &[],
        &fold,
        &Picker::default(),
    );
    let dump = text(&drawn);
    let rows: Vec<&str> = dump.lines().collect();

    let ada = rows.iter().position(|row| row.starts_with("└─ ada ─")).expect(&dump);

    assert!(
        rows[ada - 2].starts_with("│ one") && !rows[..ada].iter().any(|row| row.contains("ada")),
        "{dump}"
    );

    // padding: 0 blank row below a top border, 1 above a bottom border (owner bubbles too)
    let blank = |row: &str| row.trim().trim_matches('│').trim().is_empty();

    assert_box_borders(fold, &drawn, &dump, &rows, ada, blank);
}

fn assert_box_borders(
    fold: Fold<'_>,
    drawn: &Drawn,
    dump: &str,
    rows: &[&str],
    ada: usize,
    blank: impl Fn(&str) -> bool,
) {
    for (i, row) in rows.iter().enumerate() {
        if row.trim_start().starts_with('┌') {
            assert!(!blank(rows[i + 1]), "row {}: {dump}", i + 1);
        }

        if row.trim_start().starts_with('└') {
            assert!(blank(rows[i - 1]) && rows[i - 1].contains('│'), "row {}: {dump}", i - 1);
        }
    }

    assert!(
        rows.iter().any(|row| row.starts_with("└─ ada → bob ─")) && !rows.iter().any(|row| row.starts_with("┌─ ada")),
        "{dump}"
    );
    assert!(rows.iter().any(|row| row.starts_with("┌─ bob asks ")), "{dump}");
    assert!(rows.iter().all(|row| row.chars().count() <= 40), "{dump}");

    // the bottom border: its box's last item's row
    assert!(drawn.starts[1] > ada, "{:?} {dump}", drawn.starts);

    // a name wider than the border: cut with …, the border W wide
    let mut long_name = Thread::default();

    long_name.push(&ev(Some("a-very-long-agent-name"), Kind::Text { text: "hi".into() }));
    let long_dump = text(&draw(
        &long_name,
        Look::Bubbles,
        14,
        &Colors::new(),
        &[],
        &fold,
        &Picker::default(),
    ));
    let last = long_dump.lines().last().unwrap();

    assert_eq!(last, "└─ a-very-… ─┘", "{long_dump}");
}
