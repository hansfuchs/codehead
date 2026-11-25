//! A thread's worktree stays on its thread's branch: a `post-checkout` hook of that worktree
//! only switches back at once when a checkout (`git switch main`, an editor's branch picker, …)
//! moves HEAD to another branch, and exits 1: a `git checkout main && git pull` stops there.
//! Git has 0 hook before a checkout: a `reference-transaction` abort comes after the files changed
//! (HEAD stays, the index and files of the other branch stay). A detached HEAD (rebase, bisect,
//! `git switch --detach`) passes. Override once: `CODEHEAD_ALLOW_SWITCH=1 git switch …`.
//!
//! Only this worktree: its own `core.hooksPath` (`extensions.worktreeConfig`) →
//! `<git-dir>/codehead-hooks/`, the other hooks of the repo (`core.hooksPath` of the repo or the
//! user, else `<common-dir>/hooks`) as symlinks there: 0 extra process per hook.

use std::path::{Path, PathBuf};

use crate::domain::threads::git;

/// Client-side hooks a worktree runs: the ones of the repo get a symlink in the pinned dir.
const HOOKS: &[&str] = &[
    "applypatch-msg",
    "pre-applypatch",
    "post-applypatch",
    "pre-commit",
    "pre-merge-commit",
    "prepare-commit-msg",
    "commit-msg",
    "post-commit",
    "pre-rebase",
    "post-merge",
    "pre-push",
    "post-rewrite",
    "reference-transaction",
    "sendemail-validate",
    "pre-auto-gc",
    "post-index-change",
];

const GUARD: &str = r#"#!/bin/sh
# codehead: this worktree stays on its thread's branch (src/branchpin.rs)
want=$(cat "$(git rev-parse --git-dir)/codehead-branch" 2>/dev/null)
if [ "$3" = 1 ] && [ -n "$want" ] && [ -z "$CODEHEAD_ALLOW_SWITCH" ]; then
    now=$(git symbolic-ref --quiet --short HEAD)
    if [ -n "$now" ] && [ "$now" != "$want" ]; then
        main=$(cd "$(git rev-parse --path-format=absolute --git-common-dir)/.." && pwd)
        echo "codehead: this worktree stays on $want (a thread's worktree): back from $now." >&2
        echo "codehead: other branches: the main checkout $main. Override once: CODEHEAD_ALLOW_SWITCH=1 git switch $now" >&2
        CODEHEAD_ALLOW_SWITCH=1 git switch --quiet "$want" || echo "codehead: the switch back failed: git switch $want" >&2
        exit 1
    fi
fi
"#;

/// Pin the linked worktree WT to BRANCH (again: the hooks of the repo now). Err: git's message;
/// the main checkout or a `core.worktree` repo: Ok, 0 pin.
pub(crate) fn pin(wt: &Path, branch: &str) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;

    let out = |args: &[&str]| command_result(wt, args);

    let gitdir = PathBuf::from(out(&["rev-parse", "--absolute-git-dir"])?);
    let common = PathBuf::from(out(&["rev-parse", "--path-format=absolute", "--git-common-dir"])?);

    if gitdir == common
        || git(
            wt,
            &[
                "config",
                "--file",
                &common.join("config").display().to_string(),
                "core.worktree",
            ],
        )
        .0
    {
        return Ok(());
    }

    let orig = repo_hooks(wt, &common);
    let dir = gitdir.join("codehead-hooks");
    let io = |error_value: std::io::Error| format!("{}: {error_value}", dir.display());
    let _ = std::fs::remove_dir_all(&dir);

    std::fs::create_dir_all(&dir).map_err(io)?;

    for height in HOOKS {
        let src = orig.join(height);

        if is_exec(&src) {
            std::os::unix::fs::symlink(&src, dir.join(height)).map_err(io)?;
        }
    }

    let prev = orig.join("post-checkout");

    let chain = if is_exec(&prev) {
        format!("exec '{}' \"$@\"\n", prev.display().to_string().replace('\'', r"'\''"))
    } else {
        String::new()
    };

    let guard = dir.join("post-checkout");

    std::fs::write(&guard, format!("{GUARD}{chain}")).map_err(io)?;

    std::fs::set_permissions(&guard, std::fs::Permissions::from_mode(0o755)).map_err(io)?;
    std::fs::write(gitdir.join("codehead-branch"), format!("{branch}\n")).map_err(io)?;

    if git(wt, &["config", "--get", "--type=bool", "extensions.worktreeConfig"])
        .1
        .trim()
        != "true"
    {
        out(&[
            "config",
            "--file",
            &common.join("config").display().to_string(),
            "extensions.worktreeConfig",
            "true",
        ])?;
    }

    out(&["config", "--worktree", "core.hooksPath", &dir.display().to_string()])?;

    Ok(())
}

/// The hooks dir of the repo for worktree WT: the last `core.hooksPath` outside the worktree's
/// own config (relative: to WT), else `<COMMON>/hooks`.
fn repo_hooks(wt: &Path, common: &Path) -> PathBuf {
    let (_, output, _) = git(
        wt,
        &["config", "--show-scope", "--type=path", "--get-all", "core.hooksPath"],
    );
    let path_value = output
        .lines()
        .filter_map(|line| line.split_once('\t'))
        .filter(|(scope, _)| *scope != "worktree")
        .map(|(_, path)| path.to_string())
        .next_back();

    match path_value {
        Some(path_2) if Path::new(&path_2).is_absolute() => PathBuf::from(path_2),
        Some(path_3) => wt.join(path_3),
        None => common.join("hooks"),
    }
}

fn is_exec(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).is_ok_and(|message| message.is_file() && message.permissions().mode() & 0o111 != 0)
}

#[cfg(test)]
#[path = "branch_pin_tests.rs"]
mod tests;

fn command_result(wt: &Path, args: &[&str]) -> Result<String, String> {
    let (ok, output, error) = git(wt, args);

    if ok {
        Ok(output.trim().to_string())
    } else {
        Err(format!("git {}: {error}", args.join(" ")))
    }
}
