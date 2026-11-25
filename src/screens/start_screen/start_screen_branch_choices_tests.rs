use super::*;

#[test]
fn where_row_branch_new_branch_worktree() {
    let dir = std::env::temp_dir().join(format!("codehead-where-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);

    std::fs::create_dir_all(&dir).unwrap();
    let group = |args: &[&str]| assert!(threads::git(&dir, args).0, "git {args:?}");

    group(&["init", "-q", "-b", "main"]);
    group(&[
        "-c",
        "user.name=t",
        "-c",
        "user.email=t@t",
        "commit",
        "-q",
        "--allow-empty",
        "-m",
        "a",
    ]);
    group(&["branch", "feature/x"]);
    where_row_branch_new_branch_worktree_verify_cfg(&dir, group);
}

fn where_row_branch_new_branch_worktree_verify_cfg(dir: &Path, group: impl Fn(&[&str])) {
    group(&["switch", "-q", "feature/x"]);
    let cfg = Config {
        notes: dir.join("notes"),
        repo: Some(dir.to_path_buf()),
        cwd: dir.to_path_buf(),
        agent: "amon".into(),
        picks: vec![],
        branch: None,
        prefix: "task".into(),
        copy_mode: false,
        logs: dir.join("logs"),
        drafts: dir.join("drafts"),
    };
    let mut app = App::new(cfg, "Fix deploy");

    app.agent_list.agent_reads = Reads::default();

    // order: current branch (feature/x), new branch, new worktree, main, other branches
    assert_eq!(
        app.repository.wheres,
        [
            Where::Current,
            Where::NewBranch,
            Where::NewWorktree,
            Where::Branch("main".into())
        ]
    );
    assert!(matches!(app.target(), Ok(Target::Worktree { branch, .. }) if branch == "feature/x"));
    assert_eq!(app.checkout_box(), (true, false), "current branch: checked, disabled");
    assert!(!app.stash_box().1);

    app.compose.field = Field::Where;

    // the dropdown, bottom up: main, new worktree, new branch, current (the cursor)
    let pick_up = |app_value: &mut App, ups: usize| {
        key(app_value, KeyCode::Enter, KeyModifiers::NONE);

        for _ in 0..ups {
            key(app_value, KeyCode::Up, KeyModifiers::NONE);
        }

        key(app_value, KeyCode::Enter, KeyModifiers::NONE);

        assert!(app_value.repository.where_menu.is_none());
    };

    pick_up(&mut app, 1);

    assert!(matches!(app.target(), Ok(Target::NewBranch(bytes)) if bytes == "task/fix-deploy"));
    assert_eq!(app.checkout_box(), (false, false));

    pick_up(&mut app, 1);

    assert!(matches!(app.target(), Ok(Target::Branch(bytes_value)) if bytes_value == "task/fix-deploy"));
    assert_eq!(app.where_chip(), "+ new branch + worktree");

    pick_up(&mut app, 1);

    assert!(matches!(app.target(), Ok(Target::Checkout { branch, switch: true, stash: false }) if branch == "main"));
    where_row_branch_new_branch_worktree_verify_state(dir, app, pick_up);
}

fn where_row_branch_new_branch_worktree_verify_state(dir: &Path, mut app: App, pick_up: impl Fn(&mut App, usize)) {
    assert!(
        app.where_text().starts_with("switches ") && app.where_text().ends_with("from feature/x to main"),
        "{}",
        app.where_text()
    );

    // opts: 1 box per row, ↓ from where: checkout; ↓ again: stash (only with checkout)
    key(&mut app, KeyCode::Down, KeyModifiers::NONE);

    assert!(app.compose.field == Field::Opts && app.repository.opt_i == 0);

    key(&mut app, KeyCode::Down, KeyModifiers::NONE);

    assert!(app.compose.field == Field::Opts && app.repository.opt_i == 1);

    key(&mut app, KeyCode::Char(' '), KeyModifiers::NONE);

    assert!(matches!(app.target(), Ok(Target::Checkout { stash: true, .. })));

    key(&mut app, KeyCode::Up, KeyModifiers::NONE);

    assert!(app.compose.field == Field::Opts && app.repository.opt_i == 0);

    key(&mut app, KeyCode::Char(' '), KeyModifiers::NONE);

    assert!(matches!(
        app.target(),
        Ok(Target::Checkout {
            switch: false,
            stash: false,
            ..
        })
    ));
    assert!(app.where_text().contains("0 checkout"), "{}", app.where_text());

    key(&mut app, KeyCode::Char(' '), KeyModifiers::NONE);
    key(&mut app, KeyCode::Up, KeyModifiers::NONE);

    assert!(app.compose.field == Field::Where);

    // the cursor starts on main (the current choice): 3 rows down to current branch
    key(&mut app, KeyCode::Enter, KeyModifiers::NONE);

    for _ in 0..3 {
        key(&mut app, KeyCode::Down, KeyModifiers::NONE);
    }

    key(&mut app, KeyCode::Enter, KeyModifiers::NONE);

    assert_eq!(app.where_chip(), "current branch · feature/x");

    // a worktree from the threads tab wins until a pick in the dropdown
    app.compose.target = Some(Target::Worktree {
        path: "/w".into(),
        branch: "b".into(),
    });

    assert_eq!(app.where_chip(), "worktree b");

    pick_up(&mut app, 0);

    assert_eq!(app.where_chip(), "current branch · feature/x");

    // field order: title → repo → projects → branch → opts (2 rows, also with disabled boxes) → agents
    app.compose.field = Field::Title;
    key(&mut app, KeyCode::Down, KeyModifiers::NONE);

    assert!(app.compose.field == Field::Repo);

    key(&mut app, KeyCode::Down, KeyModifiers::NONE);

    assert!(app.compose.field == Field::Projects);

    key(&mut app, KeyCode::Down, KeyModifiers::NONE);

    assert!(app.compose.field == Field::Where);

    key(&mut app, KeyCode::Down, KeyModifiers::NONE);

    where_row_branch_new_branch_worktree_verify_out(dir, app);
}

fn where_row_branch_new_branch_worktree_verify_out(dir: &Path, mut app: App) {
    assert!(app.compose.field == Field::Opts);

    key(&mut app, KeyCode::Down, KeyModifiers::NONE);

    assert!(app.compose.field == Field::Opts && app.repository.opt_i == 1);

    key(&mut app, KeyCode::Down, KeyModifiers::NONE);

    assert!(app.compose.field == Field::Agents);

    key(&mut app, KeyCode::Up, KeyModifiers::NONE);

    assert!(app.compose.field == Field::Opts && app.repository.opt_i == 1);

    app.compose.screen = Screen::Compose;
    app.agent_list.agents = vec![
        Agent {
            name: "amon".into(),
            harness: "claude".into(),
            model: Some("opus".into()),
            color: Some("red".into()),
            ..Default::default()
        },
        Agent {
            name: "nova".into(),
            harness: "codex".into(),
            model: Some("gpt-5.5".into()),
            ..Default::default()
        },
    ];
    app.compose.picks = vec!["amon".into()];
    let out = dump(&mut app, 110, 39);

    assert!(out.contains("branch  current branch · feature/x"), "{out}");
    assert!(out.contains("◼ checkout selected branch"), "{out}");

    // each box on its own row, below each other
    let lines: Vec<&str> = out.lines().collect();

    let row = |text: &str| {
        lines
            .iter()
            .position(|line| line.contains(text))
            .unwrap_or_else(|| panic!("{text}\n{out}"))
    };

    let (co, st) = (row("checkout selected branch"), row("stash conflicting local changes"));

    assert_eq!((row("branch  current"), co), (co - 1, st - 1), "{out}");
    assert_eq!(lines[co].find("checkout"), lines[st].find("stash"), "{out}");
    assert!(
        out.contains("claude · Opus 5.5") && out.contains("codex · gpt-5.5"),
        "{out}"
    );

    // 0 blank rows between the harness groups: the codex heading right below the claude card
    let codex = row("codex · 1");

    assert!(lines[codex - 1].contains('└'), "{out}");

    println!("--- where row\n{out}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn where_dropdown_orders_filters_picks_and_closes() {
    let dir = std::env::temp_dir().join(format!("codehead-where-dd-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let repo = dir.join("repo");

    std::fs::create_dir_all(&repo).unwrap();

    // main (2020), alpha (2021, own worktree), beta (2022), feature/x (2023, checked out)
    let sh = "git init -q -b main && git config user.email t@t && git config user.name t \
        && GIT_COMMITTER_DATE=2020-01-01T00:00:00 git commit -q --allow-empty -m m \
        && git switch -q -c alpha && GIT_COMMITTER_DATE=2021-01-01T00:00:00 git commit -q --allow-empty -m a \
        && git switch -q -c beta && GIT_COMMITTER_DATE=2022-01-01T00:00:00 git commit -q --allow-empty -m b \
        && git switch -q -c feature/x && GIT_COMMITTER_DATE=2023-01-01T00:00:00 git commit -q --allow-empty -m x \
        && git worktree add -q ../wt-alpha alpha";

    assert!(
        std::process::Command::new("sh")
            .arg("-c")
            .arg(sh)
            .current_dir(&repo)
            .status()
            .unwrap()
            .success()
    );

    let mut app = test_app();

    app.cfg.repo = Some(repo.clone());
    app.cfg.cwd = repo.clone();
    let character = where_choices(&repo, &repo);

    where_dropdown_orders_filters_picks_and_closes_verify_names(&dir, app, character);
}

fn where_dropdown_orders_filters_picks_and_closes_verify_names(
    dir: &std::path::Path,
    mut app: App,
    character: Choices,
) {
    (
        app.repository.wheres,
        app.repository.checkouts,
        app.repository.repo_branch,
        app.repository.here_branch,
        app.repository.here_dir,
    ) = (
        character.wheres,
        character.checkouts,
        character.repo_branch,
        character.here_branch,
        character.here_dir,
    );
    app.repository.where_i = 0;
    app.compose.title.set_text("fix it");
    app.compose.field = Field::Where;

    let names = |app_value: &App| {
        app_value
            .preview
            .regions
            .where_rows
            .iter()
            .map(|(_, i)| app_value.where_label(&app_value.repository.wheres[*i]).0)
            .collect::<Vec<_>>()
    };

    // enter: open; the rows bottom up: current, new branch, new worktree, main, then the newest
    key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    let out = dump(&mut app, 110, 40);

    assert_eq!(
        names(&app),
        [
            "alpha",
            "beta",
            "main",
            "+ new branch + worktree",
            "+ new branch",
            "current branch"
        ],
        "{out}"
    );
    assert!(out.contains("● current branch") && out.contains("feature/x"), "{out}");
    assert!(
        out.lines()
            .any(|line| line.contains("alpha") && line.contains("worktree …") && line.contains("wt-alpha")),
        "alpha: its worktree\n{out}"
    );

    where_dropdown_orders_filters_picks_and_closes_verify_out(dir, app, names, &out);
}

fn where_dropdown_orders_filters_picks_and_closes_verify_out(
    dir: &std::path::Path,
    mut app: App,
    names: impl Fn(&App) -> Vec<String>,
    out: &str,
) {
    println!("--- where dropdown\n{out}");

    // above the where row, the cursor on the current choice (bottom row)
    assert!(
        app.preview
            .regions
            .where_rows
            .iter()
            .all(|(row, _)| row.y < app.preview.regions.where_row.y),
        "{out}"
    );
    assert_eq!(
        app.repository.where_menu.as_ref().map(|message| message.cursor),
        Some(5)
    );

    // ↑↑↑: main; enter picks it and closes
    for _ in 0..3 {
        key(&mut app, KeyCode::Up, KeyModifiers::NONE);
    }

    key(&mut app, KeyCode::Enter, KeyModifiers::NONE);

    assert!(app.repository.where_menu.is_none());
    assert_eq!(
        app.repository.wheres[app.repository.where_i],
        Where::Branch("main".into())
    );

    // reopened: the cursor on main; type: filter (r types too: 0 repo switch), best at the bottom
    key(&mut app, KeyCode::Enter, KeyModifiers::NONE);

    assert_eq!(
        app.repository
            .where_menu
            .as_ref()
            .map(|message_value| message_value.cursor),
        Some(2)
    );

    type_text(&mut app, "alp");
    let out = dump(&mut app, 110, 40);

    assert_eq!(names(&app), ["alpha"], "{out}");
    assert!(out.contains("where › alp") && out.contains("1 of 6"), "{out}");

    key(&mut app, KeyCode::Enter, KeyModifiers::NONE);

    assert_eq!(
        app.repository.wheres[app.repository.where_i],
        Where::Branch("alpha".into())
    );
    assert_eq!(app.checkout_box(), (true, false), "alpha: checked out in its worktree");

    // 0 matches: enter keeps it open; backspaces back to every row, the cursor on alpha
    key(&mut app, KeyCode::Char(' '), KeyModifiers::NONE);
    type_text(&mut app, "zzz");
    let out = dump(&mut app, 110, 40);

    where_dropdown_orders_filters_picks_and_closes_verify_width(dir, app, &out);
}

fn where_dropdown_orders_filters_picks_and_closes_verify_width(dir: &std::path::Path, mut app: App, out: &str) {
    assert!(out.contains("0 matches"), "{out}");

    key(&mut app, KeyCode::Enter, KeyModifiers::NONE);

    assert!(app.repository.where_menu.is_some());

    for _ in 0..3 {
        key(&mut app, KeyCode::Backspace, KeyModifiers::NONE);
    }

    assert_eq!(
        app.repository
            .where_menu
            .as_ref()
            .map(|message_2| (message_2.query.as_str(), message_2.cursor)),
        Some(("", 0))
    );

    // esc: closed, choice kept, 0 quit (esc of the dropdown: 0 double esc)
    key(&mut app, KeyCode::Down, KeyModifiers::NONE);
    key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    key(&mut app, KeyCode::Esc, KeyModifiers::NONE);

    assert!(app.repository.where_menu.is_none() && app.outcome.is_none());
    assert_eq!(
        app.repository.wheres[app.repository.where_i],
        Where::Branch("alpha".into())
    );

    // mouse: a click on the where row opens it, on a row picks, next to it closes
    dump(&mut app, 110, 40);
    let width = app.preview.regions.where_row;
    let click = |x: u16, y: u16| MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: x,
        row: y,
        modifiers: KeyModifiers::NONE,
    };

    app.mouse(click(width.x + 1, width.y));

    assert!(app.repository.where_menu.is_some());

    dump(&mut app, 110, 40);
    let (row_value, i) = *app
        .preview
        .regions
        .where_rows
        .iter()
        .find(|(_, i)| app.repository.wheres[*i] == Where::NewBranch)
        .unwrap();

    app.mouse(click(row_value.x + 2, row_value.y));

    assert!(app.repository.where_menu.is_none() && app.repository.where_i == i);

    app.mouse(click(width.x + 1, width.y));
    app.mouse(click(0, 0));

    assert!(app.repository.where_menu.is_none() && app.repository.wheres[app.repository.where_i] == Where::NewBranch);

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn notes_field_has_a_blank_row_below() {
    let mut app = test_app();
    let height = 30;
    let out = dump(&mut app, 100, height);
    let lines: Vec<&str> = out.lines().collect();
    let notes = lines
        .iter()
        .rposition(|line| line.contains("notes · 0 picked"))
        .unwrap();

    // notes label, blank row, footer
    assert_eq!(notes, height as usize - 3, "{out}");
    assert!(lines[notes + 1].trim().is_empty(), "{out}");
}

#[test]
fn prompt_list_items_indent_with_space_and_outdent_with_backspace() {
    let mut app = test_app();

    assert!(app.compose.field == Field::Prompt);

    // typed: `- ` then space → nested, text goes after the marker
    type_text(&mut app, "- a");
    key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    type_text(&mut app, " b");

    assert_eq!(app.compose.prompt.text(), "- a\n  - b");

    // text already there: cursor right after the marker → space indents, backspace outdents
    app.compose.prompt.set_text("- a\n- foo");

    for _ in 0..3 {
        key(&mut app, KeyCode::Left, KeyModifiers::NONE);
    }

    key(&mut app, KeyCode::Char(' '), KeyModifiers::NONE);

    assert_eq!(app.compose.prompt.text(), "- a\n  - foo");

    key(&mut app, KeyCode::Char(' '), KeyModifiers::NONE);

    assert_eq!(app.compose.prompt.text(), "- a\n    - foo");

    key(&mut app, KeyCode::Backspace, KeyModifiers::NONE);

    assert_eq!(app.compose.prompt.text(), "- a\n  - foo");

    key(&mut app, KeyCode::Backspace, KeyModifiers::NONE);

    assert_eq!(app.compose.prompt.text(), "- a\n- foo");

    // indent 0: backspace as before
    key(&mut app, KeyCode::Backspace, KeyModifiers::NONE);

    assert_eq!(app.compose.prompt.text(), "- a\n-foo");

    // undo: back through the steps
    key(&mut app, KeyCode::Char('z'), KeyModifiers::CONTROL);

    assert_eq!(app.compose.prompt.text(), "- a\n- foo");

    key(&mut app, KeyCode::Char('z'), KeyModifiers::CONTROL);

    assert_eq!(app.compose.prompt.text(), "- a\n  - foo");

    let out = dump(&mut app, 100, 20);

    assert!(out.contains("  • foo"), "{out}");
}

#[test]
fn starts_in_task_view() {
    let mut app = test_app();

    assert!(app.compose.screen == Screen::Compose && app.compose.field == Field::Prompt);

    println!("--- start\n{}", dump(&mut app, 100, 18));
}

#[test]
fn enter_keeps_zero_notes() {
    let mut app = test_app();

    // prompt → title → repo → flow → agents → notes field (↓), enter opens the picker
    for _ in 0..5 {
        key(&mut app, KeyCode::Down, KeyModifiers::NONE);
    }

    assert!(app.compose.field == Field::Notes);

    key(&mut app, KeyCode::Enter, KeyModifiers::NONE);

    assert!(app.compose.screen == Screen::Pick && app.tab == Tab::Notes);

    key(&mut app, KeyCode::Down, KeyModifiers::NONE);
    key(&mut app, KeyCode::Enter, KeyModifiers::NONE);

    assert_eq!(app.note_list.selected, [] as [std::string::String; 0]);
    assert!(app.compose.screen == Screen::Compose && app.compose.field == Field::Notes);
    assert!(!app.message().contains("Read these notes first"));
}
