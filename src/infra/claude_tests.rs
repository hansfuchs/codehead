use super::*;

/// 2 copies of 1 session (a resume in another dir): the newer one, not the 1st dir by name.
#[test]
fn session_file_takes_the_newest_copy() {
    let root = std::env::temp_dir().join(format!("codehead-session-file-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let (old, new) = (root.join("-a-codehead-x"), root.join("-b-platform-x"));

    std::fs::create_dir_all(&old).unwrap();
    std::fs::create_dir_all(&new).unwrap();
    std::fs::write(new.join("s1.jsonl"), "{}\n").unwrap();
    std::fs::write(old.join("s1.jsonl"), "{}\n").unwrap();
    let past = std::fs::File::options().write(true).open(old.join("s1.jsonl")).unwrap();

    past.set_modified(SystemTime::now() - std::time::Duration::from_secs(3600))
        .unwrap();

    assert_eq!(session_file(&root, "s1"), Some(new.join("s1.jsonl")));
    assert_eq!(session_file(&root, "s2"), None);

    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn last_response_after_last_tool_result() {
    let thread = [
        r#"{"type":"user","cwd":"/p","gitBranch":"main","message":{"content":"do it"}}"#,
        r#"{"type":"assistant","message":{"content":[{"type":"text","text":"checking"},{"type":"tool_use"}]}}"#,
        r#"{"type":"user","message":{"content":[{"type":"tool_result"}]}}"#,
        r#"{"type":"assistant","message":{"content":[{"type":"text","text":"DONE. part 1"}]}}"#,
        r#"{"type":"assistant","message":{"content":[{"type":"text","text":"part 2"}]}}"#,
        r#"{"type":"ai-title","aiTitle":"Fix it"}"#,
    ]
    .join("\n");
    let mut text = Session::default();

    parse(&thread, &mut text);

    assert_eq!(text.last, "DONE. part 1\n\npart 2");
    assert_eq!(text.title, "Fix it");
    assert_eq!(text.cwd, "/p");
    assert_eq!(text.branch, "main");
}

/// Live check: `CODEHEAD_THREAD`=<id> cargo test `live_of_thread` -- --ignored --nocapture
#[test]
#[ignore = "requires CODEHEAD_THREAD with a live Claude session"]
fn live_of_thread() {
    let id = std::env::var("CODEHEAD_THREAD").unwrap();
    let text = of_thread(&id, "live").expect("0 responses");

    println!(
        "{}\n{}",
        text.path.display(),
        text.last.chars().take(300).collect::<String>()
    );
}
