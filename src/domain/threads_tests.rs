use super::*;

/// A worktree thread: archive removes its worktree (uncommitted files → stash), keeps the
/// branch, remembers it for the next host start; unarchive opens it again.
#[test]
fn archive_removes_the_worktree_keeps_the_branch() {
    let base = std::env::temp_dir().join(format!("ch-archive-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);

    std::fs::create_dir_all(&base).unwrap();

    let sh = |script: &str| {
        assert!(
            Command::new("sh")
                .arg("-c")
                .arg(script)
                .current_dir(&base)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .unwrap()
                .success(),
            "{script}"
        );
    };

    sh(
        "git init -q -b main r && cd r && git -c user.email=t@t -c user.name=t commit -q --allow-empty -m 1 && git worktree add -q -b feat ../w && echo x > ../w/new.txt",
    );
    let repo = base.join("r");
    let wt = base.join("w");
    let mut thread = store::Thread::new("archive me", "task", &repo, vec!["ada".into()]);

    thread.worktree = Some(wt.display().to_string());
    thread.branch = Some("feat".into());
    thread.port = Some(4100);
    thread.save().unwrap();

    let out = archive(&thread.id).unwrap();

    assert!(out.contains("worktree removed") && out.contains("git stash"), "{out}");
    assert!(!wt.exists());
    assert!(
        git(&repo, &["rev-parse", "--verify", "--quiet", "refs/heads/feat"]).0,
        "branch kept"
    );

    // git forgot the worktree: its branch takes a new one (the unarchive's)
    assert!(
        !git(&repo, &["worktree", "list"]).1.contains("/w "),
        "{}",
        git(&repo, &["worktree", "list"]).1
    );

    verify_archive_removes_the_worktree_keeps_the_branch(base, &repo, &thread);
}

fn verify_archive_removes_the_worktree_keeps_the_branch(base: PathBuf, repo: &PathBuf, thread: &store::Thread) {
    let again = base.join("w2");

    assert!(
        git(repo, &["worktree", "add", "-q", &again.display().to_string(), "feat"]).0,
        "feat: free for a new worktree"
    );

    let stored = store::load(&thread.id).unwrap();

    assert_eq!(
        (
            stored.state.as_str(),
            stored.worktree.as_deref(),
            stored.worktree_pending.as_deref(),
            stored.port
        ),
        ("archived", None, Some("feat"), None)
    );
    assert_eq!(
        stored
            .repo
            .as_deref()
            .map(PathBuf::from)
            .and_then(|path| std::fs::canonicalize(path).ok()),
        std::fs::canonicalize(repo).ok()
    );
    assert!(
        store::latest().is_none_or(|latest| latest.id != thread.id),
        "plain codehead: 0 archived thread"
    );

    // unarchive: open again; 0 host binary in tests, the record is what counts
    let _ = unarchive(&thread.id);

    assert_eq!(store::load(&thread.id).unwrap().state, "open");

    let _ = store::delete(&thread.id);
    let _ = std::fs::remove_dir_all(base);
}

/// The edit: title, a repo override (the repo of its dir: 0 override), linked repos kept.
#[test]
fn edit_sets_title_repo_and_linked_repos() {
    let dir = std::env::temp_dir().join(format!("ch-edit-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let mut thread = store::Thread::new("old", "task", &dir, vec!["ada".into()]);

    thread.checkouts = vec![
        store::Checkout {
            dir: "/a".into(),
            branch: None,
        },
        store::Checkout {
            dir: "/b".into(),
            branch: None,
        },
    ];
    thread.save().unwrap();
    let form = Edit {
        title: "new title".into(),
        repo: Some("/repos/other".into()),
        checkouts: vec!["/b".into()],
        agents: vec!["ada".into()],
        projects: Some(vec!["ux".into()]),
    };
    let out = edit(&thread.id, &form).unwrap();

    assert!(
        out.contains("title") && out.contains("repo") && out.contains("linked repos") && out.contains("projects"),
        "{out}"
    );

    let stored = store::load(&thread.id).unwrap();

    assert_eq!(
        (stored.title.as_str(), stored.repo.as_deref(), stored.checkouts.len()),
        ("new title", Some("/repos/other"), 1)
    );

    // the owner's title and projects: the agents' tools leave them
    assert!(stored.owner_choices.title_set && stored.owner_choices.projects_set && stored.projects == ["ux"]);
    assert!(
        crate::domain::projects::set_title(&thread.id, "menu: other name", None)
            .unwrap()
            .contains("title kept")
    );
    assert!(
        crate::domain::projects::set_projects(&thread.id, &[])
            .unwrap()
            .contains("projects kept")
    );

    // projects back to auto
    verify_edit_sets_title_repo_and_linked_repos(dir, &thread, form, &stored);
}

fn verify_edit_sets_title_repo_and_linked_repos(
    dir: PathBuf,
    thread: &store::Thread,
    form: Edit,
    stored: &store::Thread,
) {
    let out = edit(
        &thread.id,
        &Edit {
            projects: None,
            ..Edit {
                title: "new title".into(),
                repo: Some("/repos/other".into()),
                checkouts: vec!["/b".into()],
                agents: vec!["ada".into()],
                projects: None,
            }
        },
    )
    .unwrap();

    assert!(out.contains("projects") && !out.contains("title"), "{out}");
    assert!(!store::load(&thread.id).unwrap().owner_choices.projects_set);

    let lite = Thread {
        repo: stored.repo.clone(),
        dir: Some(stored.dir.clone()),
        ..Default::default()
    };

    assert_eq!(repo_of_thread(&lite), Some(PathBuf::from("/repos/other")));
    assert!(
        edit(
            &thread.id,
            &Edit {
                agents: Vec::new(),
                ..form
            }
        )
        .is_err(),
        "0 agents refused"
    );

    let _ = store::delete(&thread.id);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_start_on_main_pulls_main_first_other_branches_not() {
    let base = std::env::temp_dir().join(format!("ch-update-main-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);

    std::fs::create_dir_all(&base).unwrap();

    let sh = |script: &str| {
        assert!(
            Command::new("sh")
                .arg("-c")
                .arg(script)
                .current_dir(&base)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .unwrap()
                .success(),
            "{script}"
        );
    };

    let cfg = "git config user.email t@t && git config user.name t";

    sh(&format!(
        "git init -q --bare -b main origin.git && git clone -q origin.git a && cd a && {cfg} && git commit -q --allow-empty -m 1 && git push -q origin main"
    ));
    sh(&format!(
        "git clone -q origin.git b && cd b && {cfg} && git commit -q --allow-empty -m 2 && git push -q origin main"
    ));
    let checkout = base.join("a");
    let head = |path: &Path| git(path, &["rev-parse", "HEAD"]).1;

    assert_ne!(head(&checkout), head(&base.join("b")));
    assert!(matches!(update_default(&checkout), Some(Ok(branch)) if branch == "main"));
    assert_eq!(head(&checkout), head(&base.join("b")), "main pulled");

    // another branch: 0 pull; a branch without upstream: 0 pull
    sh("cd a && git switch -q -c feat");

    assert!(update_default(&checkout).is_none());

    sh("cd a && git switch -q main && git branch -q --unset-upstream");

    assert!(update_default(&checkout).is_none());

    // diverged: Err with git's line
    sh(
        "cd a && git branch -q -u origin/main && git commit -q --allow-empty -m local && cd ../b && git commit -q --allow-empty -m 3 && git push -q origin main",
    );

    assert!(matches!(update_default(&checkout), Some(Err(error)) if !error.is_empty()));

    let _ = std::fs::remove_dir_all(base);
}

/// The file system read of the checkouts: the same map as `git worktree list`, from the main
/// checkout and from a linked worktree (a detached worktree: 0 entry in both).
#[test]
fn checkouts_by_the_file_system_match_git() {
    let base = std::env::temp_dir().join(format!("codehead-checkouts-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);

    std::fs::create_dir_all(&base).unwrap();

    let sh = |script: &str| {
        assert!(
            std::process::Command::new("sh")
                .arg("-c")
                .arg(script)
                .current_dir(&base)
                .output()
                .unwrap()
                .status
                .success(),
            "{script}"
        );
    };

    sh(
        "git init -q -b main r && cd r && git -c user.email=t@t -c user.name=t commit -q --allow-empty -m 1 && git worktree add -q -b feat ../w && git worktree add -q --detach ../d",
    );

    for dir in [base.join("r"), base.join("w"), base.join("r/sub")] {
        let _ = std::fs::create_dir_all(&dir);
        let fs = checkouts_fs(&dir).expect("fs read");

        let canon = |map: std::collections::HashMap<String, String>| {
            map.into_iter()
                .map(|(branch, path)| (branch, std::fs::canonicalize(path).unwrap()))
                .collect::<std::collections::BTreeMap<_, _>>()
        };

        assert_eq!(canon(fs.clone()), canon(checkouts_git(&dir)), "{}", dir.display());
        assert_eq!(fs.len(), 2);
    }

    assert!(checkouts_fs(&base).is_none(), "0 repo");

    let _ = std::fs::remove_dir_all(base);
}
