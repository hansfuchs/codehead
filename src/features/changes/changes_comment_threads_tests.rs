use super::*;

#[test]
fn enter_on_a_comment_edits_it_0_text_deletes_it() {
    let mut view = view(parse(DIFF));

    view.id = "changes-edit-0000".into();
    view.comment_state.comments.clear();
    view.build(100);
    view.add_comment(
        &[Spot {
            path: "src/a.rs".into(),
            line: 11,
            lines: String::new(),
        }],
        "old",
    );

    assert_eq!(view.comment_state.comments[0].quote, "+ let b = 3;");

    view.at = view.layout.rows.iter().position(|row| row.comment == Some(0)).unwrap();
    press(&mut view, KeyCode::Enter);

    let Some(Modal::Comment {
        target: Target::Edit(_),
        ..
    }) = &view.modal_state.modal
    else {
        panic!("edit modal")
    };

    view.key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));

    for typed in "new".chars() {
        press(&mut view, KeyCode::Char(typed));
    }

    view.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL));

    assert!(
        view.comment_state.comments[0].text.ends_with("new"),
        "{:?}",
        view.comment_state.comments
    );

    press(&mut view, KeyCode::Enter);
    view.key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL));

    if let Some(Modal::Comment { text, .. }) = view.modal_state.modal.as_mut() {
        text.set_text("");
    }

    view.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL));

    assert_eq!(view.comment_state.comments, [] as [crate::domain::changes::Comment; 0]);

    let _ = std::fs::remove_dir_all(crate::engine::store::thread_dir("changes-edit-0000"));
}

#[test]
fn comment_threads_unfold_reply_edit_fold_by_keys() {
    let (id, mut view) = prepare_comment_thread();

    let Some(Modal::Comment {
        target: Target::EditReply(cid, 0),
        text: area,
        ..
    }) = &mut view.modal_state.modal
    else {
        panic!("reply edit modal")
    };

    assert_eq!(cid, "c1");

    area.set_text("the spec says 3, page 2");
    view.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL));

    assert_eq!(read_comments(id)[0].replies[0].text, "the spec says 3, page 2");

    // c on the comment: a reply of the owner, under the agent's
    press(&mut view, KeyCode::Char('c'));

    let Some(Modal::Comment {
        target: Target::Reply(_),
        ..
    }) = &view.modal_state.modal
    else {
        panic!("reply modal")
    };

    for typed in "ok, keep it".chars() {
        press(&mut view, KeyCode::Char(typed));
    }

    assert_eq!(
        view.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL)),
        Act::Toast("reply on c1 · ctrl+enter: in the batch · @agent: at once".into())
    );

    let comment = &read_comments(id)[0];

    assert_eq!(
        (
            comment.replies.len(),
            comment.replies[1].by.as_str(),
            comment.replies[1].text.as_str(),
            comment.sent
        ),
        (2, "", "ok, keep it", false)
    );
    assert!(
        text(&view)
            .iter()
            .any(|line| line.contains("↳ you") && line.contains("ok, keep it"))
    );

    // d d on the owner's reply: it goes
    view.at = (0..view.layout.rows.len())
        .find(|&i| view.row_comment(i) == Some((0, Some(1))))
        .unwrap();
    press(&mut view, KeyCode::Char('d'));
    press(&mut view, KeyCode::Char('d'));

    assert_eq!(read_comments(id)[0].replies.len(), 1);

    // e off a comment: the file in the IDE
    view.at = view.layout.rows.iter().position(|row| row.head).unwrap();

    assert!(matches!(press(&mut view, KeyCode::Char('e')), Act::Open(..)));

    let _ = std::fs::remove_dir_all(crate::engine::store::thread_dir(id));
}

fn prepare_comment_thread() -> (&'static str, View) {
    let id = "changes-threads-keys-0000";
    let mut view = thread_view(id);

    view.add_comment(&spot11(), "why 3?");
    threads::reply(id, "REV", "c1", "the spec says 3\nsee docs", false).unwrap();
    resync(&mut view);

    // unfolded: the reply under the comment, its unseen dot
    let rows = text(&view);
    let at = rows.iter().position(|line| line.contains("✎ c1")).unwrap();

    assert!(
        rows[at + 1].contains("↳ ● @REV")
            && rows[at + 1].contains("the spec says 3")
            && rows[at + 2].contains("see docs"),
        "{rows:#?}"
    );

    // C on the comment: 1 row, the reply count, the unseen count
    view.at = view.layout.rows.iter().position(|row| row.comment == Some(0)).unwrap();
    view.key(KeyEvent::new(KeyCode::Char('C'), KeyModifiers::SHIFT));
    let folded = text(&view);
    let at = folded.iter().position(|line| line.contains("✎ c1")).unwrap();

    assert!(
        folded[at].contains("(1 reply)")
            && folded[at].contains("● 1 new")
            && !folded.iter().any(|line| line.contains("the spec says 3")),
        "{folded:#?}"
    );

    // enter: the whole thread, the cursor on its head; ↓: the reply's row
    press(&mut view, KeyCode::Enter);

    assert!(text(&view).iter().any(|line| line.contains("the spec says 3")));
    assert_eq!(view.row_comment(view.at), Some((0, None)));

    press(&mut view, KeyCode::Down);

    assert_eq!(view.row_comment(view.at), Some((0, Some(0))));

    // enter on the reply: its edit modal; ctrl+enter saves it
    press(&mut view, KeyCode::Enter);

    (id, view)
}

#[test]
fn an_owner_mention_sends_the_comment_at_once_and_skips_the_batch() {
    let id = "changes-mention-0000";
    let mut view = thread_view(id);

    view.members = vec!["REV".into(), "LEAD".into()];
    view.add_comment(&spot11(), "why 3?");
    view.at = view.layout.rows.iter().position(|row| row.comment == Some(0)).unwrap();

    press(&mut view, KeyCode::Char('c'));

    if let Some(Modal::Comment { text, .. }) = view.modal_state.modal.as_mut() {
        text.set_text("@REV is 3 right?");
    }

    let Act::Mention { to, text, context } = view.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL)) else {
        panic!("mention")
    };

    assert_eq!(
        (to, text.as_str()),
        (vec!["REV".to_string()], "on comment c1 `src/a.rs:11`: @REV is 3 right?")
    );
    assert!(
        context.contains("- owner: why 3?\n- owner: @REV is 3 right?\n") && context.contains("`comment_reply`"),
        "{context}"
    );
    assert!(read_comments(id)[0].sent, "delivered: 0 batch");
    assert!(
        matches!(
            view.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL)),
            Act::Toast(_)
        ),
        "0 new comments"
    );

    // a new comment that names an agent: to it at once too
    view.open_comment(spot11());

    if let Some(Modal::Comment { text, .. }) = view.modal_state.modal.as_mut() {
        text.set_text("@LEAD take a look");
    }

    let Act::Mention { to, .. } = view.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL)) else {
        panic!("mention")
    };

    assert_eq!(to, vec!["LEAD".to_string()]);

    let _ = std::fs::remove_dir_all(crate::engine::store::thread_dir(id));
}

#[test]
fn a_comment_on_the_1st_changed_line_sits_under_it_not_under_the_why() {
    use crate::domain::changes::rank::{Group, RFile, Rank};
    let mut view = thread_view("changes-why-0000");
    let key = crate::domain::changes::rank::key(&view.changes.files);

    view.classifier.rank = Some(Rank {
        key,
        model: "haiku".into(),
        groups: vec![Group {
            title: "Core".into(),
            why: "b becomes 3".into(),
            files: vec![RFile {
                path: "src/a.rs".into(),
                why: "changes b".into(),
                level: "high".into(),
            }],
        }],
    });
    view.apply_rank(false);
    view.add_comment(&spot11(), "why 3?");
    view.layout.built = None;
    view.build(100);

    let rows = text(&view);
    let at = rows.iter().position(|line| line.contains("+ let b = 3;")).unwrap();

    assert!(rows[at + 1].contains("✎ c1"), "under its line: {rows:#?}");

    let _ = std::fs::remove_dir_all(crate::engine::store::thread_dir("changes-why-0000"));
}
