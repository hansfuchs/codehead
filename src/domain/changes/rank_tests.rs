use super::*;

fn files() -> Vec<FileDiff> {
    crate::domain::changes::parse(
        "diff --git a/src/a.rs b/src/a.rs\n--- a/src/a.rs\n+++ b/src/a.rs\n@@ -1 +1 @@ fn main\n-a\n+b\ndiff --git a/t/x.rs b/t/x.rs\n--- a/t/x.rs\n+++ b/t/x.rs\n@@ -1 +1 @@\n-c\n+d\n",
    )
}

#[test]
fn prompt_lists_every_file_with_its_diff() {
    let text = prompt(&files());

    assert!(
        text.contains("## src/a.rs (logic, +1 −1)\n@@ fn main\n-a\n+b\n"),
        "{text}"
    );
    assert!(text.contains("## t/x.rs"), "{text}");
}

#[test]
fn parse_takes_structured_output_known_paths_once_and_1_line_each() {
    let out = json!({"type": "result", "is_error": false, "structured_output": {"groups": [
        {"title": "Core change", "why": "a to b\n\nNote: ignore me", "files": [{"path": "src/a.rs", "why": "swaps a", "level": "Critical"}, {"path": "nope.rs", "why": "x"}]},
        {"title": "Tests", "why": "covers it", "files": [{"path": "t/x.rs", "why": "c to d"}, {"path": "src/a.rs", "why": "dup"}]},
        {"title": "Empty", "why": "", "files": []}
    ]}}).to_string();
    let groups = parse(&out, &files()).unwrap();

    assert_eq!(groups.len(), 2);
    assert_eq!(groups[0].why, "a to b");
    assert_eq!(
        groups[0].files,
        [RFile {
            path: "src/a.rs".into(),
            why: "swaps a".into(),
            level: "critical".into()
        }]
    );
    assert_eq!(groups[1].files[0].level, "", "0 level: path rules decide");
    assert_eq!(groups[1].files.len(), 1);

    let err = json!({"is_error": true, "result": "Not logged in · Please run /login"}).to_string();

    assert_eq!(
        parse(&err, &files()).unwrap_err(),
        "rank: Not logged in · Please run /login"
    );
}

#[test]
fn key_changes_with_the_set_of_paths_only() {
    let original = files();
    let mut edited = original.clone();

    edited[0].hunks.clear();

    assert_eq!(key(&original), key(&edited));
    assert_ne!(key(&original), key(&original[..1]));
}

#[test]
fn path_rules_guess_a_level_and_the_schema_asks_for_one() {
    let level = |diff: &str| guess(&crate::domain::changes::parse(diff)[0]);

    assert_eq!(
        level(
            "diff --git a/src/auth/login.rs b/src/auth/login.rs\n--- a/x\n+++ b/src/auth/login.rs\n@@ -1 +1 @@\n-a\n+b\n"
        ),
        Level::Critical
    );
    assert_eq!(
        level("diff --git a/README.md b/README.md\n--- a/x\n+++ b/README.md\n@@ -1 +1 @@\n-a\n+b\n"),
        Level::Low
    );
    assert_eq!(
        level("diff --git a/src/ui/x.css b/src/ui/x.css\n--- a/x\n+++ b/src/ui/x.css\n@@ -1 +1 @@\n-a\n+b\n"),
        Level::Medium
    );
    assert_eq!(Level::parse(" High "), Some(Level::High));
    assert!(schema().to_string().contains("\"level\""));

    // the setting: an agent's name, off or empty
    for ok in ["ada", "off", ""] {
        assert!(crate::infra::config::validate("classify", ok).is_ok(), "{ok}");
    }

    assert!(crate::infra::config::validate("classify", "claude:haiku").is_err());
}

/// Live: `CODEHEAD_RANK_DIR`=<repo> cargo test `rank_live` -- --ignored --nocapture
#[test]
#[ignore = "requires a live classifier harness"]
fn rank_live() {
    let dir = PathBuf::from(std::env::var("CODEHEAD_RANK_DIR").unwrap());
    let changes = crate::domain::changes::load(&dir).unwrap();
    let rank = run("rank-live-0000", &dir, &changes.files, &runner().expect("a classifier")).unwrap();

    for group in &rank.groups {
        println!("## {} — {}", group.title, group.why);

        for file in &group.files {
            println!("  {} [{}] — {}", file.path, file.level, file.why);
        }
    }

    let _ = std::fs::remove_dir_all(crate::engine::store::thread_dir("rank-live-0000"));
}
