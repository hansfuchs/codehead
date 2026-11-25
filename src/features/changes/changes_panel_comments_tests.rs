use super::*;

#[test]
fn comments_split_into_open_and_resolved_space_picks_v_resolves() {
    let mut view = prepare_comment_groups();

    view.scm.at = view.scm.rows.iter().position(|row| *row == PRow::Comment(0)).unwrap();
    view.key(KeyEvent::new(KeyCode::Char('v'), KeyModifiers::NONE));

    assert!(!view.comment_state.comments[0].open());

    let all = draw(&mut view, 140, 40).join("\n");

    assert!(all.contains("▾ COMMENTS  0") && !all.contains("▾ OPEN"), "{all}");

    view.scm.at = view.scm.rows.iter().position(|row| *row == PRow::Comment(0)).unwrap();
    view.key(KeyEvent::new(KeyCode::Char('v'), KeyModifiers::NONE));

    assert!(view.comment_state.comments[0].open());

    let _ = std::fs::remove_dir_all(crate::engine::store::thread_dir(&view.id));
}

fn prepare_comment_groups() -> View {
    let mut view = test_view();

    view.id = format!("changes-cstate-{}", std::process::id());
    let comment = |id: &str, by: &str, line: u32| crate::features::changes::Comment {
        path: "src/a.rs".into(),
        line,
        text: format!("{id} text"),
        id: id.into(),
        by: by.into(),
        ..Default::default()
    };

    view.comment_state.comments = vec![comment("c1", "", 11), comment("c2", "REGI", 12)];
    view.comment_state.comments[1].state = crate::features::changes::CState::Resolved;
    view.comment_state.comments[1].resolved_by = "REGI".into();

    let all = draw(&mut view, 140, 40).join("\n");

    assert!(
        all.contains("▾ COMMENTS  1") && all.contains("▾ OPEN") && all.contains("▾ RESOLVED"),
        "{all}"
    );
    assert!(all.contains("c1 text") && all.contains("you"), "author: you: {all}");
    assert!(
        all.contains("@REGI ✓ @REGI"),
        "resolved: author, who resolved it: {all}"
    );

    let review: String = all
        .lines()
        .map(|line| line.chars().skip(36).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n");

    assert!(
        review.contains("c1 text") && !review.contains("c2 text"),
        "resolved: out of the file view: {review}"
    );

    // the panel: space picks c1 for the prompt, v resolves it, v again opens it
    alt_tap(&mut view);
    view.scm.at = view.scm.rows.iter().position(|row| *row == PRow::Comment(0)).unwrap();
    view.key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE));

    assert!(view.scm.comment_picks.contains("c1") && view.context().unwrap().contains("c1 [medium, open]"));

    view
}

#[test]
fn comments_resolve_on_0_uncommitted_changes_and_list_per_checkout() {
    use crate::features::changes::commits::fixture;
    let dir = fixture::repo("comment-checkouts");
    let wt = dir.parent().unwrap().join("wt");

    fixture::sh(&dir, "git worktree add -q ../wt && echo dirty >> x.txt");
    fixture::sh(&wt, "echo dirty >> x.txt");
    let root = dir.to_string_lossy().into_owned();
    let comment = |id: &str, path: String, root: &str| crate::features::changes::Comment {
        path,
        line: 1,
        text: format!("{id} text"),
        id: id.into(),
        root: root.into(),
        ..Default::default()
    };
    let mut comments = vec![
        comment("c1", "x.txt".into(), &root),
        comment("c2", "y.txt".into(), &root),
        // an older comment: absolute path in the linked worktree, 1 relative one
        comment("c3", wt.join("x.txt").to_string_lossy().into_owned(), ""),
        comment("c4", wt.join("y.txt").to_string_lossy().into_owned(), ""),
        comment("c5", "c1.txt".into(), ""),
    ];

    // dirty x.txt (both checkouts): open; clean files: resolved (c5: in the work dir)
    let ids = crate::features::changes::committed(&comments, &dir);

    assert_eq!(ids, ["c2", "c4", "c5"]);
    assert!(crate::features::changes::resolve_committed(&mut comments, &ids));
    assert!(comments[1].resolved_by == "codehead" && !comments[1].open() && comments[0].open() && comments[2].open());

    // the view on the main checkout: c3 of the linked worktree is 1 row, enter goes there
    let mut view = test_view();

    view.dir = dir.clone();
    view.comment_state.comments = comments;
    let rows = view.panel_rows();

    assert!(
        rows.contains(&PRow::CommentFile("x.txt".into(), 1)) && rows.contains(&PRow::Elsewhere(wt.clone(), 1)),
        "{rows:?}"
    );
    assert!(
        !rows
            .iter()
            .any(|row| matches!(row, PRow::CommentFile(path, _) if path.contains("wt"))),
        "{rows:?}"
    );

    let all = draw(&mut view, 140, 40).join("\n");

    assert!(all.contains("▾ COMMENTS  1") && all.contains("↗ 1 open in wt"), "{all}");

    alt_tap(&mut view);
    view.scm.at = view
        .scm
        .rows
        .iter()
        .position(|row| matches!(row, PRow::Elsewhere(..)))
        .unwrap();

    assert!(matches!(view.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)), Act::Checkout(root) if root == wt));

    fixture::drop(&dir);
}

#[test]
fn agents_list_resolve_and_reopen_comments_by_id() {
    let id = format!("changes-agent-cstate-{}", std::process::id());
    let ids = crate::features::changes::agent_comments(&id, "REV", None, &serde_json::json!([{"file": "a.rs", "line": 3, "severity": "high", "text": "off by 1"}, {"file": "b.rs", "line": 9, "severity": "low", "text": "nit"}])).unwrap();

    assert_eq!(ids, vec!["c1", "c2"]);

    let open: serde_json::Value = serde_json::from_str(&crate::features::changes::list_comments(&id, "open")).unwrap();

    assert_eq!(open.as_array().unwrap().len(), 2);
    assert_eq!(
        (
            open[0]["id"].as_str(),
            open[0]["author"].as_str(),
            open[0]["state"].as_str()
        ),
        (Some("c1"), Some("REV"), Some("open"))
    );

    crate::features::changes::set_comment_state(&id, "LEAD", &["c1".into()], "resolved", "fixed in a.rs:3").unwrap();
    let comments = crate::features::changes::read_comments(&id);

    assert_eq!(
        (
            comments[0].state,
            comments[0].resolved_by.as_str(),
            comments[0].note.as_str()
        ),
        (crate::features::changes::CState::Resolved, "LEAD", "fixed in a.rs:3")
    );
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&crate::features::changes::list_comments(&id, "open"))
            .unwrap()
            .as_array()
            .unwrap()
            .len(),
        1
    );

    // the reviewer re-opens it; an unknown id: an error, 0 change
    crate::features::changes::set_comment_state(&id, "REV", &["c1".into()], "open", "still off").unwrap();

    assert!(crate::features::changes::read_comments(&id)[0].open());
    assert!(
        crate::features::changes::set_comment_state(&id, "REV", &["c9".into()], "resolved", "x")
            .unwrap_err()
            .contains("c9")
    );

    let _ = std::fs::remove_dir_all(crate::engine::store::thread_dir(&id));
}

#[test]
fn an_older_comments_file_gets_ids_on_read() {
    let id = format!("changes-ids-{}", std::process::id());

    std::fs::create_dir_all(crate::engine::store::thread_dir(&id)).unwrap();
    std::fs::write(
        crate::engine::store::thread_dir(&id).join("comments.json"),
        r#"[{"path": "a.rs", "line": 1, "text": "old"}]"#,
    )
    .unwrap();

    let comments = crate::features::changes::read_comments(&id);

    assert_eq!((comments[0].id.as_str(), comments[0].open()), ("c1", true));
    assert!(
        std::fs::read_to_string(crate::engine::store::thread_dir(&id).join("comments.json"))
            .unwrap()
            .contains("\"c1\""),
        "written back"
    );

    let _ = std::fs::remove_dir_all(crate::engine::store::thread_dir(&id));
}

#[test]
fn changes_show_uncommitted_files_all_changes_shows_the_branch_with_comments() {
    use crate::features::changes::commits::fixture::*;
    let dir = repo("all-changes");

    sh(&dir, "echo draft > x.txt");
    let id = format!("commits-all-{}", std::process::id());
    let mut view = commit_view(&dir, &id);

    let all = draw(&mut view, 140, 40).join("\n");

    // CHANGES: the uncommitted x.txt only (c1-c4.txt: committed)
    assert!(all.contains("▾ CHANGES  1") && !all.contains("c3.txt"), "{all}");
    assert!(all.contains("uncommitted"), "{all}");

    // the "all changes" row: the branch against origin/main, comments on, git steps off
    let at = view.scm.rows.iter().position(|row| *row == PRow::All).unwrap();

    view.scm.at = at;
    press(&mut view, KeyCode::Enter);

    if let Some(peek) = view.peek.as_mut() {
        wait_read(peek);
    }

    let all = draw(&mut view, 140, 40).join("\n");
    let review: String = all
        .lines()
        .map(|line| line.chars().skip(36).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n");

    assert!(
        review.contains("all changes against origin/main") && review.contains("c3.txt") && review.contains("x.txt"),
        "{review}"
    );
    assert!(matches!(press(&mut view, KeyCode::Char('s')), Act::Toast(toast) if toast.contains("all changes")));
    assert!(view.hints().contains("c: comment"));

    press(&mut view, KeyCode::Char('c'));

    assert!(
        view.peek.as_ref().is_some_and(|peek| peek.modal_state.modal.is_some()),
        "the comment modal"
    );

    press(&mut view, KeyCode::Esc);
    press(&mut view, KeyCode::Esc);

    assert!(view.peek.is_none());

    let _ = std::fs::remove_dir_all(crate::engine::store::thread_dir(&id));

    crate::domain::changes::commits::fixture::drop(&dir);
}
