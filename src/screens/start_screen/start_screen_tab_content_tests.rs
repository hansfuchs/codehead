use super::*;

/// Repo tabs over cards and this screen's rows; the claude view: the sessions of the repo.
#[test]
fn threads_tab_repo_tabs_and_the_claude_view() {
    let mut app = test_app();

    app.thread_list.worktrees = Some(Vec::new());
    app.thread_list.sessions = vec![
        Session {
            id: "s1".into(),
            title: "claude work".into(),
            cwd: "/x/alpha".into(),
            repo: Some("/x/alpha".into()),
            ..Default::default()
        },
        Session {
            id: "s2".into(),
            title: "beta work".into(),
            cwd: "/x/beta".into(),
            repo: Some("/x/beta".into()),
            ..Default::default()
        },
    ];
    app.thread_list.tlist.set_cards(vec![
        card("one", |card| card.repo = Some("/x/alpha".into())),
        card("two", |card| card.repo = Some("/x/beta".into())),
        card("three", |card| card.repo = Some("/x/beta".into())),
    ]);
    app.switch_tab(Tab::Threads);
    app.tabs_focus = false;
    let text = dump(&mut app, 120, 40);
    let row = text.lines().find(|line| line.contains("repo ")).expect("repo tabs");

    assert!(
        row.contains(" beta ") && row.contains(" alpha ") && row.contains(" all "),
        "{row}"
    );

    // →: the 1st repo (beta: most rows), →: alpha
    key(&mut app, KeyCode::Right, KeyModifiers::NONE);

    assert_eq!(app.thread_list.tlist.visible().len(), 2);

    key(&mut app, KeyCode::Right, KeyModifiers::NONE);

    assert_eq!(app.thread_list.tlist.visible().len(), 1);

    // ↑: the status tabs, ↑: the view tabs; →→→: claude; ↓: its rows (alpha's session only)
    key(&mut app, KeyCode::Up, KeyModifiers::NONE);

    assert_eq!(app.thread_list.tlist.focus, crate::features::thread_list::Focus::Status);

    key(&mut app, KeyCode::Up, KeyModifiers::NONE);

    for _ in 0..3 {
        key(&mut app, KeyCode::Right, KeyModifiers::NONE);
    }

    key(&mut app, KeyCode::Down, KeyModifiers::NONE);

    assert_eq!(app.thread_list.tlist.view, crate::features::thread_list::View::Claude);
    assert_eq!(app.n_threads(), 1);
    assert_eq!(
        app.current_session().map(|session| session.id.clone()),
        Some("s1".to_string())
    );

    let claude_view = dump(&mut app, 120, 40);

    assert!(
        claude_view.contains("claude work") && !claude_view.contains("beta work"),
        "{claude_view}"
    );

    // ↑ on its top row: the view tabs again
    key(&mut app, KeyCode::Up, KeyModifiers::NONE);

    assert_eq!(app.thread_list.tlist.focus, crate::features::thread_list::Focus::Views);
}

/// The preview of a card: the full read of its thread (title, not the dir).
#[test]
fn a_card_preview_uses_the_threads_full_read() {
    let mut app = test_app();
    let worktree = Worktree {
        path: "/project".into(),
        branch: "in place".into(),
        linked: false,
        unpushed: 0,
        when: "1m".into(),
        repo: None,
        thread: Some(threads::Thread {
            id: "a1".into(),
            title: "fix latency".into(),
            state: "open".into(),
            live: 1,
            updated: 0,
            worktree: None,
            branch: None,
            dir: Some("/project".into()),
            ..Default::default()
        }),
    };

    app.thread_list
        .tpreview
        .insert(worktree.key(), crate::ui::worktree_preview::preview_header(&worktree));
    app.thread_list.worktrees = Some(vec![worktree]);
    app.thread_list
        .tlist
        .set_cards(vec![card("a1", |card| card.title = "fix latency".into())]);
    app.switch_tab(Tab::Threads);
    let mut terminal = Terminal::new(TestBackend::new(100, 36)).unwrap();

    terminal.draw(|frame| app.draw(frame)).unwrap();
    let buffer = terminal.backend().buffer();
    let line = |y| (0..100).map(|x| buffer[(x, y)].symbol()).collect::<String>();

    assert!(line(app.preview.regions.preview.y).contains("fix latency"));
    assert!(!line(app.preview.regions.preview.y).contains("in place"));
}

/// The projects tab: the repo's briefs, threads per project; the threads tab: a project row
/// below the repo row filters the cards.
#[test]
fn the_projects_tab_lists_briefs_and_the_threads_filter_by_project() {
    let _g = crate::engine::ENV_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut app = test_app();
    let repo = std::env::temp_dir()
        .join(format!("ch-app-proj-{}", std::process::id()))
        .join("shop");
    let dir = crate::domain::projects::dir(&repo);
    let _ = std::fs::remove_dir_all(&dir);

    crate::domain::projects::create_in(&dir, "Website refinement").unwrap();
    crate::domain::projects::create_in(&dir, "Bug fixes").unwrap();
    app.cfg.repo = Some(repo.clone());
    let card = |id: &str, ps: &[&str]| crate::ui::thread_card::Card {
        id: id.into(),
        title: format!("ui: {id}"),
        state: "open".into(),
        repo: Some(repo.clone()),
        projects: ps.iter().map(std::string::ToString::to_string).collect(),
        ..Default::default()
    };

    app.thread_list.tlist.set_cards(vec![
        card("one", &["website-refinement"]),
        card("two", &["website-refinement", "bug-fixes"]),
        card("three", &[]),
    ]);
    key(&mut app, KeyCode::Char('3'), KeyModifiers::ALT);

    assert_eq!(app.tab, Tab::Projects);

    let out = dump(&mut app, 110, 20);

    assert!(
        out.contains("Website refinement   2 threads") && out.contains("Bug fixes   1 thread"),
        "{out}"
    );

    println!("--- projects\n{out}");
    key(&mut app, KeyCode::Char('2'), KeyModifiers::ALT);
    let out = dump(&mut app, 110, 30);

    assert!(
        out.contains("project") && out.contains("website-refinement") && out.contains("0 projects"),
        "{out}"
    );
    assert_eq!(app.thread_list.tlist.visible().len(), 3);

    // the project row: → the most used project
    app.thread_list.tlist.focus = crate::features::thread_list::Focus::Projects;
    key(&mut app, KeyCode::Right, KeyModifiers::NONE);

    assert_eq!(
        app.thread_list.tlist.proj_tab(),
        crate::features::thread_list::ProjTab::Project("website-refinement".into())
    );
    assert_eq!(app.thread_list.tlist.visible().len(), 2);

    println!("--- threads by project\n{}", dump(&mut app, 110, 30));
    let _ = std::fs::remove_dir_all(&dir);
}

/// The new form's project row: a dropdown with a filter and the brief beside it; a project
/// made after the 1st read shows at once.
#[test]
fn the_project_dropdown_filters_shows_the_brief_and_sees_new_projects() {
    let _g = crate::engine::ENV_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut app = test_app();
    let repo = std::env::temp_dir()
        .join(format!("ch-app-projdd-{}", std::process::id()))
        .join("shop");
    let dir = crate::domain::projects::dir(&repo);
    let _ = std::fs::remove_dir_all(&dir);

    crate::domain::projects::create_in(&dir, "Website refinement").unwrap();
    app.cfg.repo = Some(repo.clone());
    app.compose.screen = Screen::Compose;

    assert_eq!(app.task_projects().len(), 1);

    let path = crate::domain::projects::create_in(&dir, "Bug fixes").unwrap();

    crate::engine::write_atomic(
        &path.path,
        crate::domain::projects::merge("", "Bug fixes", "Goal: 0 crashes").as_bytes(),
    )
    .unwrap();

    assert_eq!(app.task_projects().len(), 2, "a new brief: read again");

    app.compose.field = Field::Projects;
    key(&mut app, KeyCode::Enter, KeyModifiers::NONE);

    assert!(app.project_selection.proj_menu.is_some());

    for letter in "bug".chars() {
        key(&mut app, KeyCode::Char(letter), KeyModifiers::NONE);
    }

    let out = dump(&mut app, 130, 40);

    println!("--- project dropdown\n{out}");

    assert!(
        out.contains("project › bug") && out.contains("Goal: 0 crashes"),
        "{out}"
    );
    assert!(!out.contains("◻ Website refinement"), "{out}");

    key(&mut app, KeyCode::Enter, KeyModifiers::NONE);

    assert!(app.project_selection.proj_menu.is_none());
    assert_eq!(app.project_selection.proj_picks, Some(vec!["bug-fixes".to_string()]));

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_flows_tab_draws_the_rail_with_its_arcs() {
    let mut app = test_app();

    key(&mut app, KeyCode::Char('6'), KeyModifiers::ALT);

    assert_eq!(app.tab, Tab::Flows);

    key(&mut app, KeyCode::Down, KeyModifiers::NONE);
    key(&mut app, KeyCode::Up, KeyModifiers::NONE);
    key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    let screen = dump(&mut app, 130, 30);

    println!("--- flows\n{screen}");

    assert!(
        screen.contains(" flows ") && screen.contains("new feature") && screen.contains("plan approval"),
        "{screen}"
    );
    assert!(
        screen.contains("◀") && screen.contains("changes") && screen.contains("↺"),
        "{screen}"
    );

    key(&mut app, KeyCode::Char('a'), KeyModifiers::NONE);

    for letter in "rev".chars() {
        key(&mut app, KeyCode::Char(letter), KeyModifiers::NONE);
    }

    let screen = dump(&mut app, 130, 30);

    println!("--- picker\n{screen}");

    assert!(
        screen.contains("one-off: rev") && screen.contains("peer-review"),
        "{screen}"
    );
}

#[test]
fn screens() {
    let mut app = test_app();

    app.compose.field = Field::Title;

    for letter in "Fix deploy".chars() {
        key(&mut app, KeyCode::Char(letter), KeyModifiers::NONE);
    }

    // title → repo → flow → agents → notes, enter: the notes picker
    for _ in 0..4 {
        key(&mut app, KeyCode::Down, KeyModifiers::NONE);
    }

    key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    println!("--- notes picker\n{}", dump(&mut app, 130, 16));
    key(&mut app, KeyCode::Down, KeyModifiers::NONE);
    key(&mut app, KeyCode::Char(' '), KeyModifiers::NONE);
    key(&mut app, KeyCode::Char('p'), KeyModifiers::CONTROL);
    println!("--- menu\n{}", dump(&mut app, 100, 22));
    key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    app.compose.field = Field::Prompt;
    app.paste("line one\nline two");
    println!("--- compose\n{}", dump(&mut app, 100, 22));
    println!("--- message\n{}", app.message());
    key(&mut app, KeyCode::Char('2'), KeyModifiers::ALT);
    println!("--- threads\n{}", dump(&mut app, 120, 14));
    key(&mut app, KeyCode::Char('3'), KeyModifiers::NONE);

    assert!(app.tab == Tab::Threads, "0 number keys for tabs");

    key(&mut app, KeyCode::Char('5'), KeyModifiers::ALT);
    println!("--- agents\n{}", dump(&mut app, 120, 14));
    key(&mut app, KeyCode::Char('n'), KeyModifiers::NONE);
    println!("--- new agent\n{}", dump(&mut app, 120, 42));

    for _ in 0..F_SPRITE {
        key(&mut app, KeyCode::Tab, KeyModifiers::NONE);
    }

    key(&mut app, KeyCode::Char('r'), KeyModifiers::NONE);
    println!("--- sprite field\n{}", dump(&mut app, 120, 42));

    // an edited form: esc asks first
    key(&mut app, KeyCode::Esc, KeyModifiers::NONE);

    assert!(matches!(app.overlays.modal, Some(Modal::Confirm { .. })));

    key(&mut app, KeyCode::Char('y'), KeyModifiers::NONE);

    assert!(app.overlays.modal.is_none());
    assert!(app.message().starts_with("Fix deploy\n\nRead these notes first:\n- "));
    assert!(app.message().ends_with("\nline one\nline two"));

    let _ = std::fs::remove_dir_all(&app.cfg.notes);
}
