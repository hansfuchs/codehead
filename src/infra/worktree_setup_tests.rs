use super::*;

fn sh(dir: &Path, script: &str) {
    assert!(
        Command::new("sh")
            .arg("-c")
            .arg(script)
            .current_dir(dir)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap()
            .success(),
        "{script}"
    );
}

fn repo(name: &str) -> PathBuf {
    let repo_dir = std::env::temp_dir().join(format!("codehead-setup-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&repo_dir);

    std::fs::create_dir_all(&repo_dir).unwrap();
    sh(
        &repo_dir,
        "git init -q && git config user.email t@t && git config user.name t && printf '.env*\\ncerts/\\n' > .gitignore && echo x > a.rs && git add -A && git commit -qm init",
    );
    repo_dir
}

#[test]
fn default_includes_are_the_ignored_env_files() {
    let repo_dir = repo("inc");

    sh(
        &repo_dir,
        "echo S=1 > .env && echo L=1 > .env.local && echo t > .envrc.tracked && git add -f .envrc.tracked && git commit -qm t",
    );

    assert_eq!(includes(&repo_dir), [".env", ".env.local"]);

    std::fs::write(repo_dir.join(".worktreeinclude"), "# local\n.env\ncerts/\n\n").unwrap();

    assert_eq!(includes(&repo_dir), [".env", "certs"]);

    let _ = std::fs::remove_dir_all(repo_dir);
}

#[test]
fn copy_includes_brings_ignored_files_and_dirs_into_the_worktree() {
    let repo_dir = repo("copy");

    sh(
        &repo_dir,
        "echo S=1 > .env && mkdir -p certs/a && echo k > certs/a/key.pem && printf '.env\\ncerts\\nmissing\\n' > .worktreeinclude",
    );
    let wt = repo_dir.join("wt");

    sh(&repo_dir, "git worktree add -q -b t wt");
    let copied = copy_includes(&repo_dir, &wt);

    assert_eq!(copied, [".env", "certs"]);
    assert_eq!(std::fs::read_to_string(wt.join(".env")).unwrap(), "S=1\n");
    assert!(wt.join("certs/a/key.pem").is_file());

    // a 2nd run: 0 copies over existing files
    std::fs::write(wt.join(".env"), "S=2\n").unwrap();

    assert_eq!(copy_includes(&repo_dir, &wt), [] as [std::string::String; 0]);
    assert_eq!(std::fs::read_to_string(wt.join(".env")).unwrap(), "S=2\n");

    let _ = std::fs::remove_dir_all(repo_dir);
}

#[test]
fn scripts_in_order_and_their_env() {
    let repo_dir = repo("script");

    assert!(script(&repo_dir, "setup").is_none());

    std::fs::write(
        repo_dir.join("conductor.json"),
        r#"{"scripts": {"setup": "echo conductor $CONDUCTOR_PORT", "archive": "echo bye"}}"#,
    )
    .unwrap();

    assert_eq!(script(&repo_dir, "setup").unwrap().1, "conductor.json scripts.setup");
    assert_eq!(
        script(&repo_dir, "teardown").unwrap().1,
        "conductor.json scripts.archive"
    );

    std::fs::create_dir_all(repo_dir.join(".codehead")).unwrap();
    std::fs::write(
        repo_dir.join(".codehead/setup"),
        "echo port=$CODEHEAD_PORT root=$CODEHEAD_ROOT\nexit 3\n",
    )
    .unwrap();
    let (argv, label, _) = script(&repo_dir, "setup").unwrap();

    assert_eq!(label, ".codehead/setup");

    let (ok, out) = run(
        &argv,
        &repo_dir,
        &env(Path::new("/r"), &repo_dir, "t1", "b", Some(40010)),
        Duration::from_secs(10),
    );

    assert!(!ok, "exit 3: failed");
    assert_eq!(out.trim(), "port=40010 root=/r");

    let _ = std::fs::remove_dir_all(repo_dir);
}

#[test]
fn a_script_over_its_time_gets_killed() {
    let repo_dir = repo("slow");
    let (ok, out) = run(
        &["sh".into(), "-c".into(), "echo start; sleep 30".into()],
        &repo_dir,
        &[],
        Duration::from_secs(2),
    );

    assert!(!ok && out.contains("start") && out.contains("stopped after"), "{out}");

    let _ = std::fs::remove_dir_all(repo_dir);
}

#[test]
fn free_port_skips_held_blocks() {
    let thread = |port: Option<u16>| crate::engine::store::Thread {
        port,
        state: "open".into(),
        ..Default::default()
    };

    assert_eq!(free_port(&[]), 40000);
    assert_eq!(
        free_port(&[thread(Some(40000)), thread(Some(40020)), thread(None)]),
        40010
    );
}
