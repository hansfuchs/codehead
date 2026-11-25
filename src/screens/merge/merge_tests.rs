use super::test_support::*;
pub(crate) use super::test_support::{pr, with};
use super::*;

#[test]
fn the_keys_walk_the_stops_not_the_lines() {
    let mut message = with(pr());

    // the 1st key: the title
    key(&mut message, KeyCode::Down);

    assert_eq!(focus(&message), Some(Stop::Status));

    key(&mut message, KeyCode::Down);

    // the labels row: its 1st stop; → along it
    assert_eq!(focus(&message), Some(Stop::Label(0)));

    key(&mut message, KeyCode::Right);

    assert_eq!(focus(&message), Some(Stop::AddLabel));

    // ↓ leaves the row: update + merge share 1
    key(&mut message, KeyCode::Down);

    assert_eq!(focus(&message), Some(Stop::Update));

    key(&mut message, KeyCode::Right);

    assert_eq!(focus(&message), Some(Stop::Merge));

    key(&mut message, KeyCode::Down);

    assert_eq!(focus(&message), Some(Stop::Checks));

    // → shows the checks: failed + passed (skipped: 1 gray line), ← hides them
    key(&mut message, KeyCode::Right);
    key(&mut message, KeyCode::Down);

    assert_eq!(focus(&message), Some(Stop::Check(0)));

    key(&mut message, KeyCode::Down);

    assert_eq!(focus(&message), Some(Stop::Check(1)));

    key(&mut message, KeyCode::Down);

    assert_eq!(focus(&message), Some(Stop::NewComment));

    key(&mut message, KeyCode::Up);
    key(&mut message, KeyCode::Left);

    assert_eq!(focus(&message), Some(Stop::Checks));
    assert!(!message.tab_ref().unwrap().checks_open);

    // up into a row: its 1st stop
    key(&mut message, KeyCode::Up);

    assert_eq!(focus(&message), Some(Stop::Update));
}

#[test]
fn enter_on_comments_edits_own_replies_to_line_threads_quotes_others() {
    let mut message = with(pr());

    message.tab().unwrap().focus = Some(Stop::Comment(0, 0));
    key(&mut message, KeyCode::Enter);

    // someone's flat comment: a new comment that quotes it
    let thread_value = message.tab_ref().unwrap();

    assert!(
        matches!(thread_value.input.edit, Some((Edit::Comment, ref ta)) if ta.text().starts_with("@rev wrote:\n> looks off"))
    );

    key(&mut message, KeyCode::Esc);

    // a line thread: a real reply
    message.tab().unwrap().focus = Some(Stop::Comment(1, 0));
    key(&mut message, KeyCode::Enter);

    assert!(matches!(
        message.tab_ref().unwrap().input.edit,
        Some((Edit::Reply(1, 2), _))
    ));

    key(&mut message, KeyCode::Esc);

    // own reply: edit / delete
    message.tab().unwrap().focus = Some(Stop::Comment(1, 1));
    key(&mut message, KeyCode::Enter);

    assert!(matches!(
        message.tab_ref().unwrap().input.menu,
        Some(Menu::Mine {
            thread: 1,
            comment: 1,
            ..
        })
    ));

    key(&mut message, KeyCode::Enter);

    assert!(
        matches!(message.tab_ref().unwrap().input.edit, Some((Edit::Change(1, 1, Kind::Line, 3), ref ta)) if ta.text() == "because")
    );
}

#[test]
fn merge_asks_twice_update_prompts_the_agent() {
    let mut message = with(pr());

    message.tab().unwrap().focus = Some(Stop::Merge);
    let agent = key(&mut message, KeyCode::Enter);

    assert!(
        matches!(agent, Act::Toast(ref thread) if thread.contains("enter again: merge when ready (squash)")),
        "{agent:?}"
    );

    // update: a 1st enter asks, a 2nd sends the prompt
    message.tab().unwrap().focus = Some(Stop::Update);
    let agent_value = key(&mut message, KeyCode::Enter);

    assert!(
        matches!(agent_value, Act::Toast(ref thread_value) if thread_value.contains("enter again: the agents update")),
        "{agent_value:?}"
    );

    let agent_2 = key(&mut message, KeyCode::Enter);

    assert!(
        matches!(agent_2, Act::Prompt(ref thread_2) if thread_2.contains("Update the branch `feat/x`") && thread_2.contains("0 conflicts expected")),
        "{agent_2:?}"
    );

    let mut path = pr();

    path.mergeable = "CONFLICTING".into();
    let mut message_value = with(path);

    message_value.tab().unwrap().focus = Some(Stop::Update);
    key(&mut message_value, KeyCode::Enter);
    let agent_3 = key(&mut message_value, KeyCode::Enter);

    assert!(
        matches!(agent_3, Act::Prompt(ref thread_3) if thread_3.contains("resolve every conflict")),
        "{agent_3:?}"
    );
}

#[test]
fn a_label_goes_on_a_2nd_enter_and_brackets_type_into_the_diff_filter() {
    let mut path = pr();

    path.labels = vec![gh::Label {
        name: "bug".into(),
        color: String::new(),
    }];
    let mut message = with(path);

    message.tab().unwrap().focus = Some(Stop::Label(0));
    let agent = key(&mut message, KeyCode::Enter);

    assert!(
        matches!(agent, Act::Toast(ref thread) if thread.contains("enter again: remove the label bug")),
        "{agent:?}"
    );

    // another stop between: the arm goes
    message.tab().unwrap().focus = Some(Stop::Update);
    key(&mut message, KeyCode::Enter);
    message.tab().unwrap().focus = Some(Stop::Label(0));
    let agent_value = key(&mut message, KeyCode::Enter);

    assert!(
        matches!(agent_value, Act::Toast(ref thread_value) if thread_value.contains("enter again: remove the label")),
        "{agent_value:?}"
    );

    // the PR diff's filter takes [ ] < > as text, 0 PR switch
    key(&mut message, KeyCode::Char('>'));
    message.tab().unwrap().focus = None;
    message.key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL));

    for character in "[a]<>".chars() {
        key(&mut message, KeyCode::Char(character));
    }

    assert_eq!(message.tab_ref().unwrap().sub, Sub::Changes);
    assert!(message.typing(), "the filter still takes keys");
}

#[test]
fn the_changes_view_reaches_the_bar_rows() {
    let mut message = with(pr());
    let sub = |message_value: &Merge| message_value.tab_ref().unwrap().sub;

    key(&mut message, KeyCode::Char('>'));

    assert_eq!((sub(&message), focus(&message)), (Sub::Changes, None));

    // ↑ on the diff's 1st row: the views row; ←→ there: overview / changes, the focus stays
    key(&mut message, KeyCode::Up);

    assert_eq!(focus(&message), Some(Stop::Subs));

    key(&mut message, KeyCode::Left);

    assert_eq!((sub(&message), focus(&message)), (Sub::Overview, Some(Stop::Subs)));

    key(&mut message, KeyCode::Right);

    assert_eq!((sub(&message), focus(&message)), (Sub::Changes, Some(Stop::Subs)));
    assert!(message.hints().contains("←→ PR / view"), "{}", message.hints());

    // ↑: the PR tabs; ↓ ↓: back in the diff
    key(&mut message, KeyCode::Up);

    assert_eq!(focus(&message), Some(Stop::Tabs));

    key(&mut message, KeyCode::Down);
    key(&mut message, KeyCode::Down);

    assert_eq!((sub(&message), focus(&message)), (Sub::Changes, None));

    // enter on the views row: into the diff too
    key(&mut message, KeyCode::Up);
    key(&mut message, KeyCode::Enter);

    assert_eq!(focus(&message), None);
}

#[test]
fn the_title_edits_in_place() {
    let mut message = with(pr());

    message.tab().unwrap().focus = Some(Stop::Title);
    key(&mut message, KeyCode::Enter);

    assert!(message.typing());

    for character in " now".chars() {
        key(&mut message, KeyCode::Char(character));
    }

    key(&mut message, KeyCode::Enter);

    // saved: on screen at once (the gh call fails in tests: a note)
    assert_eq!(message.tab_ref().unwrap().pr.as_ref().unwrap().title, "Add X now");
    assert!(!message.typing());
}

#[test]
fn the_overview_draws() {
    let mut message = with(pr());

    message.tab().unwrap().checks_open = true;
    let text = draw(&mut message, 100, 60);

    for want in [
        "platform#12",
        "overview",
        "changes",
        "Add X #12",
        "open ▾",
        "main ← feat/x",
        " bug × ",
        "+ label",
        "checks failed",
        "update branch",
        "merge when ready · squash",
        "1 failed",
        "1 passed",
        "1 skipped",
        "CI / lint",
        "required",
        "COMMENTS · 2",
        "+ write a comment",
        "rev ·",
        "↳ me (you)",
        "because",
        "DESCRIPTION",
        "the body",
    ] {
        assert!(text.contains(want), "{want} missing:\n{text}");
    }

    // the skipped check: only counted
    assert!(!text.contains("CD / deploy"));
}

#[test]
fn a_label_click_removes_the_plus_opens_the_dropdown() {
    let mut message = with(pr());

    draw(&mut message, 100, 60);
    let plus = message
        .layout
        .hits
        .iter()
        .find(|(_, text)| *text == Stop::AddLabel)
        .map(|(result, _)| *result)
        .unwrap();

    message.mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: plus.x,
        row: plus.y,
        modifiers: KeyModifiers::NONE,
    });

    assert!(matches!(
        message.tab_ref().unwrap().input.menu,
        Some(Menu::Labels { .. })
    ));

    // the dropdown filters as it is typed
    message.labels.insert(
        "acme/platform".into(),
        vec![
            gh::Label {
                name: "ui".into(),
                color: "00ff00".into(),
            },
            gh::Label {
                name: "backend".into(),
                color: "0000ff".into(),
            },
            gh::Label {
                name: "bug".into(),
                color: String::new(),
            },
        ],
    );

    assert_eq!(message.label_opts("").len(), 2, "the PR's own label: not offered");

    key(&mut message, KeyCode::Char('b'));

    assert_eq!(
        message.label_opts("b").first().map(|line| line.name.as_str()),
        Some("backend")
    );
}
