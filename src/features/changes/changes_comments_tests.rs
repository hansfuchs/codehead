use super::*;

#[test]
fn comments_sit_under_their_line_and_go_out_as_1_batch() {
    let mut view = prepare_comment_batch();

    view.key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));

    assert_eq!(view.comment_count(), 2);

    view.key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));

    assert_eq!(view.comment_count(), 1);

    // ctrl+enter: the batch (id, severity, author, the tool); the comment stays open, sent
    match view.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL)) {
        Act::Send(batch) => assert!(
            batch.contains("`comment_status`")
                && batch.ends_with("- c1 [medium] `src/a.rs:11` by you: why 3?\n  keep 2"),
            "{batch}"
        ),
        other => panic!("{other:?}"),
    }

    assert_eq!(view.comment_count(), 1);
    assert!(view.comment_state.comments[0].sent);
    assert!(
        matches!(view.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL)), Act::Toast(toast) if toast.contains("0 new comments")),
        "sent once"
    );

    // v on the comment: resolved, it leaves the file view; v in the panel: open again
    view.at = view.layout.rows.iter().position(|row| row.comment == Some(0)).unwrap();
    view.key(KeyEvent::new(KeyCode::Char('v'), KeyModifiers::NONE));

    assert_eq!(view.comment_count(), 0);
    assert!(
        !text(&view).iter().any(|line| line.contains("why 3?")),
        "resolved: out of the file view"
    );

    let _ = std::fs::remove_dir_all(crate::engine::store::thread_dir("changes-comments-0000"));
}

fn prepare_comment_batch() -> View {
    let mut view = view(parse(DIFF));

    view.id = "changes-comments-0000".into();
    view.comment_state.comments.clear();
    view.build(100);

    let spot = |line: u32| {
        vec![Spot {
            path: "src/a.rs".into(),
            line,
            lines: String::new(),
        }]
    };

    view.add_comment(&spot(11), "why 3?\nkeep 2");
    view.add_comment(&spot(99), "outside any hunk");
    let rows = text(&view);
    let at = rows.iter().position(|line| line.contains("+ let b = 3;")).unwrap();

    assert!(
        rows[at + 1].contains("✎ c1 11 · you: why 3?") && rows[at + 2].contains("keep 2"),
        "id, line, author: {rows:#?}"
    );

    let end = rows.iter().position(|line| line.contains("outside any hunk")).unwrap();

    assert!(rows[end + 1].starts_with('└'), "orphan at the box end: {rows:#?}");

    // C: 1 row per comment, the hidden line count; C again: the whole text
    view.key(KeyEvent::new(KeyCode::Char('C'), KeyModifiers::SHIFT));
    let folded = text(&view);
    let at = folded.iter().position(|line| line.contains("✎ c1")).unwrap();

    assert!(
        folded[at].contains("(+1 lines)") && !folded[at + 1].contains("keep 2"),
        "{folded:#?}"
    );

    view.key(KeyEvent::new(KeyCode::Char('C'), KeyModifiers::SHIFT));

    assert!(text(&view).iter().any(|line| line.contains("keep 2")));

    // d d on a comment row deletes it (1 d: armed only)
    view.at = view.layout.rows.iter().position(|row| row.comment == Some(1)).unwrap();

    view
}

#[test]
fn findings_sit_under_their_line_c_takes_dd_drops() {
    use crate::features::changes::review::{Finding, Review};
    let mut view = view(parse(DIFF));

    view.id = "changes-findings-0000".into();
    view.comment_state.comments.clear();
    let finding = |line: u32, severity: &str, title: &str| Finding {
        file: "src/a.rs".into(),
        line,
        severity: severity.into(),
        title: title.into(),
        body: "what breaks".into(),
    };

    view.reviewer.review = Some(Review {
        model: "sonnet".into(),
        findings: vec![
            finding(12, "major", "c is never read"),
            finding(10, "minor", "a is shadowed"),
        ],
    });
    view.build(100);
    let rows = text(&view);
    let at = rows.iter().position(|line| line.contains("+ let c = 4;")).unwrap();

    assert!(
        rows[at + 1].contains("⚠ major · c is never read") && rows[at + 2].contains("what breaks"),
        "{rows:#?}"
    );
    assert!(summary(&view).contains("2 findings (sonnet)"), "{rows:#?}");

    // c: the finding becomes a comment
    view.at = view.layout.rows.iter().position(|row| row.finding == Some(0)).unwrap();
    view.key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE));

    assert_eq!(view.comment_count(), 1);
    assert_eq!(
        view.comment_state.comments[0].text,
        "[major] c is never read: what breaks"
    );
    assert_eq!(view.reviewer.review.as_ref().unwrap().findings.len(), 1);

    // d d: dismissed
    view.at = view.layout.rows.iter().position(|row| row.finding == Some(0)).unwrap();
    view.key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));
    view.key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));

    assert_eq!(
        view.reviewer.review.as_ref().unwrap().findings,
        [] as [crate::features::changes::review::Finding; 0]
    );
    assert_eq!(view.comment_count(), 1, "d d on a finding: 0 comment deleted");

    let _ = std::fs::remove_dir_all(crate::engine::store::thread_dir("changes-findings-0000"));
}

#[test]
fn severity_row_cycles_and_agent_comments_group_by_severity() {
    let mut view = view(parse(DIFF));

    view.id = "changes-sev-0000".into();
    let _ = std::fs::create_dir_all(crate::engine::store::thread_dir(&view.id));

    view.build(100);
    view.at = (0..view.layout.rows.len())
        .find(|&i| text_of(&view, i).contains("+ let c = 4;"))
        .unwrap();
    press(&mut view, KeyCode::Char('c'));

    // the severity row has the keys: ←→ cycle (medium → high → critical), a letter: the text
    press(&mut view, KeyCode::Left);
    press(&mut view, KeyCode::Left);
    press(&mut view, KeyCode::Char('x'));

    let Some(Modal::Comment {
        level, on_level, text, ..
    }) = &view.modal_state.modal
    else {
        panic!()
    };

    assert_eq!(
        (*level, *on_level, text.text().as_str()),
        (crate::domain::changes::rank::Level::Critical, false, "x")
    );

    view.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL));

    assert_eq!(
        view.comment_state.comments[0].level,
        crate::domain::changes::rank::Level::Critical
    );

    // an agent's comments (the MCP tool): into the file, the view reads them at its next draw
    let root = Path::new("/tmp");
    let ids = agent_comments(
        &view.id,
        "amon",
        Some(root),
        &serde_json::json!([{"file": "/tmp/src/a.rs", "line": 12, "severity": "low", "text": "nit"}]),
    )
    .unwrap();

    assert_eq!(ids, vec!["c2".to_string()], "ids after the owner's c1");

    let screen_text = screen(&mut view, 140, 40).join("\n");

    assert_eq!(view.comment_state.comments.len(), 2);
    assert_eq!(
        (
            view.comment_state.comments[1].path.as_str(),
            view.comment_state.comments[1].by.as_str()
        ),
        ("src/a.rs", "amon")
    );
    assert!(
        screen_text.contains("critical") && screen_text.contains("low"),
        "{screen_text}"
    );

    let _ = std::fs::remove_dir_all(crate::engine::store::thread_dir(&view.id));
}

#[test]
fn the_comment_modal_groups_lines_per_file_and_scrolls_them() {
    let mut view = view(parse(DIFF));

    view.id = "changes-modal-0000".into();
    view.build(100);
    select(&mut view, "+ let c = 4;");
    select(&mut view, "+ fn t() {}");
    press(&mut view, KeyCode::Char('c'));

    let Some(Modal::Comment { quote, .. }) = &view.modal_state.modal else {
        panic!()
    };

    assert_eq!(quote.iter().filter(|line| matches!(line, QLine::Path(_))).count(), 2);
    assert_eq!(quote.iter().filter(|line| **line == QLine::Gap).count(), 1);

    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 30)).unwrap();

    terminal.draw(|frame| view.draw(frame, frame.area())).unwrap();
    let buf = terminal.backend().buffer().clone();

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

    let (path_row, gap) = (row("src/a.rs:12"), row("tests/a_test.rs:2") - 1);

    assert_ne!(buf[(10, path_row)].bg, code_bg(), "the path: 0 code background");
    assert_eq!(buf[(10, path_row + 1)].bg, code_bg(), "the lines: the code background");
    assert_ne!(buf[(10, gap)].bg, code_bg(), "the gap between files: 0 background");

    // a short screen: the lines scroll (alt+↓), the box fits
    let long: Vec<QLine> = (0..40).map(|i| QLine::Code(format!("+ line {i}"))).collect();

    if let Some(Modal::Comment { quote, .. }) = view.modal_state.modal.as_mut() {
        *quote = long;
    }

    let mut short = ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 24)).unwrap();

    short.draw(|frame| view.draw(frame, frame.area())).unwrap();

    let shown = |terminal: &ratatui::Terminal<ratatui::backend::TestBackend>| {
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol().to_string())
            .collect::<String>()
    };

    assert!(shown(&short).contains("+ line 0") && !shown(&short).contains("+ line 39"));

    view.key(KeyEvent::new(KeyCode::Down, KeyModifiers::ALT));
    short.draw(|frame| view.draw(frame, frame.area())).unwrap();

    assert!(!shown(&short).contains("+ line 0 ") && shown(&short).contains("+ line 1"));

    let _ = std::fs::remove_dir_all(crate::engine::store::thread_dir("changes-modal-0000"));
}

#[test]
fn the_prompt_takes_the_selection_and_only_picked_comments_along_once() {
    let mut view = view(parse(DIFF));

    view.id = "changes-context-0000".into();
    view.comment_state.comments.clear();
    view.build(100);

    assert!(view.context().is_none());

    let i = (0..view.layout.rows.len())
        .find(|&i| text_of(&view, i).contains("+ let c = 4;"))
        .unwrap();

    view.select_row(i, true);
    view.comment_state.comments.push(Comment {
        path: "src/a.rs".into(),
        line: 11,
        text: "why 3?".into(),
        id: "c1".into(),
        ..Default::default()
    });
    view.comment_state.comments.push(Comment {
        path: "src/a.rs".into(),
        line: 12,
        text: "and 4?".into(),
        id: "c2".into(),
        by: "REGI".into(),
        ..Default::default()
    });
    view.layout.built = None;
    view.build(100);

    let context = view.context().unwrap();

    assert!(
        context.contains("- `src/a.rs:12`\n```diff\n+ let c = 4;\n```")
            && !context.contains("why 3?")
            && !context.contains("and 4?"),
        "0 unpicked comment: {context}"
    );

    // space on c2's row: picked; the prompt takes it, then the pick clears
    view.at = view.layout.rows.iter().position(|row| row.comment == Some(1)).unwrap();
    view.key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE));
    let picked = view.context().unwrap();

    assert!(
        picked.contains("- c2 [medium, open] `src/a.rs:12` by @REGI: and 4?") && !picked.contains("why 3?"),
        "{picked}"
    );

    view.took_context();

    assert!(!view.context().unwrap_or_default().contains("and 4?"), "1 prompt only");
}
