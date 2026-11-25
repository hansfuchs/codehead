use super::*;
use std::process::{Command, Stdio};

#[test]
fn a_pinned_worktree_switches_back_detached_and_rebase_pass_repo_hooks_run() {
    let base = std::env::temp_dir().join(format!("ch-branchpin-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);

    std::fs::create_dir_all(&base).unwrap();

    let run = |character: &str| {
        Command::new("sh")
            .arg("-c")
            .arg(character)
            .current_dir(&base)
            .env_remove("CODEHEAD_ALLOW_SWITCH")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap()
            .success()
    };

    let sh = |character_value: &str| assert!(run(character_value), "{character_value}");
    let cfg = "git config user.email t@t && git config user.name t";

    sh(&format!(
        "git init -q -b main r && cd r && {cfg} && mkdir .hooks && printf '#!/bin/sh\\ntouch \"$(git rev-parse --git-dir)/pre-commit-ran\"\\n' > .hooks/pre-commit && chmod +x .hooks/pre-commit && git add .hooks && git commit -qm a && git config core.hooksPath .hooks && git branch feat && git commit -q --allow-empty -m b && git switch -q -c other"
    ));
    sh("cd r && git worktree add -q ../w feat");
    let width = base.join("w");

    pin(&width, "feat").unwrap_or_else(|error| panic!("{error}"));
    let branch = || crate::domain::threads::current_branch(&width);

    // a switch: back on feat, exit 1 (the `&&` chain stops), untracked files stay
    sh("cd w && echo u > u");

    assert!(!run("cd w && git checkout -q main && touch pulled"));
    assert_eq!(branch(), "feat");
    assert!(width.join("u").exists() && !width.join("pulled").exists());
    assert!(!run("cd w && git switch -q -c side"));
    assert_eq!(branch(), "feat");

    // override, detached HEAD, rebase: pass
    sh("cd w && CODEHEAD_ALLOW_SWITCH=1 git switch -q main && CODEHEAD_ALLOW_SWITCH=1 git switch -q feat");
    sh("cd w && git switch -q --detach && git switch -q feat");
    sh(&format!(
        "cd w && {cfg} && rm u && git commit -q --allow-empty -m c && git rebase -q main"
    ));

    assert_eq!(branch(), "feat");

    // the repo's own hooks still run; the main checkout: 0 pin
    let gitdir = PathBuf::from(git(&width, &["rev-parse", "--absolute-git-dir"]).1.trim());

    assert!(gitdir.join("pre-commit-ran").exists());

    pin(&base.join("r"), "other").unwrap();
    sh("cd r && git switch -q main");
    let _ = std::fs::remove_dir_all(base);
}
