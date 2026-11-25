use super::*;

#[test]
fn briefs_parse_title_done_goal() {
    let (thread, directory, group) = parse("---\ntitle: UX improvements\ndone: true\n---\n\n# Goal\n\nless noise\n");

    assert_eq!(
        (thread.as_deref(), directory, group.as_str()),
        (Some("UX improvements"), true, "Goal")
    );

    let (thread_value, directory_value, group_value) = parse("plain text\nmore");

    assert_eq!(
        (thread_value, directory_value, group_value.as_str()),
        (None, false, "plain text")
    );

    let text = set_front("---\ntitle: X\n---\nbody\n", "done", Some("true"));

    assert_eq!(text, "---\ntitle: X\ndone: true\n---\nbody\n");
    assert_eq!(set_front(&text, "done", None), "---\ntitle: X\n---\nbody\n");
    assert_eq!(
        set_front("body\n", "done", Some("true")),
        "---\ndone: true\n---\nbody\n"
    );
}

#[test]
fn title_and_body_merge_into_the_brief() {
    let old = "---\ntitle: Old\ndone: true\n---\n\nGoal: x\n";

    assert_eq!(body_of(old), "Goal: x\n");
    assert_eq!(
        merge(old, " New ", "Goal: y"),
        "---\ndone: true\ntitle: New\n---\n\nGoal: y\n"
    );
    assert_eq!(merge("", "T", "b\n"), "---\ntitle: T\n---\n\nb\n");
}

#[test]
fn projects_live_in_the_home_repo_under_the_repos_dir() {
    let base = std::env::temp_dir().join(format!("ch-proj-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let home = base.join("home");

    std::fs::create_dir_all(&home).unwrap();
    std::fs::write(
        home.join("AGENTS.md"),
        "## Other repos\n\n- platform: core. `/abs/platform`. Notes: `notes/platform/`.\n",
    )
    .unwrap();

    assert_eq!(
        dir_in(Some(&home), &base, Path::new("/abs/platform")),
        home.join("notes/platform/projects")
    );
    assert_eq!(
        dir_in(Some(&home), &base, Path::new("/x/codehead")),
        home.join("codehead/projects")
    );
    assert_eq!(
        dir_in(None, &base.join("data"), Path::new("/x/codehead")),
        base.join("data/projects/codehead")
    );

    let directory = home.join("codehead/projects");
    let path = create_in(&directory, "Website refinement").unwrap();

    assert_eq!(
        (path.slug.as_str(), path.title.as_str(), path.done),
        ("website-refinement", "Website refinement", false)
    );
    assert!(create_in(&directory, "website refinement").is_err(), "same slug");

    create_in(&directory, "Bug fixes").unwrap();
    set_done(&path, true).unwrap();
    let all = list_in(&directory);

    assert_eq!(
        all.iter()
            .map(|path_value| (path_value.slug.as_str(), path_value.done))
            .collect::<Vec<_>>(),
        [("bug-fixes", false), ("website-refinement", true)]
    );
    assert_eq!(
        resolve_in(
            &all,
            &["Bug fixes".into(), "website-refinement".into(), "bug-fixes".into()]
        )
        .unwrap(),
        ["bug-fixes", "website-refinement"]
    );

    let error = resolve_in(&all, &["ux".into()]).unwrap_err();

    assert!(
        error.contains("ux") && error.contains("bug-fixes") && !error.contains("website-refinement"),
        "{error}"
    );

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn naming_rules() {
    assert!(check_title("menu: live quick commands").is_ok());
    assert!(check_title("live quick commands").is_err(), "0 area");
    assert!(check_title("misc: improvements #8").is_err());
    assert!(check_title("ui: misc. improvements").is_err());
    assert!(check_title("ui: tweaks 2").is_err());
    assert!(check_title(&format!("ui: {}", "x".repeat(80))).is_err());
    assert!(check_branch("feat/menu-live-commands").is_ok());
    assert!(check_branch("task/menu-live").is_err());
    assert!(check_branch("feat/Menu_live").is_err());
    assert!(check_branch("feat/menu--live").is_err());
    assert_eq!(kind_of("fix/nav-crash"), Some("fix"));
    assert_eq!(kind_of("task/x"), None);
    assert_eq!(area_of("menu: live quick commands").as_deref(), Some("menu"));
    assert_eq!(area_of("fix the menu"), None);
    assert_eq!(area_of("a b: c"), None);
}

#[test]
fn rename_branch_moves_the_branch_and_its_upstream() {
    use std::process::{Command, Stdio};
    let base = std::env::temp_dir().join(format!("ch-rename-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);

    std::fs::create_dir_all(&base).unwrap();

    let sh = |character: &str| {
        assert!(
            Command::new("sh")
                .arg("-c")
                .arg(character)
                .current_dir(&base)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .unwrap()
                .success(),
            "{character}"
        );
    };

    sh(
        "git init -q --bare -b main remote.git && git clone -q remote.git r 2>/dev/null; cd r && git -c user.email=t@t -c user.name=t commit -q --allow-empty -m 1 && git push -q origin main && git worktree add -q -b task/x ../w && cd ../w && git push -q -u origin task/x",
    );
    let wt = base.join("w");

    assert!(rename_branch(&wt, "main", "feat/y").is_err(), "default branch");

    let out = rename_branch(&wt, "task/x", "feat/menu-y").unwrap();

    assert!(out.contains("task/x → feat/menu-y"), "{out}");
    assert_eq!(crate::domain::threads::current_branch(&wt), "feat/menu-y");

    // 0 GitHub: the new name pushed, its upstream set
    let up = crate::domain::threads::git(&wt, &["rev-parse", "--abbrev-ref", "feat/menu-y@{upstream}"]).1;

    assert_eq!(up.trim(), "origin/feat/menu-y");
    assert!(
        rename_branch(&wt, "feat/menu-y", "feat/menu-y")
            .unwrap()
            .contains("unchanged")
    );

    let _ = std::fs::remove_dir_all(&base);
}
