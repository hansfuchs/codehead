use super::*;

#[test]
fn the_subcommand_comes_after_the_git_options() {
    let os_args = |words: &[&str]| words.iter().map(OsString::from).collect::<Vec<_>>();

    assert_eq!(
        split_args(&os_args(&["push", "-u", "origin", "HEAD"])).1.as_deref(),
        Some("push")
    );
    assert_eq!(
        split_args(&os_args(&["-C", "/x", "-c", "a=b", "push"])),
        (os_args(&["-C", "/x", "-c", "a=b"]), Some("push".into()))
    );
    assert_eq!(split_args(&os_args(&["--no-pager", "log"])).1.as_deref(), Some("log"));
    assert_eq!(
        split_args(&os_args(&["-C", "push", "status"])).1.as_deref(),
        Some("status")
    );
    assert_eq!(split_args(&os_args(&["--version"])).1, None);
}

#[test]
fn one_holder_at_a_time_a_dead_one_frees_the_lock() {
    let dir = std::env::temp_dir().join(format!("codehead-pushlock-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);

    std::fs::create_dir_all(&dir).unwrap();

    let guard = acquire_in(&dir, "GIGAS", std::process::id()).unwrap();

    assert_eq!(held_in(&dir).map(|holder| holder.by).as_deref(), Some("GIGAS"));
    assert_eq!(acquire_in(&dir, "you", std::process::id()).unwrap_err().by, "GIGAS");

    drop(guard);

    assert!(held_in(&dir).is_none());

    // a holder that ended (pid of a finished child): its lock goes at the next look
    let mut child = Command::new("true").spawn().unwrap();
    let pid = child.id();

    child.wait().unwrap();
    let stale = acquire_in(&dir, "ADA", pid).unwrap();

    std::mem::forget(stale);

    assert!(held_in(&dir).is_none());
    assert!(acquire_in(&dir, "you", std::process::id()).is_ok());

    let _ = std::fs::remove_dir_all(&dir);
}
