use super::test_support::*;
pub(crate) use super::test_support::{pr, with};
use super::*;

#[test]
fn the_keys_walk_the_stops_not_the_lines() {
    let mut merge = with(pr());

    // the 1st key: the title
    key(&mut merge, KeyCode::Down);

    assert_eq!(focus(&merge), Some(Stop::Status));

    key(&mut merge, KeyCode::Down);

    // the labels row: its 1st stop; → along it
    assert_eq!(focus(&merge), Some(Stop::Label(0)));

    key(&mut merge, KeyCode::Right);

    assert_eq!(focus(&merge), Some(Stop::AddLabel));

    // ↓ leaves the row: update + merge share 1
    key(&mut merge, KeyCode::Down);

    assert_eq!(focus(&merge), Some(Stop::Update));

    key(&mut merge, KeyCode::Right);

    assert_eq!(focus(&merge), Some(Stop::Merge));

    key(&mut merge, KeyCode::Down);

    assert_eq!(focus(&merge), Some(Stop::Checks));

    // → shows the checks: failed + passed (skipped: 1 gray line), ← hides them
    key(&mut merge, KeyCode::Right);
    key(&mut merge, KeyCode::Down);

    assert_eq!(focus(&merge), Some(Stop::Check(0)));

    key(&mut merge, KeyCode::Down);

    assert_eq!(focus(&merge), Some(Stop::Check(1)));

    key(&mut merge, KeyCode::Down);

    assert_eq!(focus(&merge), Some(Stop::NewComment));

    key(&mut merge, KeyCode::Up);
    key(&mut merge, KeyCode::Left);

    assert_eq!(focus(&merge), Some(Stop::Checks));
    assert!(!merge.tab_ref().unwrap().checks_open);

    // up into a row: its 1st stop
    key(&mut merge, KeyCode::Up);

    assert_eq!(focus(&merge), Some(Stop::Update));
}

#[test]
fn enter_on_comments_edits_own_replies_to_line_threads_quotes_others() {
    let mut merge = with(pr());

    merge.tab().unwrap().focus = Some(Stop::Comment(0, 0));
    key(&mut merge, KeyCode::Enter);

    // someone's flat comment: a new comment that quotes it
    let tab = merge.tab_ref().unwrap();

    assert!(
        matches!(tab.input.edit, Some((Edit::Comment, ref textarea)) if textarea.text().starts_with("@rev wrote:\n> looks off"))
    );

    key(&mut merge, KeyCode::Esc);

    // a line thread: a real reply
    merge.tab().unwrap().focus = Some(Stop::Comment(1, 0));
    key(&mut merge, KeyCode::Enter);

    assert!(matches!(
        merge.tab_ref().unwrap().input.edit,
        Some((Edit::Reply(1, 2), _))
    ));

    key(&mut merge, KeyCode::Esc);

    // own reply: edit / delete
    merge.tab().unwrap().focus = Some(Stop::Comment(1, 1));
    key(&mut merge, KeyCode::Enter);

    assert!(matches!(
        merge.tab_ref().unwrap().input.menu,
        Some(Menu::Mine {
            thread: 1,
            comment: 1,
            ..
        })
    ));

    key(&mut merge, KeyCode::Enter);

    assert!(
        matches!(merge.tab_ref().unwrap().input.edit, Some((Edit::Change(1, 1, Kind::Line, 3), ref textarea)) if textarea.text() == "because")
    );
}

#[test]
fn merge_asks_twice_update_prompts_the_agent() {
    let mut merge = with(pr());

    merge.tab().unwrap().focus = Some(Stop::Merge);
    let act = key(&mut merge, KeyCode::Enter);

    assert!(
        matches!(act, Act::Toast(ref toast) if toast.contains("enter again: merge when ready (squash)")),
        "{act:?}"
    );

    // update: a 1st enter asks, a 2nd sends the prompt
    merge.tab().unwrap().focus = Some(Stop::Update);
    let act = key(&mut merge, KeyCode::Enter);

    assert!(
        matches!(act, Act::Toast(ref toast) if toast.contains("enter again: the agents update")),
        "{act:?}"
    );

    let act = key(&mut merge, KeyCode::Enter);

    assert!(
        matches!(act, Act::Prompt(ref prompt) if prompt.contains("Update the branch `feat/x`") && prompt.contains("0 conflicts expected")),
        "{act:?}"
    );

    let mut pr = pr();

    pr.mergeable = "CONFLICTING".into();
    let mut conflicted = with(pr);

    conflicted.tab().unwrap().focus = Some(Stop::Update);
    key(&mut conflicted, KeyCode::Enter);
    let act = key(&mut conflicted, KeyCode::Enter);

    assert!(
        matches!(act, Act::Prompt(ref prompt) if prompt.contains("resolve every conflict")),
        "{act:?}"
    );
}

#[test]
fn a_label_goes_on_a_2nd_enter_and_brackets_type_into_the_diff_filter() {
    let mut pr = pr();

    pr.labels = vec![gh::Label {
        name: "bug".into(),
        color: String::new(),
    }];
    let mut merge = with(pr);

    merge.tab().unwrap().focus = Some(Stop::Label(0));
    let act = key(&mut merge, KeyCode::Enter);

    assert!(
        matches!(act, Act::Toast(ref toast) if toast.contains("enter again: remove the label bug")),
        "{act:?}"
    );

    // another stop between: the arm goes
    merge.tab().unwrap().focus = Some(Stop::Update);
    key(&mut merge, KeyCode::Enter);
    merge.tab().unwrap().focus = Some(Stop::Label(0));
    let act = key(&mut merge, KeyCode::Enter);

    assert!(
        matches!(act, Act::Toast(ref toast) if toast.contains("enter again: remove the label")),
        "{act:?}"
    );

    // the PR diff's filter takes [ ] < > as text, 0 PR switch
    key(&mut merge, KeyCode::Char('>'));
    merge.tab().unwrap().focus = None;
    merge.key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL));

    for character in "[a]<>".chars() {
        key(&mut merge, KeyCode::Char(character));
    }

    assert_eq!(merge.tab_ref().unwrap().sub, Sub::Changes);
    assert!(merge.typing(), "the filter still takes keys");
}

#[test]
fn the_changes_view_reaches_the_bar_rows() {
    let mut merge = with(pr());
    let sub = |merge: &Merge| merge.tab_ref().unwrap().sub;

    key(&mut merge, KeyCode::Char('>'));

    assert_eq!((sub(&merge), focus(&merge)), (Sub::Changes, None));

    // ↑ on the diff's 1st row: the views row; ←→ there: overview / changes, the focus stays
    key(&mut merge, KeyCode::Up);

    assert_eq!(focus(&merge), Some(Stop::Subs));

    key(&mut merge, KeyCode::Left);

    assert_eq!((sub(&merge), focus(&merge)), (Sub::Overview, Some(Stop::Subs)));

    key(&mut merge, KeyCode::Right);

    assert_eq!((sub(&merge), focus(&merge)), (Sub::Changes, Some(Stop::Subs)));
    assert!(merge.hints().contains("←→ PR / view"), "{}", merge.hints());

    // ↑: the PR tabs; ↓ ↓: back in the diff
    key(&mut merge, KeyCode::Up);

    assert_eq!(focus(&merge), Some(Stop::Tabs));

    key(&mut merge, KeyCode::Down);
    key(&mut merge, KeyCode::Down);

    assert_eq!((sub(&merge), focus(&merge)), (Sub::Changes, None));

    // enter on the views row: into the diff too
    key(&mut merge, KeyCode::Up);
    key(&mut merge, KeyCode::Enter);

    assert_eq!(focus(&merge), None);
}

#[test]
fn the_title_edits_in_place() {
    let mut merge = with(pr());

    merge.tab().unwrap().focus = Some(Stop::Title);
    key(&mut merge, KeyCode::Enter);

    assert!(merge.typing());

    for character in " now".chars() {
        key(&mut merge, KeyCode::Char(character));
    }

    key(&mut merge, KeyCode::Enter);

    // saved: on screen at once (the gh call fails in tests: a note)
    assert_eq!(merge.tab_ref().unwrap().pr.as_ref().unwrap().title, "Add X now");
    assert!(!merge.typing());
}

#[test]
fn the_overview_draws() {
    let mut merge = with(pr());

    merge.tab().unwrap().checks_open = true;
    let text = draw(&mut merge, 100, 60);

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
    let mut merge = with(pr());

    draw(&mut merge, 100, 60);
    let plus = merge
        .layout
        .hits
        .iter()
        .find(|(_, stop)| *stop == Stop::AddLabel)
        .map(|(rect, _)| *rect)
        .unwrap();

    merge.mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: plus.x,
        row: plus.y,
        modifiers: KeyModifiers::NONE,
    });

    assert!(matches!(merge.tab_ref().unwrap().input.menu, Some(Menu::Labels { .. })));

    // the dropdown filters as it is typed
    merge.labels.insert(
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

    assert_eq!(merge.label_opts("").len(), 2, "the PR's own label: not offered");

    key(&mut merge, KeyCode::Char('b'));

    assert_eq!(
        merge.label_opts("b").first().map(|label| label.name.as_str()),
        Some("backend")
    );
}
