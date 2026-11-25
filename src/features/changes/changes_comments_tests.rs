use super::*;

#[test]
fn comments_sit_under_their_line_and_go_out_as_1_batch() {
    let mut value = prepare_comment_batch();

    value.key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));

    assert_eq!(value.comment_count(), 2);

    value.key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));

    assert_eq!(value.comment_count(), 1);

    // ctrl+enter: the batch (id, severity, author, the tool); the comment stays open, sent
    match value.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL)) {
        Act::Send(key_value) => assert!(
            key_value.contains("`comment_status`")
                && key_value.ends_with("- c1 [medium] `src/a.rs:11` by you: why 3?\n  keep 2"),
            "{key_value}"
        ),
        key_2 => panic!("{key_2:?}"),
    }

    assert_eq!(value.comment_count(), 1);
    assert!(value.comment_state.comments[0].sent);
    assert!(
        matches!(value.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL)), Act::Toast(thread_value) if thread_value.contains("0 new comments")),
        "sent once"
    );

    // v on the comment: resolved, it leaves the file view; v in the panel: open again
    value.at = value
        .layout
        .rows
        .iter()
        .position(|result_value| result_value.comment == Some(0))
        .unwrap();
    value.key(KeyEvent::new(KeyCode::Char('v'), KeyModifiers::NONE));

    assert_eq!(value.comment_count(), 0);
    assert!(
        !text(&value).iter().any(|line_5| line_5.contains("why 3?")),
        "resolved: out of the file view"
    );

    let _ = std::fs::remove_dir_all(crate::engine::store::thread_dir("changes-comments-0000"));
}

fn prepare_comment_batch() -> View {
    let mut value = view(parse(DIFF));

    value.id = "changes-comments-0000".into();
    value.comment_state.comments.clear();
    value.build(100);

    let spot = |line: u32| {
        vec![Spot {
            path: "src/a.rs".into(),
            line,
            lines: String::new(),
        }]
    };

    value.add_comment(&spot(11), "why 3?\nkeep 2");
    value.add_comment(&spot(99), "outside any hunk");
    let thread = text(&value);
    let at = thread
        .iter()
        .position(|line_value| line_value.contains("+ let b = 3;"))
        .unwrap();

    assert!(
        thread[at + 1].contains("✎ c1 11 · you: why 3?") && thread[at + 2].contains("keep 2"),
        "id, line, author: {thread:#?}"
    );

    let end = thread
        .iter()
        .position(|line_2| line_2.contains("outside any hunk"))
        .unwrap();

    assert!(thread[end + 1].starts_with('└'), "orphan at the box end: {thread:#?}");

    // C: 1 row per comment, the hidden line count; C again: the whole text
    value.key(KeyEvent::new(KeyCode::Char('C'), KeyModifiers::SHIFT));
    let thread_value = text(&value);
    let at = thread_value.iter().position(|line_3| line_3.contains("✎ c1")).unwrap();

    assert!(
        thread_value[at].contains("(+1 lines)") && !thread_value[at + 1].contains("keep 2"),
        "{thread_value:#?}"
    );

    value.key(KeyEvent::new(KeyCode::Char('C'), KeyModifiers::SHIFT));

    assert!(text(&value).iter().any(|line_4| line_4.contains("keep 2")));

    // d d on a comment row deletes it (1 d: armed only)
    value.at = value
        .layout
        .rows
        .iter()
        .position(|result| result.comment == Some(1))
        .unwrap();

    value
}

#[test]
fn findings_sit_under_their_line_c_takes_d_d_drops() {
    use crate::features::changes::review::{Finding, Review};
    let mut value = view(parse(DIFF));

    value.id = "changes-findings-0000".into();
    value.comment_state.comments.clear();
    let file_value = |line: u32, sev: &str, thread: &str| Finding {
        file: "src/a.rs".into(),
        line,
        severity: sev.into(),
        title: thread.into(),
        body: "what breaks".into(),
    };

    value.reviewer.review = Some(Review {
        model: "sonnet".into(),
        findings: vec![
            file_value(12, "major", "c is never read"),
            file_value(10, "minor", "a is shadowed"),
        ],
    });
    value.build(100);
    let thread_value = text(&value);
    let at = thread_value
        .iter()
        .position(|line_value| line_value.contains("+ let c = 4;"))
        .unwrap();

    assert!(
        thread_value[at + 1].contains("⚠ major · c is never read") && thread_value[at + 2].contains("what breaks"),
        "{thread_value:#?}"
    );
    assert!(summary(&value).contains("2 findings (sonnet)"), "{thread_value:#?}");

    // c: the finding becomes a comment
    value.at = value
        .layout
        .rows
        .iter()
        .position(|result| result.finding == Some(0))
        .unwrap();
    value.key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE));

    assert_eq!(value.comment_count(), 1);
    assert_eq!(
        value.comment_state.comments[0].text,
        "[major] c is never read: what breaks"
    );
    assert_eq!(value.reviewer.review.as_ref().unwrap().findings.len(), 1);

    // d d: dismissed
    value.at = value
        .layout
        .rows
        .iter()
        .position(|result_value| result_value.finding == Some(0))
        .unwrap();
    value.key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));
    value.key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));

    assert_eq!(
        value.reviewer.review.as_ref().unwrap().findings,
        [] as [crate::features::changes::review::Finding; 0]
    );
    assert_eq!(value.comment_count(), 1, "d d on a finding: 0 comment deleted");

    let _ = std::fs::remove_dir_all(crate::engine::store::thread_dir("changes-findings-0000"));
}

#[test]
fn severity_row_cycles_and_agent_comments_group_by_severity() {
    let mut value = view(parse(DIFF));

    value.id = "changes-sev-0000".into();
    let _ = std::fs::create_dir_all(crate::engine::store::thread_dir(&value.id));

    value.build(100);
    value.at = (0..value.layout.rows.len())
        .find(|&i| text_of(&value, i).contains("+ let c = 4;"))
        .unwrap();
    press(&mut value, KeyCode::Char('c'));

    // the severity row has the keys: ←→ cycle (medium → high → critical), a letter: the text
    press(&mut value, KeyCode::Left);
    press(&mut value, KeyCode::Left);
    press(&mut value, KeyCode::Char('x'));

    let Some(Modal::Comment {
        level, on_level, text, ..
    }) = &value.modal_state.modal
    else {
        panic!()
    };

    assert_eq!(
        (*level, *on_level, text.text().as_str()),
        (crate::domain::changes::rank::Level::Critical, false, "x")
    );

    value.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL));

    assert_eq!(
        value.comment_state.comments[0].level,
        crate::domain::changes::rank::Level::Critical
    );

    // an agent's comments (the MCP tool): into the file, the view reads them at its next draw
    let root = Path::new("/tmp");
    let count = agent_comments(
        &value.id,
        "amon",
        Some(root),
        &serde_json::json!([{"file": "/tmp/src/a.rs", "line": 12, "severity": "low", "text": "nit"}]),
    )
    .unwrap();

    assert_eq!(count, vec!["c2".to_string()], "ids after the owner's c1");

    let text_value = screen(&mut value, 140, 40).join("\n");

    assert_eq!(value.comment_state.comments.len(), 2);
    assert_eq!(
        (
            value.comment_state.comments[1].path.as_str(),
            value.comment_state.comments[1].by.as_str()
        ),
        ("src/a.rs", "amon")
    );
    assert!(
        text_value.contains("critical") && text_value.contains("low"),
        "{text_value}"
    );

    let _ = std::fs::remove_dir_all(crate::engine::store::thread_dir(&value.id));
}

#[test]
fn the_comment_modal_groups_lines_per_file_and_scrolls_them() {
    let mut value = view(parse(DIFF));

    value.id = "changes-modal-0000".into();
    value.build(100);
    select(&mut value, "+ let c = 4;");
    select(&mut value, "+ fn t() {}");
    press(&mut value, KeyCode::Char('c'));

    let Some(Modal::Comment { quote, .. }) = &value.modal_state.modal else {
        panic!()
    };

    assert_eq!(quote.iter().filter(|query| matches!(query, QLine::Path(_))).count(), 2);
    assert_eq!(
        quote.iter().filter(|query_value| **query_value == QLine::Gap).count(),
        1
    );

    let mut thread = ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 30)).unwrap();

    thread.draw(|file| value.draw(file, file.area())).unwrap();
    let buf = thread.backend().buffer().clone();

    // the title has the paths too: the rows after it
    let row = |pat: &str| {
        (0..30u16)
            .filter(|&y| {
                (0..100u16)
                    .map(|x| buf[(x, y)].symbol().to_string())
                    .collect::<String>()
                    .contains(pat)
            })
            .nth(1)
            .unwrap()
    };

    let (p1, gap) = (row("src/a.rs:12"), row("tests/a_test.rs:2") - 1);

    assert_ne!(buf[(10, p1)].bg, code_bg(), "the path: 0 code background");
    assert_eq!(buf[(10, p1 + 1)].bg, code_bg(), "the lines: the code background");
    assert_ne!(buf[(10, gap)].bg, code_bg(), "the gap between files: 0 background");

    // a short screen: the lines scroll (alt+↓), the box fits
    let long: Vec<QLine> = (0..40).map(|i| QLine::Code(format!("+ line {i}"))).collect();

    if let Some(Modal::Comment { quote, .. }) = value.modal_state.modal.as_mut() {
        *quote = long;
    }

    let mut thread_value = ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 24)).unwrap();

    thread_value
        .draw(|file_value| value.draw(file_value, file_value.area()))
        .unwrap();

    let shown = |thread_2: &ratatui::Terminal<ratatui::backend::TestBackend>| {
        thread_2
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|character| character.symbol().to_string())
            .collect::<String>()
    };

    assert!(shown(&thread_value).contains("+ line 0") && !shown(&thread_value).contains("+ line 39"));

    value.key(KeyEvent::new(KeyCode::Down, KeyModifiers::ALT));
    thread_value.draw(|file_2| value.draw(file_2, file_2.area())).unwrap();

    assert!(!shown(&thread_value).contains("+ line 0 ") && shown(&thread_value).contains("+ line 1"));

    let _ = std::fs::remove_dir_all(crate::engine::store::thread_dir("changes-modal-0000"));
}

#[test]
fn the_prompt_takes_the_selection_and_only_picked_comments_along_once() {
    let mut value = view(parse(DIFF));

    value.id = "changes-context-0000".into();
    value.comment_state.comments.clear();
    value.build(100);

    assert!(value.context().is_none());

    let i = (0..value.layout.rows.len())
        .find(|&i| text_of(&value, i).contains("+ let c = 4;"))
        .unwrap();

    value.select_row(i, true);
    value.comment_state.comments.push(Comment {
        path: "src/a.rs".into(),
        line: 11,
        text: "why 3?".into(),
        id: "c1".into(),
        ..Default::default()
    });
    value.comment_state.comments.push(Comment {
        path: "src/a.rs".into(),
        line: 12,
        text: "and 4?".into(),
        id: "c2".into(),
        by: "REGI".into(),
        ..Default::default()
    });
    value.layout.built = None;
    value.build(100);

    let character = value.context().unwrap();

    assert!(
        character.contains("- `src/a.rs:12`\n```diff\n+ let c = 4;\n```")
            && !character.contains("why 3?")
            && !character.contains("and 4?"),
        "0 unpicked comment: {character}"
    );

    // space on c2's row: picked; the prompt takes it, then the pick clears
    value.at = value
        .layout
        .rows
        .iter()
        .position(|result| result.comment == Some(1))
        .unwrap();
    value.key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE));
    let character_value = value.context().unwrap();

    assert!(
        character_value.contains("- c2 [medium, open] `src/a.rs:12` by @REGI: and 4?")
            && !character_value.contains("why 3?"),
        "{character_value}"
    );

    value.took_context();

    assert!(!value.context().unwrap_or_default().contains("and 4?"), "1 prompt only");
}
