use super::*;
use std::path::Path;

/// Live: `CODEHEAD_THREAD`=<id> cargo test `handoff_live` -- --ignored --nocapture
#[test]
#[ignore = "requires a live Claude harness"]
fn handoff_live() {
    let id = std::env::var("CODEHEAD_THREAD").unwrap();

    println!("{}", write(&id, "haiku").unwrap());
}

#[test]
fn tool_lines_are_short() {
    assert_eq!(
        tool_line("Edit", &serde_json::json!({"file_path": "src/x.rs", "old_string": "a"})),
        "Edit src/x.rs"
    );
    assert_eq!(
        tool_line("Bash", &serde_json::json!({"command": "cargo test\nmore"})),
        "Bash cargo test"
    );
    assert_eq!(tool_line("TodoWrite", &serde_json::json!({})), "TodoWrite");
}

#[test]
fn start_copies_the_place_and_leaves_the_draft_once() {
    let _env = crate::engine::ENV_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut old = Thread::new(
        "fix deploy",
        "task",
        Path::new("/tmp"),
        vec!["ada".into(), "bob".into()],
    );

    old.branch = Some("task/fix".into());
    old.port = Some(40020);
    old.sessions.insert("ada".into(), "s1".into());
    let thread = start(&old, "# Handoff\n\n## Goal\nship it").unwrap();

    assert_eq!(
        (
            thread.title.as_str(),
            thread.agents.len(),
            thread.port,
            thread.branch.as_deref()
        ),
        ("fix deploy (cont.)", 2, Some(40020), Some("task/fix"))
    );
    assert!(thread.sessions.is_empty() && thread.parent.as_deref() == Some(old.id.as_str()));
    assert!(take_draft(&thread.id).unwrap().starts_with("# Handoff"));
    assert!(take_draft(&thread.id).is_none(), "once");

    // a handoff of a handoff: 1 `(cont.)`
    assert_eq!(start(&thread, "x").unwrap().title, "fix deploy (cont.)");

    let _ = std::fs::remove_dir_all(store::thread_dir(&thread.id));
}
