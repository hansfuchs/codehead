use super::*;

#[test]
fn parse_reads_hunks_numbers_and_file_kinds() {
    let file = parse(DIFF);

    assert_eq!(file.len(), 4);
    assert_eq!((file[0].path.as_str(), file[0].add, file[0].del), ("src/a.rs", 2, 1));
    assert_eq!(file[0].hunks[0].ctx, "fn run() {");

    let ls = &file[0].hunks[0].lines;

    assert_eq!((ls[0].old, ls[0].new), (Some(10), Some(10)));
    assert_eq!((ls[1].kind, ls[1].old, ls[1].new), (Kind::Del, Some(11), None));
    assert_eq!((ls[2].kind, ls[2].new), (Kind::Add, Some(11)));
    assert_eq!((ls[4].old, ls[4].new), (Some(12), Some(13)));
    assert_eq!(ls.len(), 5, "0 line for `\\ No newline`");
    assert_eq!((file[1].status, file[1].cat), (Status::New, Cat::Tests));
    assert_eq!(
        (file[2].status, file[2].path.as_str(), file[2].old_path.as_deref()),
        (Status::Renamed, "new.rs", Some("old.rs"))
    );
    assert!(file[3].binary);
    assert_eq!(file[0].first_line(), 11);
}

#[test]
fn order_puts_schema_and_logic_first_tests_and_locks_last() {
    let mk = |path_value: &str, count: usize| {
        let mut file = FileDiff::new(path_value.into());

        file.add = count;
        file
    };

    let mut value = vec![
        mk("Cargo.lock", 900),
        mk("src/ui/button.css", 5),
        mk("tests/x.rs", 50),
        mk("README.md", 3),
        mk("src/deploy.rs", 10),
        mk("migrations/001_init.sql", 2),
        mk("src/big.rs", 40),
        mk("package.json", 1),
    ];

    order(&mut value);
    let paths: Vec<&str> = value.iter().map(|file_value| file_value.path.as_str()).collect();

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
    let character = |text: &str| text.chars().collect::<Vec<char>>();
    let (output, count) = word_marks(&character("let b = 2;"), &character("let b = 3;"));

    let on = |value: &[bool], text_value: &str| {
        text_value
            .chars()
            .zip(value)
            .filter(|x| *x.1)
            .map(|x| x.0)
            .collect::<String>()
    };

    assert_eq!(
        (on(&output, "let b = 2;"), on(&count, "let b = 3;")),
        ("2".into(), "3".into())
    );

    // a rewrite: 0 marks
    let (output_value, count_value) = word_marks(&character("alpha beta gamma"), &character("one two three"));

    assert!(!output_value.contains(&true) && !count_value.contains(&true));
}

#[test]
fn pr_checks_count_by_verdict() {
    let value = serde_json::json!({"number": 12, "url": "https://x/12", "state": "OPEN", "statusCheckRollup": [
        {"status": "COMPLETED", "conclusion": "SUCCESS"}, {"status": "COMPLETED", "conclusion": "FAILURE"},
        {"status": "IN_PROGRESS", "conclusion": ""}, {"state": "SUCCESS"}, {"state": "PENDING"}
    ]});

    assert_eq!(
        parse_pr(&value),
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

    view.pull_request.pr = parse_pr(&value);
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
