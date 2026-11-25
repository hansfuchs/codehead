use super::*;

#[test]
fn main_checkout_worktree_and_detached_head() {
    let root = std::env::temp_dir().join(format!("codehead-workspot-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);

    // the main checkout: .git/ with HEAD on a branch
    let main = root.join("app");

    std::fs::create_dir_all(main.join(".git/worktrees/wt")).unwrap();
    std::fs::write(main.join(".git/HEAD"), "ref: refs/heads/feat/x\n").unwrap();

    assert_eq!(
        spot(&main.join("src").join(".."), None).branch.as_deref(),
        Some("feat/x")
    );

    let app = spot(&main, None);

    assert_eq!(
        app,
        Spot {
            repo: "app".into(),
            branch: Some("feat/x".into()),
            worktree: false
        }
    );

    // a linked worktree: .git file → its git dir, detached
    let wt = root.join("wt");

    std::fs::create_dir_all(&wt).unwrap();
    std::fs::write(
        wt.join(".git"),
        format!("gitdir: {}\n", main.join(".git/worktrees/wt").display()),
    )
    .unwrap();
    std::fs::write(main.join(".git/worktrees/wt/HEAD"), "0123456789abcdef\n").unwrap();

    assert_eq!(
        spot(&wt, None),
        Spot {
            repo: "app".into(),
            branch: Some("@0123456".into()),
            worktree: true
        }
    );

    // 0 git: the dir's name, 0 branch
    let plain = root.join("plain");

    std::fs::create_dir_all(&plain).unwrap();

    assert_eq!(
        spot(&plain, Some("main".into())),
        Spot {
            repo: "plain".into(),
            branch: None,
            worktree: false
        }
    );

    let text: String = spans(&[spot(&main, None), spot(&wt, None)])
        .iter()
        .map(|span| span.content.to_string())
        .collect();

    assert_eq!(text, "  · in app ⎇ feat/x, app ⎇ @0123456 (worktree)");

    let _ = std::fs::remove_dir_all(&root);
}
