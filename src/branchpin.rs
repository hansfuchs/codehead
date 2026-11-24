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

use crate::threads::git;

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
pub fn pin(wt: &Path, branch: &str) -> Result<(), String> {
    let out = |args: &[&str]| -> Result<String, String> {
        let (ok, o, e) = git(wt, args);
        if ok { Ok(o.trim().to_string()) } else { Err(format!("git {}: {e}", args.join(" "))) }
    };
    let gitdir = PathBuf::from(out(&["rev-parse", "--absolute-git-dir"])?);
    let common = PathBuf::from(out(&["rev-parse", "--path-format=absolute", "--git-common-dir"])?);
    if gitdir == common || git(wt, &["config", "--file", &common.join("config").display().to_string(), "core.worktree"]).0 {
        return Ok(());
    }
    let orig = repo_hooks(wt, &common);
    let dir = gitdir.join("codehead-hooks");
    let io = |e: std::io::Error| format!("{}: {e}", dir.display());
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(io)?;
    for h in HOOKS {
        let src = orig.join(h);
        if is_exec(&src) {
            std::os::unix::fs::symlink(&src, dir.join(h)).map_err(io)?;
        }
    }
    let prev = orig.join("post-checkout");
    let chain = if is_exec(&prev) { format!("exec '{}' \"$@\"\n", prev.display().to_string().replace('\'', r"'\''")) } else { String::new() };
    let guard = dir.join("post-checkout");
    std::fs::write(&guard, format!("{GUARD}{chain}")).map_err(io)?;
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&guard, std::fs::Permissions::from_mode(0o755)).map_err(io)?;
    std::fs::write(gitdir.join("codehead-branch"), format!("{branch}\n")).map_err(io)?;
    if git(wt, &["config", "--get", "--type=bool", "extensions.worktreeConfig"]).1.trim() != "true" {
        out(&["config", "--file", &common.join("config").display().to_string(), "extensions.worktreeConfig", "true"])?;
    }
    out(&["config", "--worktree", "core.hooksPath", &dir.display().to_string()])?;
    Ok(())
}

/// The hooks dir of the repo for worktree WT: the last `core.hooksPath` outside the worktree's
/// own config (relative: to WT), else `<COMMON>/hooks`.
fn repo_hooks(wt: &Path, common: &Path) -> PathBuf {
    let (_, o, _) = git(wt, &["config", "--show-scope", "--type=path", "--get-all", "core.hooksPath"]);
    let p = o.lines().filter_map(|l| l.split_once('\t')).filter(|(scope, _)| *scope != "worktree").map(|(_, p)| p.to_string()).next_back();
    match p {
        Some(p) if Path::new(&p).is_absolute() => PathBuf::from(p),
        Some(p) => wt.join(p),
        None => common.join("hooks"),
    }
}

fn is_exec(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::{Command, Stdio};

    #[test]
    fn a_pinned_worktree_switches_back_detached_and_rebase_pass_repo_hooks_run() {
        let base = std::env::temp_dir().join(format!("ch-branchpin-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let run = |c: &str| Command::new("sh").arg("-c").arg(c).current_dir(&base).env_remove("CODEHEAD_ALLOW_SWITCH").stdout(Stdio::null()).stderr(Stdio::null()).status().unwrap().success();
        let sh = |c: &str| assert!(run(c), "{c}");
        let cfg = "git config user.email t@t && git config user.name t";
        sh(&format!("git init -q -b main r && cd r && {cfg} && mkdir .hooks && printf '#!/bin/sh\\ntouch \"$(git rev-parse --git-dir)/pre-commit-ran\"\\n' > .hooks/pre-commit && chmod +x .hooks/pre-commit && git add .hooks && git commit -qm a && git config core.hooksPath .hooks && git branch feat && git commit -q --allow-empty -m b && git switch -q -c other"));
        sh("cd r && git worktree add -q ../w feat");
        let w = base.join("w");
        pin(&w, "feat").unwrap_or_else(|e| panic!("{e}"));
        let branch = || crate::threads::current_branch(&w);
        // a switch: back on feat, exit 1 (the `&&` chain stops), untracked files stay
        sh("cd w && echo u > u");
        assert!(!run("cd w && git checkout -q main && touch pulled"));
        assert_eq!(branch(), "feat");
        assert!(w.join("u").exists() && !w.join("pulled").exists());
        assert!(!run("cd w && git switch -q -c side"));
        assert_eq!(branch(), "feat");
        // override, detached HEAD, rebase: pass
        sh("cd w && CODEHEAD_ALLOW_SWITCH=1 git switch -q main && CODEHEAD_ALLOW_SWITCH=1 git switch -q feat");
        sh("cd w && git switch -q --detach && git switch -q feat");
        sh(&format!("cd w && {cfg} && rm u && git commit -q --allow-empty -m c && git rebase -q main"));
        assert_eq!(branch(), "feat");
        // the repo's own hooks still run; the main checkout: 0 pin
        let gitdir = PathBuf::from(git(&w, &["rev-parse", "--absolute-git-dir"]).1.trim());
        assert!(gitdir.join("pre-commit-ran").exists());
        pin(&base.join("r"), "other").unwrap();
        sh("cd r && git switch -q main");
        let _ = std::fs::remove_dir_all(base);
    }
}
