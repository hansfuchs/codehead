use super::*;

#[test]
fn the_subcommand_comes_after_the_git_options() {
    let agent = |value: &[&str]| value.iter().map(OsString::from).collect::<Vec<_>>();

    assert_eq!(
        split_args(&agent(&["push", "-u", "origin", "HEAD"])).1.as_deref(),
        Some("push")
    );
    assert_eq!(
        split_args(&agent(&["-C", "/x", "-c", "a=b", "push"])),
        (agent(&["-C", "/x", "-c", "a=b"]), Some("push".into()))
    );
    assert_eq!(split_args(&agent(&["--no-pager", "log"])).1.as_deref(), Some("log"));
    assert_eq!(
        split_args(&agent(&["-C", "push", "status"])).1.as_deref(),
        Some("status")
    );
    assert_eq!(split_args(&agent(&["--version"])).1, None);
}

#[test]
fn one_holder_at_a_time_a_dead_one_frees_the_lock() {
    let directory = std::env::temp_dir().join(format!("codehead-pushlock-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&directory);

    std::fs::create_dir_all(&directory).unwrap();

    let group = acquire_in(&directory, "GIGAS", std::process::id()).unwrap();

    assert_eq!(held_in(&directory).map(|height| height.by).as_deref(), Some("GIGAS"));
    assert_eq!(
        acquire_in(&directory, "you", std::process::id()).unwrap_err().by,
        "GIGAS"
    );

    drop(group);

    assert!(held_in(&directory).is_none());

    // a holder that ended (pid of a finished child): its lock goes at the next look
    let mut character = Command::new("true").spawn().unwrap();
    let pid = character.id();

    character.wait().unwrap();
    let stale = acquire_in(&directory, "ADA", pid).unwrap();

    std::mem::forget(stale);

    assert!(held_in(&directory).is_none());
    assert!(acquire_in(&directory, "you", std::process::id()).is_ok());

    let _ = std::fs::remove_dir_all(&directory);
}
