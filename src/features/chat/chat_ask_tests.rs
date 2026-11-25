use super::*;

fn question_spec(question: &str, header: &str, multi: bool, opts: &[&str]) -> Value {
    json!({"question": question, "header": header, "multiSelect": multi, "options": opts.iter().map(|option| json!({"label": option, "description": format!("{option} desc")})).collect::<Vec<_>>()})
}

fn questions(id: &str, qs: &[Value]) -> Ask {
    Ask {
        id: id.into(),
        agent: "ada".into(),
        tool: "AskUserQuestion".into(),
        input: json!({"questions": qs}),
        ts: 1,
    }
}

fn permission() -> Ask {
    Ask {
        id: "p1".into(),
        agent: "ada".into(),
        tool: "Bash".into(),
        input: json!({"command": "ls"}),
        ts: 1,
    }
}

fn picker_for(ask: &Ask) -> Picker {
    let mut picker = Picker::default();

    picker.sync(Some(ask));
    picker
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

/// Keys of a space-separated list: ↑ ↓ ← → ⏎ ␣ ⌫ ⇥, else each char.
fn press(picker: &mut Picker, ask: &Ask, keys: &str) -> Act {
    let mut last = Act::Pass;

    for token in keys.split(' ').filter(|token| !token.is_empty()) {
        let codes: Vec<KeyCode> = match token {
            "↑" => vec![KeyCode::Up],
            "↓" => vec![KeyCode::Down],
            "←" => vec![KeyCode::Left],
            "→" => vec![KeyCode::Right],
            "⏎" => vec![KeyCode::Enter],
            "␣" => vec![KeyCode::Char(' ')],
            "⌫" => vec![KeyCode::Backspace],
            "⇥" => vec![KeyCode::Tab],
            text => text.chars().map(KeyCode::Char).collect(),
        };

        for code in codes {
            last = picker.key(ask, key(code));
        }
    }

    last
}

fn answer(act: Act) -> Value {
    match act {
        Act::Answer(value) => value,
        other => panic!("0 answer: {other:?}"),
    }
}

#[test]
fn single_question_arrows_go_round_and_enter_answers_the_cursor_row() {
    let color = questions(
        "q1",
        &[question_spec("Color?", "Color", false, &["red", "green", "blue"])],
    );
    let mut picker = picker_for(&color);

    assert!(picker.focus);

    // 3 options + Other: 4 rows, round
    press(&mut picker, &color, "↑");

    assert_eq!(picker.at, 3);

    press(&mut picker, &color, "↓ ↓ ↓");

    assert_eq!(picker.at, 2);

    let value = answer(press(&mut picker, &color, "⏎"));

    assert_eq!(value["answers"]["Color?"], "blue");
    assert_eq!(
        (value["id"].as_str(), value["allow"].as_bool(), value["label"].as_str()),
        (Some("q1"), Some(true), Some("blue"))
    );
}

#[test]
fn single_question_space_and_digits_answer_at_once() {
    let color = questions("q1", &[question_spec("Color?", "", false, &["red", "green"])]);
    let mut picker = picker_for(&color);

    assert_eq!(answer(press(&mut picker, &color, "↓ ␣"))["answers"]["Color?"], "green");

    let mut by_digit = picker_for(&color);

    assert_eq!(answer(press(&mut by_digit, &color, "1"))["answers"]["Color?"], "red");

    // a digit past the rows: 0 change; the Other row's digit: the cursor only
    let mut past_rows = picker_for(&color);

    assert_eq!(press(&mut past_rows, &color, "9"), Act::Changed);
    assert_eq!(past_rows.at, 0);
    assert_eq!(press(&mut past_rows, &color, "3"), Act::Changed);
    assert_eq!(past_rows.at, 2);
}

#[test]
fn multi_question_space_toggles_and_enter_sends_the_picks_in_option_order() {
    let ask = questions("q1", &[question_spec("Which?", "Keys", true, &["a", "b", "c", "d"])]);
    let mut picker = picker_for(&ask);

    press(&mut picker, &ask, "↓ ↓ ␣ ↑ ↑ ␣ ↓ ␣ ␣");
    let rows = picker.rows(&ask);
    let picks: Vec<bool> = rows.iter().map(|row| row.picked).collect();

    assert_eq!(picks, vec![true, false, true, false, false]);
    assert_eq!(answer(press(&mut picker, &ask, "⏎"))["answers"]["Which?"], "a, c");

    // digits toggle too, enter with 0 picks: the cursor row
    let mut by_digits = picker_for(&ask);

    press(&mut by_digits, &ask, "2 4 2");

    assert_eq!(answer(press(&mut by_digits, &ask, "⏎"))["answers"]["Which?"], "d");

    let mut cursor_only = picker_for(&ask);

    press(&mut cursor_only, &ask, "↓ ↓");

    assert_eq!(answer(press(&mut cursor_only, &ask, "⏎"))["answers"]["Which?"], "c");
}

#[test]
fn letters_type_into_other_and_enter_sends_the_text() {
    let ask = questions("q1", &[question_spec("Name?", "", false, &["ada", "bob"])]);
    let mut picker = picker_for(&ask);

    press(&mut picker, &ask, "Cl");

    assert_eq!(picker.at, 2);

    press(&mut picker, &ask, "x ⌫ a u d e ␣ 5");

    assert_eq!(picker.rows(&ask)[2].label, "Claude 5");
    assert_eq!(answer(press(&mut picker, &ask, "⏎"))["answers"]["Name?"], "Claude 5");

    // Other with 0 text: 0 answer; space on it: 0 text
    let mut empty_other = picker_for(&ask);

    press(&mut empty_other, &ask, "↑");

    assert_eq!(press(&mut empty_other, &ask, "⏎"), Act::Changed);
    assert_eq!(press(&mut empty_other, &ask, "␣"), Act::Changed);
    assert_eq!(empty_other.rows(&ask)[2].label, "");
}

#[test]
fn multi_other_text_is_picked_and_comes_last() {
    let ask = questions("q1", &[question_spec("Which?", "", true, &["a", "b"])]);
    let mut picker = picker_for(&ask);

    press(&mut picker, &ask, "␣ zed");

    assert!(picker.rows(&ask)[2].picked);
    assert_eq!(answer(press(&mut picker, &ask, "⏎"))["answers"]["Which?"], "a, zed");

    // backspace to 0 text: Other unpicked
    let mut erased = picker_for(&ask);

    press(&mut erased, &ask, "z ⌫");

    assert!(!erased.rows(&ask)[2].picked);

    press(&mut erased, &ask, "↑ ↑ ␣");

    assert_eq!(answer(press(&mut erased, &ask, "⏎"))["answers"]["Which?"], "a");
}

#[test]
fn several_questions_go_in_order_back_and_forth_then_send_all() {
    let ask = questions(
        "q1",
        &[
            question_spec("One?", "First", false, &["x", "y"]),
            question_spec("Two?", "Second", true, &["m", "n", "o"]),
            question_spec("Three?", "Third", false, &["p", "q"]),
        ],
    );
    let mut picker = picker_for(&ask);

    // → before an answer: 0 move
    press(&mut picker, &ask, "→");

    assert_eq!(picker.question, 0);
    assert_eq!(press(&mut picker, &ask, "↓ ⏎"), Act::Changed);
    assert_eq!((picker.question, picker.at), (1, 0));

    press(&mut picker, &ask, "␣ ↓ ↓ ␣ ⏎");

    assert_eq!(picker.question, 2);

    // back to the 1st: the cursor on its answer; another answer, then the open 3rd
    press(&mut picker, &ask, "← ←");

    assert_eq!((picker.question, picker.at), (0, 1));
    assert_eq!(
        picker.answered(&ask),
        vec![("One?".into(), "y".into()), ("Two?".into(), "m, o".into())]
    );

    press(&mut picker, &ask, "↑ ⏎");

    assert_eq!(picker.question, 2);

    let value = answer(press(&mut picker, &ask, "2"));

    assert_eq!(value["answers"], json!({"One?": "x", "Two?": "m, o", "Three?": "q"}));
    assert_eq!(value["label"], "x · m, o · q");
}

#[test]
fn going_back_to_an_answered_question_and_enter_sends_when_all_are_answered() {
    let ask = questions(
        "q1",
        &[
            question_spec("One?", "", false, &["x", "y"]),
            question_spec("Two?", "", false, &["m", "n"]),
        ],
    );
    let mut picker = picker_for(&ask);

    press(&mut picker, &ask, "⏎");
    press(&mut picker, &ask, "←");

    assert_eq!(picker.question, 0);

    // the 2nd still open: enter goes there
    assert_eq!(press(&mut picker, &ask, "↓ ⏎"), Act::Changed);
    assert_eq!(picker.question, 1);

    let value = answer(press(&mut picker, &ask, "⏎"));

    assert_eq!(value["answers"], json!({"One?": "y", "Two?": "m"}));
}

#[test]
fn without_the_focus_the_prompt_gets_every_key() {
    let color = questions("q1", &[question_spec("Color?", "", true, &["red", "green"])]);
    let mut picker = picker_for(&color);

    // tab: the view's focus ring moves the focus
    picker.focus = false;

    for code in [
        KeyCode::Down,
        KeyCode::Char(' '),
        KeyCode::Enter,
        KeyCode::Char('1'),
        KeyCode::Char('x'),
    ] {
        assert_eq!(picker.key(&color, key(code)), Act::Pass, "{code:?}");
    }

    assert!(picker.rows(&color).iter().all(|row| !row.picked));
    assert_eq!(picker.hint(&color), "tab: answer the question");
}

#[test]
fn with_the_focus_the_view_keeps_esc_scroll_menu_and_ctrl_keys() {
    let color = questions("q1", &[question_spec("Color?", "", false, &["red", "green"])]);
    let mut picker = picker_for(&color);

    for event in [
        key(KeyCode::Esc),
        key(KeyCode::PageUp),
        key(KeyCode::PageDown),
        key(KeyCode::F(2)),
        KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
        KeyEvent::new(KeyCode::Up, KeyModifiers::SHIFT),
        KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL),
    ] {
        assert_eq!(picker.key(&color, event), Act::Pass, "{event:?}");
    }

    // plain keys never reach the hidden prompt
    for code in [
        KeyCode::Left,
        KeyCode::Right,
        KeyCode::Backspace,
        KeyCode::Delete,
        KeyCode::Home,
    ] {
        assert_eq!(picker.key(&color, key(code)), Act::Changed, "{code:?}");
    }

    // shift + letter: a capital in Other
    assert_eq!(
        picker.key(&color, KeyEvent::new(KeyCode::Char('Z'), KeyModifiers::SHIFT)),
        Act::Changed
    );
    assert_eq!(picker.rows(&color)[2].label, "Z");
}

#[test]
fn permission_requests_answer_allow_or_deny() {
    let ask = permission();
    let mut picker = picker_for(&ask);

    assert_eq!(picker.rows(&ask).len(), 2);

    let value = answer(press(&mut picker, &ask, "⏎"));

    assert_eq!(
        (value["allow"].as_bool(), value["label"].as_str()),
        (Some(true), Some("allow"))
    );

    let mut denied = picker_for(&ask);

    assert_eq!(answer(press(&mut denied, &ask, "↓ ⏎"))["allow"], false);

    let mut by_digit = picker_for(&ask);

    assert_eq!(answer(press(&mut by_digit, &ask, "2"))["label"], "deny");

    // letters: 0 Other row, 0 answer
    let mut typed = picker_for(&ask);

    assert_eq!(press(&mut typed, &ask, "y"), Act::Changed);

    let mut by_space = picker_for(&ask);

    assert_eq!(answer(press(&mut by_space, &ask, "↓ ␣"))["allow"], false);
}

#[test]
fn acp_options_answer_with_their_option_id() {
    let options = Ask {
        id: "o1".into(),
        agent: "ada".into(),
        tool: "Edit".into(),
        input: json!({"_options": [{"id": "ok1", "name": "Allow once", "kind": "allow_once"}, {"id": "ok2", "name": "Allow always", "kind": "allow_always"}, {"id": "no", "name": "Reject", "kind": "reject_once"}]}),
        ts: 1,
    };
    let mut picker = picker_for(&options);

    assert_eq!(
        picker
            .rows(&options)
            .iter()
            .map(|row| row.label.as_str())
            .collect::<Vec<_>>(),
        vec!["Allow once", "Allow always", "Reject"]
    );

    let value = answer(press(&mut picker, &options, "↑ ⏎"));

    assert_eq!(
        (value["option"].as_str(), value["allow"].as_bool()),
        (Some("no"), Some(false))
    );

    let mut by_digit = picker_for(&options);
    let by_digit_answer = answer(press(&mut by_digit, &options, "2"));

    assert_eq!(
        (by_digit_answer["option"].as_str(), by_digit_answer["allow"].as_bool()),
        (Some("ok2"), Some(true))
    );
}

#[test]
fn sync_keeps_the_state_of_the_same_ask_and_resets_on_a_new_one() {
    let ask = questions("q1", &[question_spec("Which?", "", true, &["a", "b"])]);
    let mut picker = picker_for(&ask);

    press(&mut picker, &ask, "↓ ␣");
    picker.focus = false;
    picker.sync(Some(&ask));

    assert_eq!((picker.at, picker.focus), (1, false));
    assert!(picker.rows(&ask)[1].picked);

    let next = questions("q2", &[question_spec("Next?", "", false, &["x"])]);

    picker.sync(Some(&next));

    assert_eq!((picker.at, picker.focus, picker.question), (0, true, 0));
    assert!(picker.rows(&next).iter().all(|row| !row.picked));

    picker.sync(None);

    assert_eq!(picker.rows_n(&next), 2);
    assert!(!picker.focus);
}

#[test]
fn a_sent_answer_takes_0_keys() {
    let color = questions("q1", &[question_spec("Color?", "", false, &["red"])]);
    let mut picker = picker_for(&color);

    assert!(matches!(press(&mut picker, &color, "⏎"), Act::Answer(_)));

    picker.sent = true;

    assert_eq!(press(&mut picker, &color, "↓"), Act::Pass);
    assert_eq!(picker.hint(&color), "answer sent…");
}

#[test]
fn clicks_toggle_multi_rows_answer_single_ones_and_focus_the_ask() {
    let ask = questions("q1", &[question_spec("Which?", "", true, &["a", "b"])]);
    let mut picker = picker_for(&ask);

    picker.focus = false;

    assert_eq!(picker.click(&ask, 1), Act::Changed);
    assert!(picker.focus && picker.rows(&ask)[1].picked);
    assert_eq!(picker.click(&ask, 2), Act::Changed);
    assert_eq!(picker.at, 2);

    let single = questions("q2", &[question_spec("One?", "", false, &["x", "y"])]);
    let mut single_picker = picker_for(&single);

    assert_eq!(answer(single_picker.click(&single, 1))["answers"]["One?"], "y");
}
