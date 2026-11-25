use super::*;

fn sh(dir: &Path, cmd: &str) {
    let ok = Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .current_dir(dir)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap()
        .success();

    assert!(ok, "{cmd}");
}

fn repo(name: &str) -> PathBuf {
    let directory = std::env::temp_dir().join(format!("codehead-edits-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&directory);

    std::fs::create_dir_all(&directory).unwrap();
    sh(
        &directory,
        "git init -q && git config user.email t@t && git config user.name t && printf 'a\\nb\\nc\\nd\\ne\\n' > x.txt && echo .env > .gitignore && git add -A && git commit -qm init",
    );
    directory
}

#[test]
fn tree_leaves_the_index_alone_and_sees_untracked_files() {
    let directory = repo("tree");
    let t0 = tree(&directory).unwrap();

    std::fs::write(directory.join("new.txt"), "hi\n").unwrap();
    std::fs::write(directory.join(".env"), "SECRET=1\n").unwrap();
    let t1 = tree(&directory).unwrap();

    assert_ne!(t0, t1);

    // 0 staged files: the real index did not change
    assert_eq!(
        git(&directory, &["diff", "--cached", "--name-only"], None)
            .unwrap()
            .trim(),
        ""
    );

    let names = git(&directory, &["diff", "--name-only", &t0, &t1], None).unwrap();

    assert!(names.contains("new.txt") && !names.contains(".env"), "{names}");

    let _ = std::fs::remove_dir_all(directory);
}

#[test]
fn files_have_new_side_ranges_and_tags() {
    let diff = "diff --git a/x.rs b/x.rs\n--- a/x.rs\n+++ b/x.rs\n@@ -3,2 +3,3 @@\n-a\n-b\n+a\n+b\n+c\n@@ -10 +11,0 @@\n-gone\ndiff --git a/n.rs b/n.rs\n--- /dev/null\n+++ b/n.rs\n@@ -0,0 +1,2 @@\n+1\n+2\n";
    let file = files(diff);

    assert_eq!(file.len(), 2);
    assert_eq!(
        (file[0].ranges.clone(), file[0].add, file[0].del),
        (vec![(3, 5), (11, 11)], 3, 3)
    );
    assert_eq!(file[0].label(), "x.rs:3-5, 11 (+3 −3)");
    assert_eq!(file[1].label(), "n.rs (new)");
}

#[test]
fn owner_and_other_agent_edits_reach_the_agent_its_own_do_not() {
    let directory = repo("note");
    let mut log = Vec::new();
    let t0 = tree(&directory).unwrap();

    // ada's turn: edits line 2
    sh(&directory, "printf 'a\\nB\\nc\\nd\\ne\\n' > x.txt");
    let t1 = tree(&directory).unwrap();

    log.push(Change {
        ts: 10,
        by: Some("ada".into()),
        from: t0,
        to: t1.clone(),
    });

    // the owner, between turns: line 4, a new file
    sh(&directory, "printf 'a\\nB\\nc\\nD\\ne\\n' > x.txt && echo hi > y.txt");
    let t2 = tree(&directory).unwrap();

    log.push(Change {
        ts: 20,
        by: None,
        from: t1,
        to: t2.clone(),
    });

    // bob's turn: line 5
    sh(&directory, "printf 'a\\nB\\nc\\nD\\nE\\n' > x.txt");
    let t3 = tree(&directory).unwrap();

    log.push(Change {
        ts: 30,
        by: Some("bob".into()),
        from: t2,
        to: t3,
    });
    let count = note(&directory, &log, 10, "ada", Mode::Diff).unwrap();

    assert!(
        count.text.contains("owner:\n- x.txt:4 (+1 −1)\n- y.txt (new)\n"),
        "{}",
        count.text
    );
    assert!(count.text.contains("@bob:\n- x.txt:5 (+1 −1)"), "{}", count.text);
    assert!(!count.text.contains("@ada"), "own edits: 0\n{}", count.text);
    assert!(
        count.text.contains("```diff") && count.text.contains("+D"),
        "{}",
        count.text
    );
    assert_eq!(
        count.summary,
        "owner: x.txt:4 (+1 −1), y.txt (new) · @bob: x.txt:5 (+1 −1)"
    );

    // files mode: 0 diff
    let file = note(&directory, &log, 10, "ada", Mode::Files).unwrap();

    assert!(!file.text.contains("```diff"), "{}", file.text);

    // 0 changes after its turn: 0 note
    assert!(note(&directory, &log, 30, "ada", Mode::Diff).is_none());

    let _ = std::fs::remove_dir_all(directory);
}

#[test]
fn owner_spans_are_the_gaps_with_0_agents_at_work() {
    let mut st = State::default();

    let by = |st: &State| {
        st.log
            .iter()
            .map(|character| {
                (
                    character.by.clone().unwrap_or_else(|| "owner".into()),
                    character.from.clone(),
                    character.to.clone(),
                )
            })
            .collect::<Vec<_>>()
    };

    // ada: t0 → t1; the owner: t1 → t2; ada again
    st.start("t0".into(), 1, "ada", false);
    st.end("t1".into(), 2, "ada", true);
    st.start("t2".into(), 3, "ada", false);

    assert_eq!(
        by(&st),
        [
            ("ada".into(), "t0".into(), "t1".into()),
            ("owner".into(), "t1".into(), "t2".into())
        ]
    );

    // bob starts while ada works: 0 owner span; ada ends while bob works: 0 quiet tree
    st.start("t3".into(), 4, "bob", true);
    st.end("t4".into(), 5, "ada", false);

    assert_eq!(st.quiet, None);

    // bob ends, all idle: the next start (same tree) adds 0 owner span
    st.end("t5".into(), 6, "bob", true);
    st.start("t5".into(), 7, "ada", false);

    assert_eq!(by(&st).len(), 4, "{:?}", by(&st));
    assert_eq!(st.seen.get("bob"), Some(&6));
}

#[test]
fn whitespace_only_edits_make_no_note() {
    let directory = repo("ws");
    let t0 = tree(&directory).unwrap();

    sh(&directory, "printf 'a  \\nb\\nc\\nd\\ne\\n' > x.txt");
    let t1 = tree(&directory).unwrap();
    let log = vec![Change {
        ts: 20,
        by: None,
        from: t0,
        to: t1,
    }];

    assert!(note(&directory, &log, 10, "ada", Mode::Diff).is_none());

    let _ = std::fs::remove_dir_all(directory);
}
