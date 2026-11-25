use super::*;

fn thread(name: &str) -> String {
    let id = format!("threads-{name}-{}", std::process::id());
    let _ = std::fs::remove_dir_all(crate::engine::store::thread_dir(&id));

    super::super::agent_comments(
        &id,
        "REV",
        None,
        &serde_json::json!([{"file": "a.rs", "line": 3, "severity": "high", "text": "off by 1"}]),
    )
    .unwrap();
    id
}

#[test]
fn replies_reopen_on_the_owner_and_stay_unseen_from_an_agent() {
    let id = thread("reply");

    super::super::set_comment_state(&id, "REV", &["c1".into()], "resolved", "fixed").unwrap();

    // the owner's reply: open again, in the next batch
    let character = reply(&id, "", "c1", "why 1? @REV", false).unwrap();

    assert!(character.open() && !character.sent && character.resolved_by.is_empty() && character.note.is_empty());
    assert!(waits_on_agent(&character));

    // the agent's reply: unseen until the owner looks
    let character_value = reply(&id, "REV", "c1", "the loop starts at 0", false).unwrap();

    assert_eq!(
        (
            character_value.replies.len(),
            unseen(&character_value),
            waits_on_agent(&character_value)
        ),
        (2, 1, false)
    );
    assert_eq!(
        unseen_replies(&id),
        vec![(
            "c1".to_string(),
            character_value.replies[1].at,
            "REV".to_string(),
            "c1 a.rs: the loop starts at 0".to_string()
        )]
    );
    assert!(mark_seen(&id, "c1") && !mark_seen(&id, "c1"));
    assert_eq!(
        unseen_replies(&id),
        [] as [(std::string::String, u64, std::string::String, std::string::String); 0]
    );

    assert!(reply(&id, "REV", "c9", "x", false).unwrap_err().contains("c9"));
    assert!(reply(&id, "REV", "c1", "  ", false).is_err());

    let _ = std::fs::remove_dir_all(crate::engine::store::thread_dir(&id));
}

#[test]
fn edits_change_or_drop_a_reply_or_the_comment() {
    let id = thread("edit");

    reply(&id, "", "c1", "a", false).unwrap();
    reply(&id, "REV", "c1", "b", false).unwrap();

    edit(&id, "c1", Some(1), "b2").unwrap();
    edit(&id, "c1", Some(0), "").unwrap();
    edit(&id, "c1", None, "off by 2").unwrap();
    let comment = &super::super::read_comments(&id)[0];

    assert_eq!(
        (
            comment.text.as_str(),
            comment.replies.len(),
            comment.replies[0].text.as_str()
        ),
        ("off by 2", 1, "b2")
    );

    assert!(edit(&id, "c1", Some(5), "x").is_err());

    edit(&id, "c1", None, "").unwrap();

    assert_eq!(
        super::super::read_comments(&id),
        [] as [crate::domain::changes::Comment; 0]
    );

    let _ = std::fs::remove_dir_all(crate::engine::store::thread_dir(&id));
}

#[test]
fn the_context_names_authors_without_at_and_the_tools() {
    let id = thread("ctx");

    reply(&id, "", "c1", "sure? @REV", false).unwrap();
    let comment = &super::super::read_comments(&id)[0];

    let text = context(comment);

    assert!(text.starts_with("[codehead: comment c1 on `a.rs:3`"), "{text}");
    assert!(
        text.contains("- REV: off by 1\n- owner: sure? @REV\n")
            && text.contains("`comment_reply`")
            && text.ends_with(']'),
        "{text}"
    );
    assert_eq!(
        mentioned("ask @REV and @LEAD, not @owner", &["REV".into(), "LEAD".into()], "rev"),
        vec!["LEAD".to_string()]
    );
    assert!(!names_agent("cc @owner, mail a@b") && names_agent("@LEAD look"));

    let _ = std::fs::remove_dir_all(crate::engine::store::thread_dir(&id));
}
