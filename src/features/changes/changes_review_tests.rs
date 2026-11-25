use super::*;

fn files() -> Vec<FileDiff> {
    crate::features::changes::parse(
        "diff --git a/src/a.rs b/src/a.rs\n--- a/src/a.rs\n+++ b/src/a.rs\n@@ -1,2 +1,2 @@ fn main\n a\n-b\n+c\n",
    )
}

#[test]
fn prompt_has_new_side_numbers() {
    let path = prompt(&files());

    assert!(
        path.contains("## src/a.rs\n@@ fn main\n   1| a\n    |-b\n   2|+c\n"),
        "{path}"
    );
}

#[test]
fn parse_keeps_known_files_critical_first() {
    let out = json!({"is_error": false, "structured_output": {"findings": [
        {"file": "src/a.rs", "line": 2, "severity": "minor", "title": "t1", "body": "b1"},
        {"file": "nope.rs", "line": 1, "severity": "critical", "title": "x", "body": "x"},
        {"file": "src/a.rs", "line": 0, "severity": "critical", "title": "t2\nnote", "body": "b2"}
    ]}})
    .to_string();
    let file = parse(&out, &files()).unwrap();

    assert_eq!(file.len(), 2);
    assert_eq!((file[0].title.as_str(), file[0].line), ("t2", 1));
    assert_eq!(as_comment(&file[1]), "[minor] t1: b1");
}
