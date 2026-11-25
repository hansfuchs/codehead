use super::*;

#[test]
fn boxes_unified_and_side_by_side_by_width() {
    let mut value = view(parse(DIFF));

    value.build(120);
    let thread = text(&value);
    let height = thread
        .iter()
        .position(|line| line.starts_with("┌─ ☐ src/a.rs"))
        .unwrap();

    assert!(
        thread[height].ends_with("logic · +2 −1 ─┐") && summary(&value).contains("4 files"),
        "{thread:#?}"
    );
    assert!(
        thread.iter().any(|line_value| line_value.contains("⋯ fn run() {")),
        "{thread:#?}"
    );
    assert!(
        thread.iter().any(|line_2| line_2.contains(" 11     - let b = 2;")),
        "{thread:#?}"
    );
    assert!(
        thread.iter().any(|line_3| line_3.contains("     11 + let b = 3;")),
        "{thread:#?}"
    );
    assert!(
        thread
            .iter()
            .filter(|line_4| line_4.starts_with(['┌', '│', '└']))
            .all(|line_5| line_5.width() == 120),
        "every box row 120 wide: {thread:#?}"
    );

    // binary: folded
    assert!(!thread.iter().any(|line_6| line_6.contains("binary file")));

    value.build(SIDE_MIN);
    let thread_value = text(&value);
    let row = thread_value
        .iter()
        .find(|line_7| line_7.contains("let b = 2;"))
        .unwrap();

    assert!(
        row.contains("- let b = 2;") && row.contains("+ let b = 3;"),
        "pairs on 1 row: {row}"
    );
    assert!(
        thread_value
            .iter()
            .any(|line_8| line_8.contains("+ let c = 4;") && !line_8.contains("- ")),
        "{thread_value:#?}"
    );

    // a new file stays unified: 0 empty left half
    let new = thread_value.iter().find(|line_9| line_9.contains("fn t() {}")).unwrap();

    assert!(!new.contains(" │ "), "{new}");
}

#[test]
fn a_rank_orders_groups_and_shows_whys() {
    use crate::domain::changes::rank::{Group, RFile, Rank};
    let mut value = view(parse(DIFF));
    let key = crate::domain::changes::rank::key(&value.changes.files);
    let file = |path_value: &str, width: &str| RFile {
        path: path_value.into(),
        why: width.into(),
        level: "high".into(),
    };

    value.classifier.rank = Some(Rank {
        key,
        model: "haiku".into(),
        groups: vec![
            Group {
                title: "Test first".into(),
                why: "the test drives it".into(),
                files: vec![file("tests/a_test.rs", "adds the test")],
            },
            Group {
                title: "Core".into(),
                why: "b becomes 3".into(),
                files: vec![file("src/a.rs", "changes b"), file("new.rs", "")],
            },
        ],
    });
    value.apply_rank(false);
    let paths: Vec<&str> = value
        .changes
        .files
        .iter()
        .map(|file_value| file_value.path.as_str())
        .collect();

    assert_eq!(
        paths,
        ["tests/a_test.rs", "src/a.rs", "new.rs", "img.png"],
        "unranked files after the groups"
    );

    value.build(100);
    let thread = text(&value);

    assert!(summary(&value).ends_with("order: haiku"), "{thread:#?}");

    let g1 = thread
        .iter()
        .position(|line| line.starts_with(" 1 · Test first  the test drives it"))
        .unwrap();

    assert!(
        thread[g1 + 1].contains("☐ tests/a_test.rs") && thread[g1 + 2].contains("adds the test"),
        "{thread:#?}"
    );
    assert!(
        thread.iter().any(|line_value| line_value.starts_with(" 2 · Core")),
        "{thread:#?}"
    );

    // the agent's levels: the 3 high; img.png (unranked, path rules: low) after them, under
    // its category
    assert_eq!(value.classifier.counts, [(0, 0), (0, 3), (0, 0), (0, 1)]);

    let img = thread.iter().position(|line_2| line_2.contains("img.png")).unwrap();

    assert_eq!(thread[img - 1], " logic", "{thread:#?}");

    // a new set of paths: the rank no longer applies
    value.changes.files.pop();
    value.layout.built = None;
    value.build(100);

    assert!(summary(&value).ends_with("path order"));
}

#[test]
fn rows_have_vivid_syntax_colors_a_sign_column_and_word_marks() {
    let mut value = view(parse(DIFF));

    value.build(100);
    let i = (0..value.layout.rows.len())
        .find(|&i| text_of(&value, i).contains("+ let b = 3;"))
        .unwrap();
    let line = value.line(i);
    let (sign_bg, mark_bg) = tints(Kind::Add);

    // the sign column: the add color as its background; the rest: 0 line tint
    let sign = line
        .spans
        .iter()
        .find(|text_value| text_value.content == " + ")
        .unwrap();

    assert_eq!(sign.style.bg, sign_bg);

    // `let`: a keyword color, more saturated than the theme's
    let kw = line
        .spans
        .iter()
        .find(|text_2| text_2.content.starts_with("let"))
        .unwrap();

    assert_ne!(kw.style.fg, Some(theme::pal().text), "syntax color");
    assert_eq!(kw.style.bg, None, "0 tint over the line");

    let plain = crate::ui::code::highlight("let b = 3;", "rs")[0][0].0;

    assert_eq!(kw.style.fg, Some(theme::desaturate(plain, SATURATION)));

    // `3`: the changed word, its tint
    let three = line.spans.iter().find(|text_3| text_3.content.contains('3')).unwrap();

    assert_eq!(three.style.bg, mark_bg, "{:?}", line.spans);

    // context rows: 0 tint
    let ctx = (0..value.layout.rows.len())
        .find(|&i| text_of(&value, i).contains("let a = 1;"))
        .unwrap();

    assert!(
        value
            .line(ctx)
            .spans
            .iter()
            .filter(|text_4| text_4.content.contains("let"))
            .all(|text_5| text_5.style.bg.is_none())
    );
}

#[test]
fn v_marks_reviewed_folds_and_a_changed_diff_shows_updated() {
    let mut value = view(parse(DIFF));

    value.build(100);
    value.at = value.layout.rows.iter().position(|result| result.head).unwrap();
    value.key(KeyEvent::new(KeyCode::Char('v'), KeyModifiers::NONE));

    assert_eq!(value.progress(), (1, 4));

    let thread = text(&value);
    let height = thread.iter().position(|line| line.contains("☑ src/a.rs")).unwrap();

    assert!(
        thread[height].starts_with("── ☑") && thread[height].ends_with("──") && !thread[height + 1].starts_with('│'),
        "folded: 1 line: {thread:#?}"
    );

    // the cursor went to the next box (medium: src/a.rs, new.rs)
    assert!(value.layout.rows[value.at].head && value.changes.files[value.layout.rows[value.at].file].path == "new.rs");

    // the file changes: the mark goes, `updated` shows
    let mut changed = parse(&DIFF.replace("let c = 4;", "let c = 5;"));

    order(&mut changed);
    value.changes.files = changed;
    value.layout.built = None;
    value.build(100);
    let thread_value = text(&value);
    let head = thread_value
        .iter()
        .find(|line_value| line_value.contains("src/a.rs"))
        .unwrap();

    assert!(head.contains("☐ src/a.rs updated"), "{head}");
    assert_eq!(value.progress().0, 0);

    let _ = std::fs::remove_dir_all(crate::engine::store::thread_dir("changes-test-0000"));
}

#[test]
fn every_level_shows_and_the_summary_stays_on_top() {
    let mut value = view(parse(DIFF));
    let text = screen(&mut value, 100, 12);

    assert!(
        text[0].contains("4 files") && text[1].contains("filter by name"),
        "{text:#?}"
    );

    // all files, by level: src/a.rs (medium) before tests/a_test.rs (low)
    let all = value
        .layout
        .rows
        .iter()
        .filter(|result| result.head)
        .map(|result_value| value.changes.files[result_value.file].path.clone())
        .collect::<Vec<_>>();

    assert_eq!(all[0], "src/a.rs");
    assert!(
        all.contains(&"tests/a_test.rs".to_string()) && all.contains(&"img.png".to_string()),
        "{all:?}"
    );

    // scrolled into the box: the summary stays, the box head pins under it
    for _ in 0..6 {
        press(&mut value, KeyCode::Down);
    }

    value.top = value
        .layout
        .rows
        .iter()
        .position(|result_2| result_2.body == Body::Diff(0, 2))
        .unwrap();
    value.at = value.top + 2;
    let text_value = screen(&mut value, 100, 12);

    assert!(text_value[0].contains("4 files"), "{text_value:#?}");
    assert!(
        text_value[2].starts_with("┌─ ☐ src/a.rs"),
        "the head pinned: {text_value:#?}"
    );
    assert!(
        !text_value.iter().skip(3).any(|line| line.contains("┌─ ☐ src/a.rs")),
        "{text_value:#?}"
    );

    // a call on its way: classifying…
    let (_tx, rx) = crate::infra::bus::channel();

    value.classifier.ranking = Some(rx);

    assert!(screen(&mut value, 100, 12)[0].contains("classifying…"));
}

#[test]
fn a_re_read_of_the_same_diffs_keeps_the_rows_and_takes_the_git_state() {
    let mut value = view(parse(DIFF));

    screen(&mut value, 100, 30);

    for _ in 0..4 {
        press(&mut value, KeyCode::Down);
    }

    let at = value.at;

    let read = |view_value: &mut View, changes: Changes| {
        let (tx, rx) = crate::infra::bus::channel();

        tx.send(Ok(changes)).unwrap();
        view_value.loading = Some(rx);
        view_value.poll()
    };

    // the same files in the read's (path) order, 1 staged now
    let mut files = parse(DIFF);

    files.reverse();
    let changes_value = Changes {
        base: "origin/main".into(),
        files,
        staged: HashMap::from([("src/a.rs".to_string(), true)]),
        ..Default::default()
    };

    assert!(read(&mut value, changes_value.clone()), "the staged mark changed");
    assert!(
        value.layout.built.is_some() && value.at == at,
        "0 new rows, the cursor stays"
    );
    assert!(
        screen(&mut value, 100, 30)
            .iter()
            .any(|line| line.contains("☐ ● src/a.rs"))
    );
    assert!(!read(&mut value, changes_value), "0 change: 0 frame");

    // a changed diff of the cursor's file: the cursor stays on its row of the file
    let mut files = parse(&DIFF.replace("let d = 5;", "let d = 6;"));

    order(&mut files);

    assert!(read(
        &mut value,
        Changes {
            base: "origin/main".into(),
            files,
            ..Default::default()
        }
    ));

    screen(&mut value, 100, 30);

    assert_eq!(value.at, at);
}
