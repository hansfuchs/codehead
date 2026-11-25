use super::sources::notes_of;
use super::*;

#[test]
fn agent_files_come_from_write_and_edit_calls_once() {
    let _env = crate::engine::ENV_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let thread = store::Thread::new("f", "t", Path::new("/w"), vec!["ada".into()]);

    std::fs::create_dir_all(store::thread_dir(&thread.id)).unwrap();
    thread.save().unwrap();

    let tool = |name: &str, input: serde_json::Value| {
        store::Event::new(
            Some("ada"),
            Kind::Tool {
                id: "1".into(),
                name: name.into(),
                input,
            },
        )
    };

    for error_value in [
        tool("Write", serde_json::json!({"file_path": "/w/notes/plan.md"})),
        tool("Edit", serde_json::json!({"file_path": "src/a.rs"})),
        tool("Read", serde_json::json!({"file_path": "/w/README.md"})),
        tool("Edit", serde_json::json!({"file_path": "/w/notes/plan.md"})),
    ] {
        store::append(&thread.id, &error_value).unwrap();
    }

    assert_eq!(
        agent_files(&thread.id, Path::new("/w")),
        [PathBuf::from("/w/notes/plan.md"), PathBuf::from("/w/src/a.rs")]
    );

    let _ = std::fs::remove_dir_all(store::thread_dir(&thread.id));
}

#[test]
fn notes_of_a_thread_by_their_1st_line() {
    let directory = std::env::temp_dir().join(format!("codehead-notes-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&directory);

    std::fs::create_dir_all(directory.join("area/plans")).unwrap();
    std::fs::write(
        directory.join("area/plans/a.md"),
        "thread: abc123 · branch: main · status: open\n# A",
    )
    .unwrap();
    std::fs::write(directory.join("area/plans/b.md"), "thread: other · branch: main\n").unwrap();
    std::fs::write(directory.join("area/INDEX.md"), "- plans/a.md\n").unwrap();

    assert_eq!(notes_of(&directory, "abc123"), [directory.join("area/plans/a.md")]);

    let _ = std::fs::remove_dir_all(directory);
}

#[test]
fn share_commits_only_the_threads_home_files() {
    let directory = std::env::temp_dir().join(format!("codehead-share-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&directory);

    std::fs::create_dir_all(directory.join("notes")).unwrap();

    let sh = |character: &str| {
        assert!(
            std::process::Command::new("sh")
                .arg("-c")
                .arg(character)
                .current_dir(&directory)
                .status()
                .unwrap()
                .success(),
            "{character}"
        );
    };

    sh(
        "git init -q && git config user.email t@t && git config user.name t && echo x > other.md && git add -A && git commit -qm init",
    );
    std::fs::write(directory.join("notes/plan.md"), "thread: t1\n").unwrap();
    std::fs::write(directory.join("other.md"), "changed\n").unwrap();
    let path = share(
        &directory,
        &[directory.join("notes/plan.md"), PathBuf::from("/elsewhere/x.rs")],
        "fix deploy",
    )
    .unwrap();

    assert_eq!(path, "home repo: 1 files committed (0 upstream: 0 push)");

    let log = std::process::Command::new("git")
        .arg("-C")
        .arg(&directory)
        .args(["show", "--stat", "--format=%s", "HEAD"])
        .output()
        .unwrap();
    let log = String::from_utf8_lossy(&log.stdout);

    assert!(
        log.starts_with("notes: fix deploy") && log.contains("notes/plan.md") && !log.contains("other.md"),
        "{log}"
    );
    assert_eq!(
        share(&directory, &[directory.join("notes/plan.md")], "x").unwrap(),
        "home repo: 0 changes to share"
    );

    let _ = std::fs::remove_dir_all(directory);
}

#[test]
fn yazi_runs_with_codeheads_config_and_a_chooser_file() {
    let (argv, env, chooser) = yazi_cmd(Path::new("/w/src"));

    assert_eq!(argv[..2], ["yazi".to_string(), "/w/src".to_string()]);
    assert!(argv[2].starts_with("--chooser-file=") && argv[2].ends_with(&chooser.display().to_string()));
    assert_eq!(env[0].0, "YAZI_CONFIG_HOME");

    let dir = PathBuf::from(&env[0].1);

    assert!(
        std::fs::read_to_string(dir.join("theme.toml"))
            .unwrap()
            .contains("[mgr]")
    );
}
