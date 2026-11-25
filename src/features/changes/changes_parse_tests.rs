use super::*;

#[test]
fn parse_reads_hunks_numbers_and_file_kinds() {
    let files = parse(DIFF);

    assert_eq!(files.len(), 4);
    assert_eq!((files[0].path.as_str(), files[0].add, files[0].del), ("src/a.rs", 2, 1));
    assert_eq!(files[0].hunks[0].ctx, "fn run() {");

    let lines = &files[0].hunks[0].lines;

    assert_eq!((lines[0].old, lines[0].new), (Some(10), Some(10)));
    assert_eq!((lines[1].kind, lines[1].old, lines[1].new), (Kind::Del, Some(11), None));
    assert_eq!((lines[2].kind, lines[2].new), (Kind::Add, Some(11)));
    assert_eq!((lines[4].old, lines[4].new), (Some(12), Some(13)));
    assert_eq!(lines.len(), 5, "0 line for `\\ No newline`");
    assert_eq!((files[1].status, files[1].cat), (Status::New, Cat::Tests));
    assert_eq!(
        (files[2].status, files[2].path.as_str(), files[2].old_path.as_deref()),
        (Status::Renamed, "new.rs", Some("old.rs"))
    );
    assert!(files[3].binary);
    assert_eq!(files[0].first_line(), 11);
}

#[test]
fn order_puts_schema_and_logic_first_tests_and_locks_last() {
    let mk = |path: &str, added: usize| {
        let mut file = FileDiff::new(path.into());

        file.add = added;
        file
    };

    let mut files = vec![
        mk("Cargo.lock", 900),
        mk("src/ui/button.css", 5),
        mk("tests/x.rs", 50),
        mk("README.md", 3),
        mk("src/deploy.rs", 10),
        mk("migrations/001_init.sql", 2),
        mk("src/big.rs", 40),
        mk("package.json", 1),
    ];

    order(&mut files);
    let paths: Vec<&str> = files.iter().map(|file| file.path.as_str()).collect();

    assert_eq!(
        paths,
        [
            "migrations/001_init.sql",
            "src/big.rs",
            "src/deploy.rs",
            "package.json",
            "src/ui/button.css",
            "README.md",
            "tests/x.rs",
            "Cargo.lock"
        ]
    );
}

#[test]
fn word_marks_hit_only_the_changed_tokens() {
    let chars = |text: &str| text.chars().collect::<Vec<char>>();
    let (old_marks, new_marks) = word_marks(&chars("let b = 2;"), &chars("let b = 3;"));

    let on = |marks: &[bool], text: &str| {
        text.chars()
            .zip(marks)
            .filter(|(_, marked)| **marked)
            .map(|(character, _)| character)
            .collect::<String>()
    };

    assert_eq!(
        (on(&old_marks, "let b = 2;"), on(&new_marks, "let b = 3;")),
        ("2".into(), "3".into())
    );

    // a rewrite: 0 marks
    let (old_marks, new_marks) = word_marks(&chars("alpha beta gamma"), &chars("one two three"));

    assert!(!old_marks.contains(&true) && !new_marks.contains(&true));
}

#[test]
fn pr_checks_count_by_verdict() {
    let pr = serde_json::json!({"number": 12, "url": "https://x/12", "state": "OPEN", "statusCheckRollup": [
        {"status": "COMPLETED", "conclusion": "SUCCESS"}, {"status": "COMPLETED", "conclusion": "FAILURE"},
        {"status": "IN_PROGRESS", "conclusion": ""}, {"state": "SUCCESS"}, {"state": "PENDING"}
    ]});

    assert_eq!(
        parse_pr(&pr),
        Some(Pr {
            number: 12,
            url: "https://x/12".into(),
            state: "OPEN".into(),
            pass: 2,
            fail: 1,
            pending: 2
        })
    );

    let mut view = view(parse(DIFF));

    view.pull_request.pr = parse_pr(&pr);
    view.build(140);

    assert!(summary(&view).contains("PR #12 open ✓2 ✗1 ●2"), "{:?}", summary(&view));
}

#[test]
fn commit_drafts_lose_fences_and_notes() {
    assert_eq!(clean_message("```\nfix: x\n\n- y\n```"), "fix: x\n\n- y");
    assert_eq!(
        clean_message("fix: x\n\n- y\n\nNote: I could not check doinstruct.tools"),
        "fix: x\n\n- y"
    );
}
