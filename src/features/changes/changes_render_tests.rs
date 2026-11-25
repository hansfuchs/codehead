use super::*;

#[test]
fn boxes_unified_and_side_by_side_by_width() {
    let mut view = view(parse(DIFF));

    view.build(120);
    let rows = text(&view);
    let head_row = rows.iter().position(|line| line.starts_with("┌─ ☐ src/a.rs")).unwrap();

    assert!(
        rows[head_row].ends_with("logic · +2 −1 ─┐") && summary(&view).contains("4 files"),
        "{rows:#?}"
    );
    assert!(rows.iter().any(|line| line.contains("⋯ fn run() {")), "{rows:#?}");
    assert!(
        rows.iter().any(|line| line.contains(" 11     - let b = 2;")),
        "{rows:#?}"
    );
    assert!(
        rows.iter().any(|line| line.contains("     11 + let b = 3;")),
        "{rows:#?}"
    );
    assert!(
        rows.iter()
            .filter(|line| line.starts_with(['┌', '│', '└']))
            .all(|line| line.width() == 120),
        "every box row 120 wide: {rows:#?}"
    );

    // binary: folded
    assert!(!rows.iter().any(|line| line.contains("binary file")));

    view.build(SIDE_MIN);
    let side = text(&view);
    let row = side.iter().find(|line| line.contains("let b = 2;")).unwrap();

    assert!(
        row.contains("- let b = 2;") && row.contains("+ let b = 3;"),
        "pairs on 1 row: {row}"
    );
    assert!(
        side.iter()
            .any(|line| line.contains("+ let c = 4;") && !line.contains("- ")),
        "{side:#?}"
    );

    // a new file stays unified: 0 empty left half
    let new = side.iter().find(|line| line.contains("fn t() {}")).unwrap();

    assert!(!new.contains(" │ "), "{new}");
}

#[test]
fn a_rank_orders_groups_and_shows_whys() {
    use crate::domain::changes::rank::{Group, RFile, Rank};
    let mut view = view(parse(DIFF));
    let key = crate::domain::changes::rank::key(&view.changes.files);
    let file = |path: &str, why: &str| RFile {
        path: path.into(),
        why: why.into(),
        level: "high".into(),
    };

    view.classifier.rank = Some(Rank {
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
    view.apply_rank(false);
    let paths: Vec<&str> = view.changes.files.iter().map(|diff| diff.path.as_str()).collect();

    assert_eq!(
        paths,
        ["tests/a_test.rs", "src/a.rs", "new.rs", "img.png"],
        "unranked files after the groups"
    );

    view.build(100);
    let rows = text(&view);

    assert!(summary(&view).ends_with("order: haiku"), "{rows:#?}");

    let first_group = rows
        .iter()
        .position(|line| line.starts_with(" 1 · Test first  the test drives it"))
        .unwrap();

    assert!(
        rows[first_group + 1].contains("☐ tests/a_test.rs") && rows[first_group + 2].contains("adds the test"),
        "{rows:#?}"
    );
    assert!(rows.iter().any(|line| line.starts_with(" 2 · Core")), "{rows:#?}");

    // the agent's levels: the 3 high; img.png (unranked, path rules: low) after them, under
    // its category
    assert_eq!(view.classifier.counts, [(0, 0), (0, 3), (0, 0), (0, 1)]);

    let img = rows.iter().position(|line| line.contains("img.png")).unwrap();

    assert_eq!(rows[img - 1], " logic", "{rows:#?}");

    // a new set of paths: the rank no longer applies
    view.changes.files.pop();
    view.layout.built = None;
    view.build(100);

    assert!(summary(&view).ends_with("path order"));
}

#[test]
fn rows_have_vivid_syntax_colors_a_sign_column_and_word_marks() {
    let mut view = view(parse(DIFF));

    view.build(100);
    let i = (0..view.layout.rows.len())
        .find(|&i| text_of(&view, i).contains("+ let b = 3;"))
        .unwrap();
    let line = view.line(i);
    let (sign_bg, mark_bg) = tints(Kind::Add);

    // the sign column: the add color as its background; the rest: 0 line tint
    let sign = line.spans.iter().find(|span| span.content == " + ").unwrap();

    assert_eq!(sign.style.bg, sign_bg);

    // `let`: a keyword color, more saturated than the theme's
    let keyword = line.spans.iter().find(|span| span.content.starts_with("let")).unwrap();

    assert_ne!(keyword.style.fg, Some(theme::pal().text), "syntax color");
    assert_eq!(keyword.style.bg, None, "0 tint over the line");

    let plain = crate::ui::code::highlight("let b = 3;", "rs")[0][0].0;

    assert_eq!(keyword.style.fg, Some(theme::desaturate(plain, SATURATION)));

    // `3`: the changed word, its tint
    let three = line.spans.iter().find(|span| span.content.contains('3')).unwrap();

    assert_eq!(three.style.bg, mark_bg, "{:?}", line.spans);

    // context rows: 0 tint
    let ctx = (0..view.layout.rows.len())
        .find(|&i| text_of(&view, i).contains("let a = 1;"))
        .unwrap();

    assert!(
        view.line(ctx)
            .spans
            .iter()
            .filter(|span| span.content.contains("let"))
            .all(|span| span.style.bg.is_none())
    );
}

#[test]
fn v_marks_reviewed_folds_and_a_changed_diff_shows_updated() {
    let mut view = view(parse(DIFF));

    view.build(100);
    view.at = view.layout.rows.iter().position(|row| row.head).unwrap();
    view.key(KeyEvent::new(KeyCode::Char('v'), KeyModifiers::NONE));

    assert_eq!(view.progress(), (1, 4));

    let rows = text(&view);
    let head_row = rows.iter().position(|line| line.contains("☑ src/a.rs")).unwrap();

    assert!(
        rows[head_row].starts_with("── ☑") && rows[head_row].ends_with("──") && !rows[head_row + 1].starts_with('│'),
        "folded: 1 line: {rows:#?}"
    );

    // the cursor went to the next box (medium: src/a.rs, new.rs)
    assert!(view.layout.rows[view.at].head && view.changes.files[view.layout.rows[view.at].file].path == "new.rs");

    // the file changes: the mark goes, `updated` shows
    let mut changed = parse(&DIFF.replace("let c = 4;", "let c = 5;"));

    order(&mut changed);
    view.changes.files = changed;
    view.layout.built = None;
    view.build(100);
    let updated = text(&view);
    let head = updated.iter().find(|line| line.contains("src/a.rs")).unwrap();

    assert!(head.contains("☐ src/a.rs updated"), "{head}");
    assert_eq!(view.progress().0, 0);

    let _ = std::fs::remove_dir_all(crate::engine::store::thread_dir("changes-test-0000"));
}

#[test]
fn every_level_shows_and_the_summary_stays_on_top() {
    let mut view = view(parse(DIFF));
    let text = screen(&mut view, 100, 12);

    assert!(
        text[0].contains("4 files") && text[1].contains("filter by name"),
        "{text:#?}"
    );

    // all files, by level: src/a.rs (medium) before tests/a_test.rs (low)
    let all = view
        .layout
        .rows
        .iter()
        .filter(|row| row.head)
        .map(|row| view.changes.files[row.file].path.clone())
        .collect::<Vec<_>>();

    assert_eq!(all[0], "src/a.rs");
    assert!(
        all.contains(&"tests/a_test.rs".to_string()) && all.contains(&"img.png".to_string()),
        "{all:?}"
    );

    // scrolled into the box: the summary stays, the box head pins under it
    for _ in 0..6 {
        press(&mut view, KeyCode::Down);
    }

    view.top = view
        .layout
        .rows
        .iter()
        .position(|row| row.body == Body::Diff(0, 2))
        .unwrap();
    view.at = view.top + 2;
    let pinned = screen(&mut view, 100, 12);

    assert!(pinned[0].contains("4 files"), "{pinned:#?}");
    assert!(pinned[2].starts_with("┌─ ☐ src/a.rs"), "the head pinned: {pinned:#?}");
    assert!(
        !pinned.iter().skip(3).any(|line| line.contains("┌─ ☐ src/a.rs")),
        "{pinned:#?}"
    );

    // a call on its way: classifying…
    let (_tx, rx) = crate::infra::bus::channel();

    view.classifier.ranking = Some(rx);

    assert!(screen(&mut view, 100, 12)[0].contains("classifying…"));
}

#[test]
fn a_re_read_of_the_same_diffs_keeps_the_rows_and_takes_the_git_state() {
    let mut view = view(parse(DIFF));

    screen(&mut view, 100, 30);

    for _ in 0..4 {
        press(&mut view, KeyCode::Down);
    }

    let at = view.at;

    let read = |view: &mut View, changes: Changes| {
        let (tx, rx) = crate::infra::bus::channel();

        tx.send(Ok(changes)).unwrap();
        view.loading = Some(rx);
        view.poll()
    };

    // the same files in the read's (path) order, 1 staged now
    let mut files = parse(DIFF);

    files.reverse();
    let staged = Changes {
        base: "origin/main".into(),
        files,
        staged: HashMap::from([("src/a.rs".to_string(), true)]),
        ..Default::default()
    };

    assert!(read(&mut view, staged.clone()), "the staged mark changed");
    assert!(
        view.layout.built.is_some() && view.at == at,
        "0 new rows, the cursor stays"
    );
    assert!(
        screen(&mut view, 100, 30)
            .iter()
            .any(|line| line.contains("☐ ● src/a.rs"))
    );
    assert!(!read(&mut view, staged), "0 change: 0 frame");

    // a changed diff of the cursor's file: the cursor stays on its row of the file
    let mut files = parse(&DIFF.replace("let d = 5;", "let d = 6;"));

    order(&mut files);

    assert!(read(
        &mut view,
        Changes {
            base: "origin/main".into(),
            files,
            ..Default::default()
        }
    ));

    screen(&mut view, 100, 30);

    assert_eq!(view.at, at);
}
