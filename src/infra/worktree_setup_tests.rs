use super::*;

fn sh(path: &Path, character: &str) {
    assert!(
        Command::new("sh")
            .arg("-c")
            .arg(character)
            .current_dir(path)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap()
            .success(),
        "{character}"
    );
}

fn repo(name: &str) -> PathBuf {
    let directory = std::env::temp_dir().join(format!("codehead-setup-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&directory);

    std::fs::create_dir_all(&directory).unwrap();
    sh(
        &directory,
        "git init -q && git config user.email t@t && git config user.name t && printf '.env*\\ncerts/\\n' > .gitignore && echo x > a.rs && git add -A && git commit -qm init",
    );
    directory
}

#[test]
fn default_includes_are_the_ignored_env_files() {
    let directory = repo("inc");

    sh(
        &directory,
        "echo S=1 > .env && echo L=1 > .env.local && echo t > .envrc.tracked && git add -f .envrc.tracked && git commit -qm t",
    );

    assert_eq!(includes(&directory), [".env", ".env.local"]);

    std::fs::write(directory.join(".worktreeinclude"), "# local\n.env\ncerts/\n\n").unwrap();

    assert_eq!(includes(&directory), [".env", "certs"]);

    let _ = std::fs::remove_dir_all(directory);
}

#[test]
fn copy_includes_brings_ignored_files_and_dirs_into_the_worktree() {
    let directory = repo("copy");

    sh(
        &directory,
        "echo S=1 > .env && mkdir -p certs/a && echo k > certs/a/key.pem && printf '.env\\ncerts\\nmissing\\n' > .worktreeinclude",
    );
    let wt = directory.join("wt");

    sh(&directory, "git worktree add -q -b t wt");
    let copied = copy_includes(&directory, &wt);

    assert_eq!(copied, [".env", "certs"]);
    assert_eq!(std::fs::read_to_string(wt.join(".env")).unwrap(), "S=1\n");
    assert!(wt.join("certs/a/key.pem").is_file());

    // a 2nd run: 0 copies over existing files
    std::fs::write(wt.join(".env"), "S=2\n").unwrap();

    assert_eq!(copy_includes(&directory, &wt), [] as [std::string::String; 0]);
    assert_eq!(std::fs::read_to_string(wt.join(".env")).unwrap(), "S=2\n");

    let _ = std::fs::remove_dir_all(directory);
}

#[test]
fn scripts_in_order_and_their_env() {
    let directory = repo("script");

    assert!(script(&directory, "setup").is_none());

    std::fs::write(
        directory.join("conductor.json"),
        r#"{"scripts": {"setup": "echo conductor $CONDUCTOR_PORT", "archive": "echo bye"}}"#,
    )
    .unwrap();

    assert_eq!(script(&directory, "setup").unwrap().1, "conductor.json scripts.setup");
    assert_eq!(
        script(&directory, "teardown").unwrap().1,
        "conductor.json scripts.archive"
    );

    std::fs::create_dir_all(directory.join(".codehead")).unwrap();
    std::fs::write(
        directory.join(".codehead/setup"),
        "echo port=$CODEHEAD_PORT root=$CODEHEAD_ROOT\nexit 3\n",
    )
    .unwrap();
    let (argv, label, _) = script(&directory, "setup").unwrap();

    assert_eq!(label, ".codehead/setup");

    let (ok, out) = run(
        &argv,
        &directory,
        &env(Path::new("/r"), &directory, "t1", "b", Some(40010)),
        Duration::from_secs(10),
    );

    assert!(!ok, "exit 3: failed");
    assert_eq!(out.trim(), "port=40010 root=/r");

    let _ = std::fs::remove_dir_all(directory);
}

#[test]
fn a_script_over_its_time_gets_killed() {
    let directory = repo("slow");
    let (ok, out) = run(
        &["sh".into(), "-c".into(), "echo start; sleep 30".into()],
        &directory,
        &[],
        Duration::from_secs(2),
    );

    assert!(!ok && out.contains("start") && out.contains("stopped after"), "{out}");

    let _ = std::fs::remove_dir_all(directory);
}

#[test]
fn free_port_skips_held_blocks() {
    let thread = |path: Option<u16>| crate::engine::store::Thread {
        port: path,
        state: "open".into(),
        ..Default::default()
    };

    assert_eq!(free_port(&[]), 40000);
    assert_eq!(
        free_port(&[thread(Some(40000)), thread(Some(40020)), thread(None)]),
        40010
    );
}
