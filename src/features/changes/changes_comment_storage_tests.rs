use super::*;

#[test]
fn an_older_comments_file_keeps_its_fields_on_a_write() {
    let id = "changes-old-file-0000";
    let _ = std::fs::remove_dir_all(crate::engine::store::thread_dir(id));

    std::fs::create_dir_all(crate::engine::store::thread_dir(id)).unwrap();
    let old = r#"[{"path":"a.rs","line":3,"text":"t","by":"REV","id":"c1","state":"resolved","resolved_by":"REV","note":"n","sent":true}]"#;

    std::fs::write(comments_path(id), old).unwrap();

    let comments = read_comments(id);

    write_comments(id, &comments);

    let back = std::fs::read_to_string(comments_path(id)).unwrap();

    assert_eq!(read_comments(id), comments);
    assert!(
        !back.contains("replies") && !back.contains("\"at\""),
        "0 new keys: {back}"
    );
    assert_eq!(
        (
            comments[0].state,
            comments[0].resolved_by.as_str(),
            comments[0].sent,
            comments[0].replies.len()
        ),
        (CState::Resolved, "REV", true, 0)
    );

    let _ = std::fs::remove_dir_all(crate::engine::store::thread_dir(id));
}

#[test]
fn a_save_keeps_comments_and_replies_agents_wrote_since_the_read() {
    let id = "changes-merge-0000";
    let mut view = thread_view(id);

    view.add_comment(&spot11(), "mine");
    agent_comments(
        id,
        "REV",
        None,
        &serde_json::json!([{"file": "src/a.rs", "line": 12, "severity": "low", "text": "theirs"}]),
    )
    .unwrap();
    threads::reply(id, "REV", "c1", "answer", false).unwrap();

    // the view resolves c1 from its list of before the agent's writes
    view.toggle_resolved(0);

    let all = read_comments(id);

    assert_eq!(all.len(), 2, "{all:#?}");
    assert!(
        !all[0].open() && all[0].replies.len() == 1 && all[1].text == "theirs",
        "{all:#?}"
    );
    assert_eq!(view.comment_state.comments, all, "the view has the merged list");

    let _ = std::fs::remove_dir_all(crate::engine::store::thread_dir(id));
}

#[test]
fn comment_writes_of_2_threads_all_land() {
    const ID: &str = "changes-lock-0000";
    let _ = std::fs::remove_dir_all(crate::engine::store::thread_dir(ID));

    let writers: Vec<_> = (0..2)
        .map(|writer| {
            std::thread::spawn(move || {
                write_comment_batch(ID, writer);
            })
        })
        .collect();

    for writer in writers {
        writer.join().unwrap();
    }

    let all = read_comments(ID);
    let ids: HashSet<&str> = all.iter().map(|comment| comment.id.as_str()).collect();

    assert_eq!((all.len(), ids.len()), (50, 50));

    let _ = std::fs::remove_dir_all(crate::engine::store::thread_dir(ID));
}

fn write_comment_batch(id: &str, writer: i32) {
    for i in 0..25 {
        agent_comments(
            id,
            &format!("A{writer}"),
            None,
            &serde_json::json!([{"file": "a.rs", "line": i, "severity": "low", "text": "x"}]),
        )
        .unwrap();
    }
}

#[test]
fn the_batch_carries_replies_and_names_authors_without_at() {
    let mut comment = Comment {
        path: "a.rs".into(),
        line: 3,
        text: "why?".into(),
        by: "REV".into(),
        id: "c1".into(),
        ..Default::default()
    };
    let plain = batch(&[&comment]).unwrap();

    assert!(
        plain.ends_with("- c1 [medium] `a.rs:3` by REV: why?") && !plain.contains("comment_reply"),
        "{plain}"
    );

    comment.replies.push(Reply {
        by: String::new(),
        at: 1,
        text: "@REV sure?\nline 2".into(),
        unseen: false,
    });
    let with_reply = batch(&[&comment]).unwrap();

    assert!(
        with_reply.ends_with("by REV: why?\n  ↳ you: @REV sure?\n    line 2")
            && with_reply.contains("`comment_reply` tool:\n"),
        "{with_reply}"
    );
}

#[test]
fn a_commit_leaves_open_a_comment_whose_last_owner_reply_names_an_agent() {
    let mut waits = Comment {
        path: "x.txt".into(),
        id: "c1".into(),
        ..Default::default()
    };

    waits.replies.push(Reply {
        text: "@REV why?".into(),
        ..Default::default()
    });
    let mut plain = Comment {
        path: "x.txt".into(),
        id: "c2".into(),
        ..Default::default()
    };

    plain.replies.push(Reply {
        text: "thanks".into(),
        ..Default::default()
    });
    let dir = std::env::temp_dir().join(format!("codehead-waits-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);

    std::fs::create_dir_all(&dir).unwrap();

    assert!(
        Command::new("git")
            .arg("-C")
            .arg(&dir)
            .args(["init", "-q"])
            .status()
            .unwrap()
            .success()
    );

    assert_eq!(committed(&[waits, plain], &dir), vec!["c2".to_string()]);

    let _ = std::fs::remove_dir_all(&dir);
}
