use super::*;

#[test]
fn initialize_and_list() {
    let initialized = handle(
        "t",
        "a",
        &json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-03-26"}}),
    )
    .unwrap();

    assert_eq!(initialized["result"]["protocolVersion"], "2025-03-26");

    let listed = handle("t", "a", &json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"})).unwrap();

    assert_eq!(listed["result"]["tools"].as_array().unwrap().len(), 13);
    assert!(
        handle(
            "t",
            "a",
            &json!({"jsonrpc": "2.0", "method": "notifications/initialized"})
        )
        .is_none()
    );
}

#[test]
fn comment_reply_adds_a_reply_and_lists_it() {
    let thread = format!("mcp-reply-{}", std::process::id());
    let _ = std::fs::remove_dir_all(super::super::store::thread_dir(&thread));

    call(
        &thread,
        "REV",
        "comment",
        &json!({"comments": [{"file": "a.rs", "line": 3, "severity": "low", "text": "nit"}]}),
    )
    .unwrap();

    assert_eq!(
        call(
            &thread,
            "REV",
            "comment_reply",
            &json!({"id": "c1", "text": "fixed in a.rs:4"})
        )
        .unwrap(),
        "reply added to c1"
    );
    assert!(
        call(&thread, "REV", "comment_reply", &json!({"id": "c9", "text": "x"}))
            .unwrap_err()
            .contains("c9")
    );

    let rows: Value =
        serde_json::from_str(&call(&thread, "LEAD", "comments", &json!({"state": "open"})).unwrap()).unwrap();

    assert_eq!(
        (
            rows[0]["replies"][0]["author"].as_str(),
            rows[0]["replies"][0]["text"].as_str(),
            rows[0]["last_by"].as_str()
        ),
        (Some("REV"), Some("fixed in a.rs:4"), Some("REV"))
    );

    let _ = std::fs::remove_dir_all(super::super::store::thread_dir(&thread));
}
