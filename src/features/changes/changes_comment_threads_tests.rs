use super::*;

#[test]
fn enter_on_a_comment_edits_it_0_text_deletes_it() {
    let mut value = view(parse(DIFF));

    value.id = "changes-edit-0000".into();
    value.comment_state.comments.clear();
    value.build(100);
    value.add_comment(
        &[Spot {
            path: "src/a.rs".into(),
            line: 11,
            lines: String::new(),
        }],
        "old",
    );

    assert_eq!(value.comment_state.comments[0].quote, "+ let b = 3;");

    value.at = value
        .layout
        .rows
        .iter()
        .position(|result| result.comment == Some(0))
        .unwrap();
    press(&mut value, KeyCode::Enter);

    let Some(Modal::Comment {
        target: Target::Edit(_),
        ..
    }) = &value.modal_state.modal
    else {
        panic!("edit modal")
    };

    value.key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));

    for character in "new".chars() {
        press(&mut value, KeyCode::Char(character));
    }

    value.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL));

    assert!(
        value.comment_state.comments[0].text.ends_with("new"),
        "{:?}",
        value.comment_state.comments
    );

    press(&mut value, KeyCode::Enter);
    value.key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL));

    if let Some(Modal::Comment { text, .. }) = value.modal_state.modal.as_mut() {
        text.set_text("");
    }

    value.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL));

    assert_eq!(value.comment_state.comments, [] as [crate::domain::changes::Comment; 0]);

    let _ = std::fs::remove_dir_all(crate::engine::store::thread_dir("changes-edit-0000"));
}

#[test]
fn comment_threads_unfold_reply_edit_fold_by_keys() {
    let (id, mut value) = prepare_comment_thread();

    let Some(Modal::Comment {
        target: Target::EditReply(cid, 0),
        text: ta,
        ..
    }) = &mut value.modal_state.modal
    else {
        panic!("reply edit modal")
    };

    assert_eq!(cid, "c1");

    ta.set_text("the spec says 3, page 2");
    value.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL));

    assert_eq!(read_comments(id)[0].replies[0].text, "the spec says 3, page 2");

    // c on the comment: a reply of the owner, under the agent's
    press(&mut value, KeyCode::Char('c'));

    let Some(Modal::Comment {
        target: Target::Reply(_),
        ..
    }) = &value.modal_state.modal
    else {
        panic!("reply modal")
    };

    for ch in "ok, keep it".chars() {
        press(&mut value, KeyCode::Char(ch));
    }

    assert_eq!(
        value.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL)),
        Act::Toast("reply on c1 · ctrl+enter: in the batch · @agent: at once".into())
    );

    let comment_value = &read_comments(id)[0];

    assert_eq!(
        (
            comment_value.replies.len(),
            comment_value.replies[1].by.as_str(),
            comment_value.replies[1].text.as_str(),
            comment_value.sent
        ),
        (2, "", "ok, keep it", false)
    );
    assert!(
        text(&value)
            .iter()
            .any(|line_4| line_4.contains("↳ you") && line_4.contains("ok, keep it"))
    );

    // d d on the owner's reply: it goes
    value.at = (0..value.layout.rows.len())
        .find(|&i| value.row_comment(i) == Some((0, Some(1))))
        .unwrap();
    press(&mut value, KeyCode::Char('d'));
    press(&mut value, KeyCode::Char('d'));

    assert_eq!(read_comments(id)[0].replies.len(), 1);

    // e off a comment: the file in the IDE
    value.at = value
        .layout
        .rows
        .iter()
        .position(|result_value| result_value.head)
        .unwrap();

    assert!(matches!(press(&mut value, KeyCode::Char('e')), Act::Open(..)));

    let _ = std::fs::remove_dir_all(crate::engine::store::thread_dir(id));
}

fn prepare_comment_thread() -> (&'static str, View) {
    let id = "changes-threads-keys-0000";
    let mut value = thread_view(id);

    value.add_comment(&spot11(), "why 3?");
    threads::reply(id, "REV", "c1", "the spec says 3\nsee docs", false).unwrap();
    resync(&mut value);

    // unfolded: the reply under the comment, its unseen dot
    let thread = text(&value);
    let at = thread.iter().position(|line| line.contains("✎ c1")).unwrap();

    assert!(
        thread[at + 1].contains("↳ ● @REV")
            && thread[at + 1].contains("the spec says 3")
            && thread[at + 2].contains("see docs"),
        "{thread:#?}"
    );

    // C on the comment: 1 row, the reply count, the unseen count
    value.at = value
        .layout
        .rows
        .iter()
        .position(|result| result.comment == Some(0))
        .unwrap();
    value.key(KeyEvent::new(KeyCode::Char('C'), KeyModifiers::SHIFT));
    let thread_value = text(&value);
    let at = thread_value
        .iter()
        .position(|line_value| line_value.contains("✎ c1"))
        .unwrap();

    assert!(
        thread_value[at].contains("(1 reply)")
            && thread_value[at].contains("● 1 new")
            && !thread_value.iter().any(|line_2| line_2.contains("the spec says 3")),
        "{thread_value:#?}"
    );

    // enter: the whole thread, the cursor on its head; ↓: the reply's row
    press(&mut value, KeyCode::Enter);

    assert!(text(&value).iter().any(|line_3| line_3.contains("the spec says 3")));
    assert_eq!(value.row_comment(value.at), Some((0, None)));

    press(&mut value, KeyCode::Down);

    assert_eq!(value.row_comment(value.at), Some((0, Some(0))));

    // enter on the reply: its edit modal; ctrl+enter saves it
    press(&mut value, KeyCode::Enter);

    (id, value)
}

#[test]
fn an_owner_mention_sends_the_comment_at_once_and_skips_the_batch() {
    let id = "changes-mention-0000";
    let mut value = thread_view(id);

    value.members = vec!["REV".into(), "LEAD".into()];
    value.add_comment(&spot11(), "why 3?");
    value.at = value
        .layout
        .rows
        .iter()
        .position(|result| result.comment == Some(0))
        .unwrap();

    press(&mut value, KeyCode::Char('c'));

    if let Some(Modal::Comment { text, .. }) = value.modal_state.modal.as_mut() {
        text.set_text("@REV is 3 right?");
    }

    let Act::Mention { to, text, context } = value.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL)) else {
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
            value.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL)),
            Act::Toast(_)
        ),
        "0 new comments"
    );

    // a new comment that names an agent: to it at once too
    value.open_comment(spot11());

    if let Some(Modal::Comment { text, .. }) = value.modal_state.modal.as_mut() {
        text.set_text("@LEAD take a look");
    }

    let Act::Mention { to, .. } = value.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL)) else {
        panic!("mention")
    };

    assert_eq!(to, vec!["LEAD".to_string()]);

    let _ = std::fs::remove_dir_all(crate::engine::store::thread_dir(id));
}

#[test]
fn a_comment_on_the_1st_changed_line_sits_under_it_not_under_the_why() {
    use crate::domain::changes::rank::{Group, RFile, Rank};
    let mut value = thread_view("changes-why-0000");
    let key = crate::domain::changes::rank::key(&value.changes.files);

    value.classifier.rank = Some(Rank {
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
    value.apply_rank(false);
    value.add_comment(&spot11(), "why 3?");
    value.layout.built = None;
    value.build(100);

    let thread = text(&value);
    let at = thread.iter().position(|line| line.contains("+ let b = 3;")).unwrap();

    assert!(thread[at + 1].contains("✎ c1"), "under its line: {thread:#?}");

    let _ = std::fs::remove_dir_all(crate::engine::store::thread_dir("changes-why-0000"));
}
