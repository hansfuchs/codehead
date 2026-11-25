use super::{
    Changes, Command, EMPTY_TREE, HashSet, MAX_UNTRACKED, Path, PathBuf, Pr, Stdio, commits, order, parse, untracked,
    z_paths,
};

pub(crate) fn git(dir: &Path, args: &[&str]) -> Result<String, String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["-c", "core.quotePath=false", "-c", "log.showSignature=false"])
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|error| format!("git: {error}"))?;

    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(String::from_utf8_lossy(&out.stderr)
            .lines()
            .next()
            .unwrap_or("git failed")
            .to_string())
    }
}
/// The base the branch gets compared with: its merge-base with the remote's default branch
/// (label: `origin/main`). On the default branch itself, or 0 remote: HEAD (uncommitted only).
pub(crate) fn base(dir: &Path) -> (String, String) {
    let head = || ("HEAD".to_string(), "HEAD".to_string());

    let def = git(dir, &["symbolic-ref", "--short", "refs/remotes/origin/HEAD"])
        .ok()
        .map(|text| text.trim().to_string())
        .filter(|text_value| !text_value.is_empty())
        .or_else(|| {
            ["origin/main", "origin/master"]
                .into_iter()
                .find(|result| git(dir, &["rev-parse", "--verify", "--quiet", result]).is_ok())
                .map(String::from)
        });

    let Some(def) = def else { return head() };
    let branch = git(dir, &["rev-parse", "--abbrev-ref", "HEAD"]).unwrap_or_default();

    if def.ends_with(&format!("/{}", branch.trim())) {
        return head();
    }

    match git(dir, &["merge-base", "HEAD", &def]) {
        Ok(mb) if !mb.trim().is_empty() => (mb.trim().to_string(), def),
        _ => head(),
    }
}
/// A rebase / merge of DIR stopped (its state dir or `MERGE_HEAD` exists).
pub(crate) fn stopped(dir: &Path) -> bool {
    ["rebase-merge", "rebase-apply", "MERGE_HEAD"].iter().any(|path| {
        git(dir, &["rev-parse", "--git-path", path]).is_ok_and(|path_value| {
            let path_2 = PathBuf::from(path_value.trim());

            absolute_conflict_path(dir, path_2)
        })
    })
}

fn absolute_conflict_path(dir: &Path, path_2: PathBuf) -> bool {
    if path_2.is_absolute() {
        path_2.exists()
    } else {
        dir.join(path_2).exists()
    }
}
/// The uncommitted changes of DIR (staged + unstaged + untracked, against HEAD); the base,
/// ahead / behind and the commits as for the branch.
#[cfg(test)]
pub(crate) fn load(dir: &Path) -> Result<Changes, String> {
    load_from(dir, false)
}
/// ALL: the branch's changes against its merge-base (commits + uncommitted), else the
/// uncommitted ones (against HEAD). `rev`: the diff's base.
pub(crate) fn load_from(dir: &Path, all: bool) -> Result<Changes, String> {
    git(dir, &["rev-parse", "--is-inside-work-tree"]).map_err(|_| "not a git worktree".to_string())?;
    let (mb, label) = base(dir);

    // an unborn branch (0 commits): against the empty tree
    let head = if git(dir, &["rev-parse", "--verify", "--quiet", "HEAD"]).is_ok() {
        "HEAD".to_string()
    } else {
        EMPTY_TREE.to_string()
    };

    let rev = if all { mb.clone() } else { head };
    let diff = git(dir, &["diff", "-M", "--no-color", "--no-ext-diff", "-U3", &rev])?;
    let mut files = parse(&diff);
    let others = git(dir, &["ls-files", "--others", "--exclude-standard", "-z"]).unwrap_or_default();
    let mut fresh = HashSet::new();

    for path_value in others.split('\0').filter(|path| !path.is_empty()).take(MAX_UNTRACKED) {
        files.push(untracked(dir, path_value));
        fresh.insert(path_value.to_string());
    }

    order(&mut files);

    // staged: in the index diff; a part: also in the worktree diff
    let cached = z_paths(&git(dir, &["diff", "--cached", "--name-only", "-z"]).unwrap_or_default());
    let unstaged = z_paths(&git(dir, &["diff", "--name-only", "-z"]).unwrap_or_default());
    let staged = cached
        .into_iter()
        .map(|path_2| (path_2.clone(), !unstaged.contains(&path_2)))
        .collect();
    let stopped =
        stopped(dir).then(|| z_paths(&git(dir, &["diff", "--name-only", "--diff-filter=U", "-z"]).unwrap_or_default()));

    let (ahead, behind) = if label == "HEAD" {
        (0, 0)
    } else {
        let lr = git(
            dir,
            &["rev-list", "--left-right", "--count", &format!("HEAD...{label}")],
        )
        .unwrap_or_default();
        let mut it = lr.split_whitespace().map(|x| x.parse::<usize>().unwrap_or(0));

        (it.next().unwrap_or(0), it.next().unwrap_or(0))
    };

    let commits = commits::list(dir, &mb);
    let git_dir = crate::infra::pushlock::git_dir(dir, &[]);

    Ok(Changes {
        base: label,
        rev,
        untracked: fresh,
        files,
        staged,
        stopped,
        ahead,
        behind,
        commits,
        committed: Vec::new(),
        git_dir,
    })
}
/// The pull request of DIR's branch (gh pr view). None: 0 PR, 0 gh.
pub(crate) fn pr_state(dir: &Path) -> Option<Pr> {
    let out = Command::new("gh")
        .current_dir(dir)
        .args(["pr", "view", "--json", "number,url,state,statusCheckRollup"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;

    if !out.status.success() {
        return None;
    }

    let value: serde_json::Value = serde_json::from_slice(&out.stdout).ok()?;

    parse_pr(&value)
}
pub(crate) fn parse_pr(value: &serde_json::Value) -> Option<Pr> {
    let mut pr = Pr {
        number: value["number"].as_u64()?,
        url: value["url"].as_str().unwrap_or_default().into(),
        state: value["state"].as_str().unwrap_or_default().into(),
        ..Default::default()
    };

    for character in value["statusCheckRollup"].as_array().into_iter().flatten() {
        // check runs: status + conclusion; status contexts: state
        let verdict = character["conclusion"]
            .as_str()
            .filter(|text| !text.is_empty())
            .or_else(|| character["state"].as_str())
            .unwrap_or_default()
            .to_ascii_uppercase();

        match verdict.as_str() {
            "SUCCESS" | "NEUTRAL" | "SKIPPED" => pr.pass += 1,
            "FAILURE" | "ERROR" | "TIMED_OUT" | "CANCELLED" | "ACTION_REQUIRED" | "STARTUP_FAILURE" => pr.fail += 1,
            _ => pr.pending += 1,
        }
    }

    Some(pr)
}
/// The failed CI log of DIR's branch (the latest failed run, last 150 lines) as a message for
/// the lead.
pub(crate) fn ci_fix_message(dir: &Path) -> Result<String, String> {
    let gh = |args: &[&str]| -> Result<String, String> {
        let output_value = Command::new("gh")
            .current_dir(dir)
            .args(args)
            .stdin(Stdio::null())
            .output()
            .map_err(|error| format!("gh: {error}"))?;

        if output_value.status.success() {
            Ok(String::from_utf8_lossy(&output_value.stdout).into_owned())
        } else {
            Err(String::from_utf8_lossy(&output_value.stderr)
                .lines()
                .next()
                .unwrap_or("gh failed")
                .to_string())
        }
    };

    let branch = git(dir, &["rev-parse", "--abbrev-ref", "HEAD"])?.trim().to_string();
    let runs: serde_json::Value = serde_json::from_str(&gh(&[
        "run",
        "list",
        "--branch",
        &branch,
        "--limit",
        "10",
        "--json",
        "databaseId,conclusion,name,url",
    ])?)
    .map_err(|error_value| error_value.to_string())?;
    let run = runs
        .as_array()
        .into_iter()
        .flatten()
        .find(|result| result["conclusion"] == "failure")
        .ok_or("0 failed CI runs on this branch")?;
    let id = run["databaseId"].as_u64().ok_or("0 run id")?.to_string();
    let log = gh(&["run", "view", &id, "--log-failed"])?;
    let lines: Vec<&str> = log.lines().collect();
    let tail = lines[lines.len().saturating_sub(150)..].join("\n");

    Ok(format!(
        "CI failed on this branch: {} ({}). Find the cause, fix it, run the check locally if you can, commit and push. The failed log (last 150 lines):\n\n```text\n{tail}\n```",
        run["name"].as_str().unwrap_or("run"),
        run["url"].as_str().unwrap_or_default()
    ))
}
/// The patch of the hunk of PATH that holds new-side LINE: from the worktree-vs-index diff
/// (CACHED: the index-vs-HEAD diff, to unstage), -U0. Err: 0 such hunk.
pub(crate) fn hunk_patch(dir: &Path, path: &str, line: u32, cached: bool) -> Result<String, String> {
    let mut args = vec!["diff", "--no-color", "--no-ext-diff", "-U0"];

    if cached {
        args.push("--cached");
    }

    args.extend(["--", path]);
    let diff = git(dir, &args)?;

    pick_hunk(&diff, line)
        .ok_or_else(|| format!("0 {} hunk at {path}:{line}", if cached { "staged" } else { "unstaged" }))
}
/// Header + the hunk of unified diff DIFF (1 file, -U0) whose new side holds LINE (a removal:
/// the line after it).
pub(crate) fn pick_hunk(diff: &str, line: u32) -> Option<String> {
    let mut head = String::new();
    let mut hunks: Vec<(u32, u32, String)> = Vec::new();

    for line_value in diff.lines() {
        if let Some(height) = line_value.strip_prefix("@@ ") {
            let new = height.split(' ').find(|width| width.starts_with('+'))?;
            let mut it = new[1..].split(',');
            let character: u32 = it.next()?.parse().ok()?;
            let directory: u32 = it.next().and_then(|x| x.parse().ok()).unwrap_or(1);

            hunks.push((character, directory, format!("{line_value}\n")));
        } else if let Some(hunk) = hunks.last_mut() {
            hunk.2.push_str(line_value);
            hunk.2.push('\n');
        } else {
            head.push_str(line_value);
            head.push('\n');
        }
    }

    let hit = hunks.into_iter().find(|(character_value, directory_value, _)| {
        if *directory_value == 0 {
            line == *character_value || line == *character_value + 1
        } else {
            line >= *character_value && line < character_value + directory_value
        }
    })?;

    Some(head + &hit.2)
}
