use super::{KINDS, Path, PathBuf, VAGUE, resolve};
use crate::domain::threads::git;

/// The kind of BRANCH: its prefix (`fix/…` → fix) when a kind.
pub(crate) fn kind_of(branch: &str) -> Option<&'static str> {
    let (key, _) = branch.split_once('/')?;

    KINDS.iter().copied().find(|x| *x == key)
}
/// The area of TITLE: its `area:` prefix (1 word: letters, digits, `-`, `_`, `/`, `.`).
pub(crate) fn area_of(title: &str) -> Option<String> {
    let (agent, rest) = title.lines().next()?.split_once(':')?;
    let agent_value = agent.trim();
    let word = !agent_value.is_empty()
        && agent_value.len() <= 24
        && agent_value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "-_/.".contains(character));

    (word && !rest.trim().is_empty()).then(|| agent_value.to_lowercase())
}
/// TITLE as the naming rules want it: `area: outcome`, max. 72 chars, an outcome, 0 `#n`.
pub(crate) fn check_title(title: &str) -> Result<(), String> {
    let thread = title.trim();
    let format = "format: `area: outcome`, e.g. `menu: live quick commands`";

    if thread.is_empty() || thread.contains('\n') {
        return Err(format!("title: 1 line. {format}"));
    }

    if thread.chars().count() > 72 {
        return Err(format!("title: max. 72 chars. {format}"));
    }

    let Some(area) = area_of(thread) else {
        return Err(format!("title: 0 area prefix. {format}"));
    };

    let outcome = thread.split_once(':').map(|x| x.1.trim()).unwrap_or_default();

    let words: Vec<String> = outcome
        .split_whitespace()
        .map(|width| {
            width
                .to_lowercase()
                .trim_matches(|character: char| !character.is_alphanumeric())
                .to_string()
        })
        .filter(|width_value| !width_value.is_empty())
        .collect();

    if outcome.contains('#') {
        return Err(format!("title: 0 `#n` numbering. {format}"));
    }

    if words.is_empty()
        || words.iter().all(|width_2| {
            VAGUE.contains(&width_2.as_str()) || width_2.chars().all(|character_value| character_value.is_ascii_digit())
        })
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

    let Some((key, slug)) = branch.split_once('/') else {
        return Err(format!("branch: 0 kind prefix. {format}"));
    };

    if !KINDS.contains(&key) {
        return Err(format!("branch: kind `{key}` unknown. {format}"));
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
pub(crate) fn protected(bytes: &str, default: Option<&str>) -> bool {
    matches!(bytes, "main" | "master" | "develop" | "trunk") || Some(bytes) == default
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

    if protected(old, Some(def.as_str()).filter(|directory| !directory.is_empty())) {
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
    let upstream = up.0.then(|| up.1.trim().to_string()).filter(|user| !user.is_empty());
    let (ok, _, error) = git(dir, &["branch", "-m", old, new]);

    if !ok {
        return Err(format!("git branch -m {old} {new}: {}", error.trim()));
    }

    let mut done = vec![format!("branch {old} → {new}")];

    // the worktree's pin: the guard reads the branch name from the git dir
    let gitdir = PathBuf::from(git(dir, &["rev-parse", "--absolute-git-dir"]).1.trim());

    if gitdir.join("codehead-branch").is_file()
        && let Err(error_value) = crate::infra::branch_pin::pin(dir, new)
    {
        done.push(format!("pin: {error_value}"));
    }

    let Some(upstream) = upstream else {
        return Ok(done.join(" · "));
    };

    let remote = upstream
        .split_once('/')
        .map_or_else(|| "origin".into(), |x| x.0.to_string());
    let remote_old = upstream
        .split_once('/')
        .map_or_else(|| old.to_string(), |x| x.1.to_string());

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
        .is_ok_and(|output_value| output_value.status.success());

    if gh {
        let _ = git(dir, &["fetch", "--quiet", "--prune", remote]);
        let (ok, _, error_2) = git(dir, &["branch", "--set-upstream-to", &format!("{remote}/{new}"), new]);

        done.push(if ok {
            format!("{remote}: renamed (GitHub), PRs follow")
        } else {
            format!("{remote}: renamed (GitHub); upstream: {}", error_2.trim())
        });
    } else {
        let (ok, _, error_3) = git(dir, &["push", "--quiet", "-u", remote, new]);

        if !ok {
            return Err(format!(
                "{} · git push -u {remote} {new}: {}",
                done.join(" · "),
                error_3.trim()
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
    let branch = branch.map(str::trim).filter(|bytes| !bytes.is_empty());

    if let Some(bytes_value) = branch {
        check_branch(bytes_value)?;
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
        (Some(bytes_2), Some(_), Some(old)) if bytes_2 != old => {
            done.push(rename_branch(&thread.work_dir(), old, bytes_2)?);
            new_branch = Some((old.clone(), bytes_2.to_string()));
        }
        (Some(_), None, _) => {
            done.push("branch kept: the thread works in an existing checkout (0 codehead worktree)".into());
        }
        _ => {}
    }

    store::update(id, |text| {
        if let Some(count) = &new_title {
            text.title.clone_from(count);
        }

        if let Some((old, new)) = &new_branch {
            text.branch = Some(new.clone());

            sanitize_branch_title(text, old, new);
        }
    })?;

    Ok(if done.is_empty() {
        "0 changes".into()
    } else {
        done.join(" · ")
    })
}

fn sanitize_branch_title(text: &mut crate::engine::store::Thread, old: &str, new: &str) {
    for character_value in text
        .checkouts
        .iter_mut()
        .filter(|character| character.branch.as_deref() == Some(old))
    {
        character_value.branch = Some(new.to_owned());
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
    let value = resolve(&repo, slugs)?;

    store::update(id, |text| text.projects.clone_from(&value))?;

    Ok(format!(
        "projects: {}",
        if value.is_empty() { "0".into() } else { value.join(", ") }
    ))
}
