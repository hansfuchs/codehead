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

fn picker(agent: &Ask) -> Picker {
    let mut path = Picker::default();

    path.sync(Some(agent));
    path
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

/// Keys of a space-separated list: ↑ ↓ ← → ⏎ ␣ ⌫ ⇥, else each char.
fn press(path: &mut Picker, agent: &Ask, keys: &str) -> Act {
    let mut last = Act::Pass;

    for thread_value in keys.split(' ').filter(|thread| !thread.is_empty()) {
        let codes: Vec<KeyCode> = match thread_value {
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

        for character in codes {
            last = path.key(agent, key(character));
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
    let mut path = picker(&color);

    assert!(path.focus);

    // 3 options + Other: 4 rows, round
    press(&mut path, &color, "↑");

    assert_eq!(path.at, 3);

    press(&mut path, &color, "↓ ↓ ↓");

    assert_eq!(path.at, 2);

    let value = answer(press(&mut path, &color, "⏎"));

    assert_eq!(value["answers"]["Color?"], "blue");
    assert_eq!(
        (value["id"].as_str(), value["allow"].as_bool(), value["label"].as_str()),
        (Some("q1"), Some(true), Some("blue"))
    );
}

#[test]
fn single_question_space_and_digits_answer_at_once() {
    let color = questions("q1", &[question_spec("Color?", "", false, &["red", "green"])]);
    let mut path = picker(&color);

    assert_eq!(answer(press(&mut path, &color, "↓ ␣"))["answers"]["Color?"], "green");

    let mut path_value = picker(&color);

    assert_eq!(answer(press(&mut path_value, &color, "1"))["answers"]["Color?"], "red");

    // a digit past the rows: 0 change; the Other row's digit: the cursor only
    let mut path_2 = picker(&color);

    assert_eq!(press(&mut path_2, &color, "9"), Act::Changed);
    assert_eq!(path_2.at, 0);
    assert_eq!(press(&mut path_2, &color, "3"), Act::Changed);
    assert_eq!(path_2.at, 2);
}

#[test]
fn multi_question_space_toggles_and_enter_sends_the_picks_in_option_order() {
    let agent = questions("q1", &[question_spec("Which?", "Keys", true, &["a", "b", "c", "d"])]);
    let mut path = picker(&agent);

    press(&mut path, &agent, "↓ ↓ ␣ ↑ ↑ ␣ ↓ ␣ ␣");
    let rows = path.rows(&agent);
    let picked: Vec<bool> = rows.iter().map(|result| result.picked).collect();

    assert_eq!(picked, vec![true, false, true, false, false]);
    assert_eq!(answer(press(&mut path, &agent, "⏎"))["answers"]["Which?"], "a, c");

    // digits toggle too, enter with 0 picks: the cursor row
    let mut path_value = picker(&agent);

    press(&mut path_value, &agent, "2 4 2");

    assert_eq!(answer(press(&mut path_value, &agent, "⏎"))["answers"]["Which?"], "d");

    let mut path_2 = picker(&agent);

    press(&mut path_2, &agent, "↓ ↓");

    assert_eq!(answer(press(&mut path_2, &agent, "⏎"))["answers"]["Which?"], "c");
}

#[test]
fn letters_type_into_other_and_enter_sends_the_text() {
    let agent = questions("q1", &[question_spec("Name?", "", false, &["ada", "bob"])]);
    let mut path = picker(&agent);

    press(&mut path, &agent, "Cl");

    assert_eq!(path.at, 2);

    press(&mut path, &agent, "x ⌫ a u d e ␣ 5");

    assert_eq!(path.rows(&agent)[2].label, "Claude 5");
    assert_eq!(answer(press(&mut path, &agent, "⏎"))["answers"]["Name?"], "Claude 5");

    // Other with 0 text: 0 answer; space on it: 0 text
    let mut path_value = picker(&agent);

    press(&mut path_value, &agent, "↑");

    assert_eq!(press(&mut path_value, &agent, "⏎"), Act::Changed);
    assert_eq!(press(&mut path_value, &agent, "␣"), Act::Changed);
    assert_eq!(path_value.rows(&agent)[2].label, "");
}

#[test]
fn multi_other_text_is_picked_and_comes_last() {
    let agent = questions("q1", &[question_spec("Which?", "", true, &["a", "b"])]);
    let mut path = picker(&agent);

    press(&mut path, &agent, "␣ zed");

    assert!(path.rows(&agent)[2].picked);
    assert_eq!(answer(press(&mut path, &agent, "⏎"))["answers"]["Which?"], "a, zed");

    // backspace to 0 text: Other unpicked
    let mut path_value = picker(&agent);

    press(&mut path_value, &agent, "z ⌫");

    assert!(!path_value.rows(&agent)[2].picked);

    press(&mut path_value, &agent, "↑ ↑ ␣");

    assert_eq!(answer(press(&mut path_value, &agent, "⏎"))["answers"]["Which?"], "a");
}

#[test]
fn several_questions_go_in_order_back_and_forth_then_send_all() {
    let agent = questions(
        "q1",
        &[
            question_spec("One?", "First", false, &["x", "y"]),
            question_spec("Two?", "Second", true, &["m", "n", "o"]),
            question_spec("Three?", "Third", false, &["p", "q"]),
        ],
    );
    let mut path = picker(&agent);

    // → before an answer: 0 move
    press(&mut path, &agent, "→");

    assert_eq!(path.question, 0);
    assert_eq!(press(&mut path, &agent, "↓ ⏎"), Act::Changed);
    assert_eq!((path.question, path.at), (1, 0));

    press(&mut path, &agent, "␣ ↓ ↓ ␣ ⏎");

    assert_eq!(path.question, 2);

    // back to the 1st: the cursor on its answer; another answer, then the open 3rd
    press(&mut path, &agent, "← ←");

    assert_eq!((path.question, path.at), (0, 1));
    assert_eq!(
        path.answered(&agent),
        vec![("One?".into(), "y".into()), ("Two?".into(), "m, o".into())]
    );

    press(&mut path, &agent, "↑ ⏎");

    assert_eq!(path.question, 2);

    let value = answer(press(&mut path, &agent, "2"));

    assert_eq!(value["answers"], json!({"One?": "x", "Two?": "m, o", "Three?": "q"}));
    assert_eq!(value["label"], "x · m, o · q");
}

#[test]
fn going_back_to_an_answered_question_and_enter_sends_when_all_are_answered() {
    let agent = questions(
        "q1",
        &[
            question_spec("One?", "", false, &["x", "y"]),
            question_spec("Two?", "", false, &["m", "n"]),
        ],
    );
    let mut path = picker(&agent);

    press(&mut path, &agent, "⏎");
    press(&mut path, &agent, "←");

    assert_eq!(path.question, 0);

    // the 2nd still open: enter goes there
    assert_eq!(press(&mut path, &agent, "↓ ⏎"), Act::Changed);
    assert_eq!(path.question, 1);

    let value = answer(press(&mut path, &agent, "⏎"));

    assert_eq!(value["answers"], json!({"One?": "y", "Two?": "m"}));
}

#[test]
fn without_the_focus_the_prompt_gets_every_key() {
    let color = questions("q1", &[question_spec("Color?", "", true, &["red", "green"])]);
    let mut path = picker(&color);

    // tab: the view's focus ring moves the focus
    path.focus = false;

    for character in [
        KeyCode::Down,
        KeyCode::Char(' '),
        KeyCode::Enter,
        KeyCode::Char('1'),
        KeyCode::Char('x'),
    ] {
        assert_eq!(path.key(&color, key(character)), Act::Pass, "{character:?}");
    }

    assert!(path.rows(&color).iter().all(|result| !result.picked));
    assert_eq!(path.hint(&color), "tab: answer the question");
}

#[test]
fn with_the_focus_the_view_keeps_esc_scroll_menu_and_ctrl_keys() {
    let color = questions("q1", &[question_spec("Color?", "", false, &["red", "green"])]);
    let mut path = picker(&color);

    for key_value in [
        key(KeyCode::Esc),
        key(KeyCode::PageUp),
        key(KeyCode::PageDown),
        key(KeyCode::F(2)),
        KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
        KeyEvent::new(KeyCode::Up, KeyModifiers::SHIFT),
        KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL),
    ] {
        assert_eq!(path.key(&color, key_value), Act::Pass, "{key_value:?}");
    }

    // plain keys never reach the hidden prompt
    for character in [
        KeyCode::Left,
        KeyCode::Right,
        KeyCode::Backspace,
        KeyCode::Delete,
        KeyCode::Home,
    ] {
        assert_eq!(path.key(&color, key(character)), Act::Changed, "{character:?}");
    }

    // shift + letter: a capital in Other
    assert_eq!(
        path.key(&color, KeyEvent::new(KeyCode::Char('Z'), KeyModifiers::SHIFT)),
        Act::Changed
    );
    assert_eq!(path.rows(&color)[2].label, "Z");
}

#[test]
fn permission_requests_answer_allow_or_deny() {
    let agent = permission();
    let mut path = picker(&agent);

    assert_eq!(path.rows(&agent).len(), 2);

    let value = answer(press(&mut path, &agent, "⏎"));

    assert_eq!(
        (value["allow"].as_bool(), value["label"].as_str()),
        (Some(true), Some("allow"))
    );

    let mut path_value = picker(&agent);

    assert_eq!(answer(press(&mut path_value, &agent, "↓ ⏎"))["allow"], false);

    let mut path_2 = picker(&agent);

    assert_eq!(answer(press(&mut path_2, &agent, "2"))["label"], "deny");

    // letters: 0 Other row, 0 answer
    let mut path_3 = picker(&agent);

    assert_eq!(press(&mut path_3, &agent, "y"), Act::Changed);

    let mut path_4 = picker(&agent);

    assert_eq!(answer(press(&mut path_4, &agent, "↓ ␣"))["allow"], false);
}

#[test]
fn acp_options_answer_with_their_option_id() {
    let agent_value = Ask {
        id: "o1".into(),
        agent: "ada".into(),
        tool: "Edit".into(),
        input: json!({"_options": [{"id": "ok1", "name": "Allow once", "kind": "allow_once"}, {"id": "ok2", "name": "Allow always", "kind": "allow_always"}, {"id": "no", "name": "Reject", "kind": "reject_once"}]}),
        ts: 1,
    };
    let mut path = picker(&agent_value);

    assert_eq!(
        path.rows(&agent_value)
            .iter()
            .map(|result| result.label.as_str())
            .collect::<Vec<_>>(),
        vec!["Allow once", "Allow always", "Reject"]
    );

    let value = answer(press(&mut path, &agent_value, "↑ ⏎"));

    assert_eq!(
        (value["option"].as_str(), value["allow"].as_bool()),
        (Some("no"), Some(false))
    );

    let mut path_value = picker(&agent_value);
    let value_value = answer(press(&mut path_value, &agent_value, "2"));

    assert_eq!(
        (value_value["option"].as_str(), value_value["allow"].as_bool()),
        (Some("ok2"), Some(true))
    );
}

#[test]
fn sync_keeps_the_state_of_the_same_ask_and_resets_on_a_new_one() {
    let agent = questions("q1", &[question_spec("Which?", "", true, &["a", "b"])]);
    let mut path = picker(&agent);

    press(&mut path, &agent, "↓ ␣");
    path.focus = false;
    path.sync(Some(&agent));

    assert_eq!((path.at, path.focus), (1, false));
    assert!(path.rows(&agent)[1].picked);

    let bytes = questions("q2", &[question_spec("Next?", "", false, &["x"])]);

    path.sync(Some(&bytes));

    assert_eq!((path.at, path.focus, path.question), (0, true, 0));
    assert!(path.rows(&bytes).iter().all(|result| !result.picked));

    path.sync(None);

    assert_eq!(path.rows_n(&bytes), 2);
    assert!(!path.focus);
}

#[test]
fn a_sent_answer_takes_0_keys() {
    let color = questions("q1", &[question_spec("Color?", "", false, &["red"])]);
    let mut path = picker(&color);

    assert!(matches!(press(&mut path, &color, "⏎"), Act::Answer(_)));

    path.sent = true;

    assert_eq!(press(&mut path, &color, "↓"), Act::Pass);
    assert_eq!(path.hint(&color), "answer sent…");
}

#[test]
fn clicks_toggle_multi_rows_answer_single_ones_and_focus_the_ask() {
    let agent = questions("q1", &[question_spec("Which?", "", true, &["a", "b"])]);
    let mut path = picker(&agent);

    path.focus = false;

    assert_eq!(path.click(&agent, 1), Act::Changed);
    assert!(path.focus && path.rows(&agent)[1].picked);
    assert_eq!(path.click(&agent, 2), Act::Changed);
    assert_eq!(path.at, 2);

    let text = questions("q2", &[question_spec("One?", "", false, &["x", "y"])]);
    let mut path_value = picker(&text);

    assert_eq!(answer(path_value.click(&text, 1))["answers"]["One?"], "y");
}
