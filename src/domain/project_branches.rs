use super::{KINDS, Path, PathBuf, VAGUE, resolve};
use crate::domain::threads::git;

/// The kind of BRANCH: its prefix (`fix/…` → fix) when a kind.
pub(crate) fn kind_of(branch: &str) -> Option<&'static str> {
    let (prefix, _) = branch.split_once('/')?;

    KINDS.iter().copied().find(|kind| *kind == prefix)
}
/// The area of TITLE: its `area:` prefix (1 word: letters, digits, `-`, `_`, `/`, `.`).
pub(crate) fn area_of(title: &str) -> Option<String> {
    let (head, rest) = title.lines().next()?.split_once(':')?;
    let area = head.trim();
    let word = !area.is_empty()
        && area.len() <= 24
        && area
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "-_/.".contains(character));

    (word && !rest.trim().is_empty()).then(|| area.to_lowercase())
}
/// TITLE as the naming rules want it: `area: outcome`, max. 72 chars, an outcome, 0 `#n`.
pub(crate) fn check_title(title: &str) -> Result<(), String> {
    let title = title.trim();
    let format = "format: `area: outcome`, e.g. `menu: live quick commands`";

    if title.is_empty() || title.contains('\n') {
        return Err(format!("title: 1 line. {format}"));
    }

    if title.chars().count() > 72 {
        return Err(format!("title: max. 72 chars. {format}"));
    }

    let Some(area) = area_of(title) else {
        return Err(format!("title: 0 area prefix. {format}"));
    };

    let outcome = title.split_once(':').map(|parts| parts.1.trim()).unwrap_or_default();

    let words: Vec<String> = outcome
        .split_whitespace()
        .map(|word| {
            word.to_lowercase()
                .trim_matches(|character: char| !character.is_alphanumeric())
                .to_string()
        })
        .filter(|word| !word.is_empty())
        .collect();

    if outcome.contains('#') {
        return Err(format!("title: 0 `#n` numbering. {format}"));
    }

    if words.is_empty()
        || words
            .iter()
            .all(|word| VAGUE.contains(&word.as_str()) || word.chars().all(|character| character.is_ascii_digit()))
    {
        return Err(format!(
            "title: `{outcome}` names 0 outcome (area `{area}`). say what changed. {format}"
        ));
    }

    Ok(())
}
/// BRANCH as the naming rules want it: `<kind>/<slug>`, slug `a-z 0-9 -`, max. 60 chars.
pub(crate) fn check_branch(branch: &str) -> Result<(), String> {
    let format = format!("format: `<kind>/<area>-<slug>`, kind: {}", KINDS.join(" "));

    let Some((kind, slug)) = branch.split_once('/') else {
        return Err(format!("branch: 0 kind prefix. {format}"));
    };

    if !KINDS.contains(&kind) {
        return Err(format!("branch: kind `{kind}` unknown. {format}"));
    }

    if slug.is_empty()
        || branch.len() > 60
        || !slug
            .chars()
            .all(|character| character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-')
        || slug.starts_with('-')
        || slug.ends_with('-')
        || slug.contains("--")
    {
        return Err(format!(
            "branch: slug `{slug}`: a-z, 0-9, single `-`, max. 60 chars in all. {format}"
        ));
    }

    Ok(())
}
/// Branches an agent never renames.
pub(crate) fn protected(branch: &str, default: Option<&str>) -> bool {
    matches!(branch, "main" | "master" | "develop" | "trunk") || Some(branch) == default
}
/// Rename branch OLD of checkout DIR to NEW: local (`git branch -m`), the worktree's pin, the
/// remote branch when OLD has an upstream (GitHub: its rename API, open PRs follow; else NEW
/// pushed, OLD kept there). Ok: what changed.
pub(crate) fn rename_branch(dir: &Path, old: &str, new: &str) -> Result<String, String> {
    if old == new {
        return Ok("branch unchanged".into());
    }

    let def = git(dir, &["symbolic-ref", "--quiet", "--short", "refs/remotes/origin/HEAD"])
        .1
        .trim()
        .trim_start_matches("origin/")
        .to_string();

    if protected(old, Some(def.as_str()).filter(|name| !name.is_empty())) {
        return Err(format!("branch {old}: the default branch, 0 rename"));
    }

    if git(dir, &["rev-parse", "--verify", "--quiet", &format!("refs/heads/{new}")]).0 {
        return Err(format!("branch {new}: exists"));
    }

    let up = git(
        dir,
        &[
            "rev-parse",
            "--abbrev-ref",
            "--symbolic-full-name",
            &format!("{old}@{{upstream}}"),
        ],
    );
    let upstream = up.0.then(|| up.1.trim().to_string()).filter(|name| !name.is_empty());
    let (ok, _, stderr) = git(dir, &["branch", "-m", old, new]);

    if !ok {
        return Err(format!("git branch -m {old} {new}: {}", stderr.trim()));
    }

    let mut done = vec![format!("branch {old} → {new}")];

    // the worktree's pin: the guard reads the branch name from the git dir
    let gitdir = PathBuf::from(git(dir, &["rev-parse", "--absolute-git-dir"]).1.trim());

    if gitdir.join("codehead-branch").is_file()
        && let Err(stderr) = crate::infra::branch_pin::pin(dir, new)
    {
        done.push(format!("pin: {stderr}"));
    }

    let Some(upstream) = upstream else {
        return Ok(done.join(" · "));
    };

    let remote = upstream
        .split_once('/')
        .map_or_else(|| "origin".into(), |parts| parts.0.to_string());
    let remote_old = upstream
        .split_once('/')
        .map_or_else(|| old.to_string(), |parts| parts.1.to_string());

    rename_remote_branch(dir, new, done, &remote, &remote_old)
}

fn rename_remote_branch(
    dir: &Path,
    new: &str,
    mut done: Vec<String>,
    remote: &str,
    remote_old: &str,
) -> Result<String, String> {
    let gh = std::process::Command::new("gh")
        .args([
            "api",
            "-X",
            "POST",
            &format!("repos/{{owner}}/{{repo}}/branches/{remote_old}/rename"),
            "-f",
            &format!("new_name={new}"),
        ])
        .current_dir(dir)
        .stdin(std::process::Stdio::null())
        .output()
        .is_ok_and(|output| output.status.success());

    if gh {
        let _ = git(dir, &["fetch", "--quiet", "--prune", remote]);
        let (ok, _, stderr) = git(dir, &["branch", "--set-upstream-to", &format!("{remote}/{new}"), new]);

        done.push(if ok {
            format!("{remote}: renamed (GitHub), PRs follow")
        } else {
            format!("{remote}: renamed (GitHub); upstream: {}", stderr.trim())
        });
    } else {
        let (ok, _, stderr) = git(dir, &["push", "--quiet", "-u", remote, new]);

        if !ok {
            return Err(format!(
                "{} · git push -u {remote} {new}: {}",
                done.join(" · "),
                stderr.trim()
            ));
        }

        done.push(format!(
            "{remote}: {new} pushed, {remote_old} kept there (a PR on it: still open)"
        ));
    }

    Ok(done.join(" · "))
}
/// The repo of thread T (its main checkout).
pub(crate) fn repo_of(thread: &crate::engine::store::Thread) -> Option<PathBuf> {
    crate::domain::threads::repo_of_thread_store(thread)
}
/// `set_title`: thread ID's title TITLE, its branch BRANCH (a codehead worktree's own branch
/// only). An owner-written title stays. Ok: what changed.
pub(crate) fn set_title(id: &str, title: &str, branch: Option<&str>) -> Result<String, String> {
    use crate::engine::store;
    let thread = store::load(id).ok_or_else(|| format!("no thread {id}"))?;
    let title = title.trim();

    check_title(title)?;
    let branch = branch.map(str::trim).filter(|name| !name.is_empty());

    if let Some(branch) = branch {
        check_branch(branch)?;
    }

    let mut done = Vec::new();

    let new_title = if thread.owner_choices.title_set {
        done.push("title kept: the owner wrote it".to_string());
        None
    } else if thread.title.lines().next() == Some(title) {
        None
    } else {
        done.push(format!("title: {title}"));
        Some(title.to_string())
    };

    let mut new_branch = None;

    match (branch, &thread.worktree, &thread.branch) {
        (Some(new), Some(_), Some(old)) if new != old => {
            done.push(rename_branch(&thread.work_dir(), old, new)?);
            new_branch = Some((old.clone(), new.to_string()));
        }
        (Some(_), None, _) => {
            done.push("branch kept: the thread works in an existing checkout (0 codehead worktree)".into());
        }
        _ => {}
    }

    store::update(id, |record| {
        if let Some(title) = &new_title {
            record.title.clone_from(title);
        }

        if let Some((old, new)) = &new_branch {
            record.branch = Some(new.clone());

            rename_checkout_branches(record, old, new);
        }
    })?;

    Ok(if done.is_empty() {
        "0 changes".into()
    } else {
        done.join(" · ")
    })
}

fn rename_checkout_branches(thread: &mut crate::engine::store::Thread, old: &str, new: &str) {
    for checkout in thread
        .checkouts
        .iter_mut()
        .filter(|checkout| checkout.branch.as_deref() == Some(old))
    {
        checkout.branch = Some(new.to_owned());
    }
}
/// `set_projects`: thread ID's projects SLUGS (existing projects of its repo only). Owner-picked
/// projects stay. Ok: what changed.
pub(crate) fn set_projects(id: &str, slugs: &[String]) -> Result<String, String> {
    use crate::engine::store;
    let thread = store::load(id).ok_or_else(|| format!("no thread {id}"))?;

    if thread.owner_choices.projects_set {
        return Ok(format!(
            "projects kept: the owner picked them ({})",
            if thread.projects.is_empty() {
                "0".into()
            } else {
                thread.projects.join(", ")
            }
        ));
    }

    let repo = repo_of(&thread).ok_or("the thread has 0 git repo: 0 projects")?;
    let projects = resolve(&repo, slugs)?;

    store::update(id, |record| record.projects.clone_from(&projects))?;

    Ok(format!(
        "projects: {}",
        if projects.is_empty() {
            "0".into()
        } else {
            projects.join(", ")
        }
    ))
}
