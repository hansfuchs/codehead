use super::*;

#[test]
fn thread_return_keeps_cached_state_and_queues_background_refresh() {
    let mut app = test_app();

    app.tab = Tab::Threads;
    app.compose.prompt.set_text("retained draft");
    app.overlays.last_esc = Some((Instant::now(), false));
    let files = app.note_list.notes.files.clone();
    let agents = app
        .agent_list
        .agents
        .iter()
        .map(|agent| agent.name.clone())
        .collect::<Vec<_>>();

    app.return_from_thread();

    assert_eq!(app.tab, Tab::Threads);
    assert_eq!(app.compose.prompt.text(), "retained draft");
    assert_eq!(app.note_list.notes.files, files);
    assert_eq!(
        app.agent_list
            .agents
            .iter()
            .map(|agent| agent.name.clone())
            .collect::<Vec<_>>(),
        agents
    );
    assert!(app.overlays.last_esc.is_none());
    assert!(app.thread_list.worktrees.is_some());
    assert_eq!(app.thread_list.thread_reads.pending(), 1);
    assert_eq!(app.agent_list.agent_reads.pending(), 1);
}

#[test]
fn the_repo_setting_starts_the_where_rows_on_its_repo() {
    let base = std::env::temp_dir().join(format!("ch-default-repo-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);

    let mk = |name: &str| {
        let repo = base.join(name);

        std::fs::create_dir_all(&repo).unwrap();

        assert!(std::process::Command::new("sh").arg("-c").arg("git init -q -b main && git config user.email t@t && git config user.name t && git commit -q --allow-empty -m i && mkdir sub").current_dir(&repo).status().unwrap().success());

        repo
    };

    let (start, other) = (mk("start"), mk("other"));
    let mut app = test_app();

    app.repository.start_cwd = start.clone();
    app.set_repo(crate::screens::start_screen::repo_pick::Entry {
        name: "start".into(),
        path: start.clone(),
        git: true,
        clone: None,
    });

    // a dir in the start repo: 0 change, the current branch stays first
    app.use_default_repo(&start.join("sub"));

    assert_eq!(
        (app.cfg.repo.as_deref(), app.repository.where_i),
        (Some(start.as_path()), 0)
    );

    // a dir in another repo: that repo, a new worktree first
    app.use_default_repo(&other.join("sub"));

    assert_eq!(app.cfg.repo.as_deref(), Some(other.as_path()));
    assert_eq!(
        app.repository.wheres.get(app.repository.where_i),
        Some(&Where::NewWorktree)
    );

    // codehead started outside of a git repo: the setting's repo all the same
    let mut fresh = test_app();

    fresh.repository.start_cwd = base.clone();
    fresh.cfg.repo = None;
    fresh.use_default_repo(&other);

    assert_eq!(
        (
            fresh.repository.here_branch.as_str(),
            fresh.repository.wheres.get(fresh.repository.where_i)
        ),
        ("main", Some(&Where::NewWorktree))
    );

    // 0 git repo: 0 change
    app.use_default_repo(&base);

    assert_eq!(app.cfg.repo.as_deref(), Some(other.as_path()));

    let _ = std::fs::remove_dir_all(base);
}

#[test]
fn repo_field_picks_a_repo_in_2_panes_and_a_missing_one_gets_cloned_first() {
    let base = std::env::temp_dir().join(format!("ch-repos-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);

    let mk = |name: &str| {
        let repo = base.join(name);

        std::fs::create_dir_all(&repo).unwrap();

        assert!(std::process::Command::new("sh").arg("-c").arg("git init -q -b main && git config user.email t@t && git config user.name t && echo x > a && git add -A && git commit -qm i && git branch feat && mkdir -p sub/deep").current_dir(&repo).status().unwrap().success());

        repo
    };

    let (start, other) = (mk("start"), mk("other"));
    let mut app = test_app();

    app.repository.start_cwd = start.clone();
    app.set_repo(crate::screens::start_screen::repo_pick::Entry {
        name: "start".into(),
        path: start.clone(),
        git: true,
        clone: None,
    });
    app.repository.repos = vec![crate::infra::home::Repo {
        name: "gone".into(),
        path: base.join("gone"),
        url: Some("git@x:y/gone.git".into()),
    }];
    app.compose.screen = Screen::Compose;
    app.compose.title.set_text("fix it");

    // ↓ from the title: the repo row, the projects row, then the branch row (0 field skipped)
    app.compose.field = Field::Title;
    key(&mut app, KeyCode::Down, KeyModifiers::NONE);

    assert!(app.compose.field == Field::Repo);

    key(&mut app, KeyCode::Down, KeyModifiers::NONE);

    assert!(app.compose.field == Field::Projects);

    key(&mut app, KeyCode::Down, KeyModifiers::NONE);

    assert!(app.compose.field == Field::Where);

    // r on the branch row: 0 repo switch (the hotkey is gone)
    key(&mut app, KeyCode::Char('r'), KeyModifiers::NONE);

    assert_eq!(app.cfg.repo.as_deref(), Some(start.as_path()));

    key(&mut app, KeyCode::Up, KeyModifiers::NONE);
    key(&mut app, KeyCode::Up, KeyModifiers::NONE);

    // enter: the picker on start; left: base's dirs (gone: a clone), right: start's dirs
    key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    let out = dump(&mut app, 120, 40);

    assert_eq!(
        app.repository
            .repo_pick
            .as_ref()
            .and_then(|picker| picker.current())
            .map(|entry| entry.path.clone()),
        Some(start.clone()),
        "{out}"
    );
    assert!(
        out.lines().any(|line| line.contains("start/") && line.contains("git")),
        "{out}"
    );
    assert!(
        out.lines()
            .any(|line| line.contains("gone/") && line.contains("│ sub/")),
        "{out}"
    );

    repo_field_picks_a_repo_in_2_panes_and_a_missing_one_gets_cloned_first_verify_state(
        base, &start, &other, app, &out,
    );
}

fn repo_field_picks_a_repo_in_2_panes_and_a_missing_one_gets_cloned_first_verify_state(
    base: PathBuf,
    start: &std::path::Path,
    other: &std::path::Path,
    mut app: App,
    out: &str,
) {
    assert!(
        out.lines().any(|line| line.contains("gone/") && line.contains("clone")),
        "{out}"
    );

    println!("--- repo picker\n{out}");

    // ↑ (other) + enter: other, a new worktree first
    key(&mut app, KeyCode::Up, KeyModifiers::NONE);
    key(&mut app, KeyCode::Enter, KeyModifiers::NONE);

    assert!(app.repository.repo_pick.is_none());
    assert_eq!(app.cfg.repo.as_deref(), Some(other));
    assert_eq!(
        app.repository.wheres.get(app.repository.where_i),
        Some(&Where::NewWorktree),
        "another repo: a new worktree first"
    );
    assert!(app.repository.wheres.contains(&Where::Branch("feat".into())));
    assert!(app.clone_first().is_none());

    // a dir in a repo (→ into other, ↓ to sub): its repo
    key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    key(&mut app, KeyCode::Right, KeyModifiers::NONE);

    assert_eq!(
        app.repository
            .repo_pick
            .as_ref()
            .and_then(|picker| picker.current())
            .map(|entry| entry.name.clone())
            .as_deref(),
        Some("sub")
    );

    key(&mut app, KeyCode::Enter, KeyModifiers::NONE);

    assert_eq!(app.cfg.repo.as_deref(), Some(other));

    // the picker on the repo (other, 0 sub), ↑ to gone: cloned first
    key(&mut app, KeyCode::Enter, KeyModifiers::NONE);

    assert_eq!(
        app.repository
            .repo_pick
            .as_ref()
            .and_then(|picker| picker.current())
            .map(|entry| entry.name.clone())
            .as_deref(),
        Some("other")
    );

    key(&mut app, KeyCode::Up, KeyModifiers::NONE);
    key(&mut app, KeyCode::Enter, KeyModifiers::NONE);

    assert_eq!(app.repository.wheres, [Where::NewWorktree]);
    assert_eq!(
        app.clone_first(),
        Some(("git@x:y/gone.git".to_string(), base.join("gone")))
    );
    repo_field_picks_a_repo_in_2_panes_and_a_missing_one_gets_cloned_first_verify_layout(base, start, app);
}

fn repo_field_picks_a_repo_in_2_panes_and_a_missing_one_gets_cloned_first_verify_layout(
    base: PathBuf,
    start: &std::path::Path,
    mut app: App,
) {
    assert!(
        app.where_text().starts_with("clones git@x:y/gone.git into"),
        "{}",
        app.where_text()
    );

    // a dir outside of a git repo: a toast, the repo stays
    key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    key(&mut app, KeyCode::Left, KeyModifiers::NONE);
    key(&mut app, KeyCode::Enter, KeyModifiers::NONE);

    assert!(app.repository.repo_pick.is_some() && app.clone_first().is_some());

    key(&mut app, KeyCode::Esc, KeyModifiers::NONE);

    assert!(app.repository.repo_pick.is_none() && app.outcome.is_none());

    // the start repo again: the start dir, current branch first
    key(&mut app, KeyCode::Enter, KeyModifiers::NONE);

    for _ in 0..2 {
        key(&mut app, KeyCode::Down, KeyModifiers::NONE);
    }

    key(&mut app, KeyCode::Enter, KeyModifiers::NONE);

    assert_eq!(
        (app.cfg.repo.as_deref(), app.repository.where_i, app.cfg.cwd.clone()),
        (Some(start), 0, start.to_path_buf())
    );

    // "where" a label; the repo row and the branch row below it, the boxes in their column
    let text = dump(&mut app, 120, 40);

    let at = |pat: &str| {
        text.lines()
            .position(|line| line.contains(pat))
            .unwrap_or_else(|| panic!("{pat}\n{text}"))
    };

    let col = |pat: &str| {
        text.lines()
            .find_map(|line| line.find(pat).map(|byte| line[..byte].chars().count()))
            .unwrap()
    };

    assert!(text.lines().any(|line| line.trim() == "where"), "{text}");
    assert_eq!(
        (at(" repo "), at(" project "), at(" branch ")),
        (at("where") + 1, at("where") + 2, at("where") + 3),
        "{text}"
    );
    assert!(
        text.lines()
            .any(|line| line.contains(" repo ") && line.contains(" start ")),
        "{text}"
    );
    assert_eq!(col("repo"), col("branch"), "{text}");
    assert_eq!(col("checkout selected"), col("branch") + 2, "{text}");

    let _ = std::fs::remove_dir_all(base);
}

#[test]
fn outside_of_a_git_repo_the_repo_row_stays_in_the_field_order() {
    let mut app = test_app();

    app.cfg.repo = None;
    app.repository.repo = None;
    app.repository.wheres.clear();
    app.compose.screen = Screen::Compose;
    app.compose.field = Field::Title;
    key(&mut app, KeyCode::Down, KeyModifiers::NONE);

    assert!(app.compose.field == Field::Repo);

    key(&mut app, KeyCode::Down, KeyModifiers::NONE);

    assert!(app.compose.field == Field::Agents);

    let text = dump(&mut app, 120, 40);

    assert!(
        text.contains("0 git repo") && text.contains("enter: pick a repo"),
        "{text}"
    );
}
