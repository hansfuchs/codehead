use super::{
    Changes, Command, Op, Path, Stdio, commits, file_diff, files_label, git, git_in, hunk_patch, patch, stash_items,
};

impl Op {
    /// The step pushes commits.
    pub(crate) fn pushes(&self) -> bool {
        matches!(self, Op::Push | Op::PushTo(_) | Op::OpenPr)
    }
    pub(crate) fn label(&self) -> String {
        match self {
            Op::Stage(path) => format!("staged {path}"),
            Op::Unstage(path_value) => format!("unstaged {path_value}"),
            Op::Commit(_) => "committed".into(),
            Op::Push => "pushed".into(),
            Op::Update(bytes) => format!("rebased on {bytes}"),
            Op::Continue => "rebase continued".into(),
            Op::Abort => "rebase aborted".into(),
            Op::StageHunk(path_2, line) => format!("staged the hunk at {path_2}:{line}"),
            Op::UnstageHunk(path_3, line_value) => format!("unstaged the hunk at {path_3}:{line_value}"),
            Op::OpenPr => "pull request opened".into(),
            Op::Merge => "merge on green set".into(),
            Op::StageFiles(path_4) => format!("staged {}", files_label(path_4)),
            Op::UnstageFiles(path_5) => format!("unstaged {}", files_label(path_5)),
            Op::StageLines(value) => format!(
                "staged the selected lines of {}",
                files_label(&value.iter().map(|x| x.0.clone()).collect::<Vec<_>>())
            ),
            Op::Stash { items, .. } => format!(
                "stashed {}",
                files_label(&items.iter().map(|x| x.0.clone()).collect::<Vec<_>>())
            ),
            Op::Uncommit { picks, .. } => format!(
                "uncommitted {} commit{}",
                picks.len(),
                if picks.len() == 1 { "" } else { "s" }
            ),
            Op::PushTo(oid) => format!("pushed up to {}", &oid[..oid.len().min(7)]),
        }
    }
}
/// Run OP in DIR: Ok(what happened) / Err(git's 1st error line).
/// The landed state OP should leave (C: the changes before it, NOW: the state before it). None: 0
/// guess (a step that keeps the state, a checkout on the default branch).
pub(crate) fn expected(
    op: &Op,
    changes: &Changes,
    now: Option<crate::domain::landed::Land>,
) -> Option<crate::domain::landed::Land> {
    use crate::domain::landed::Land;
    let task = !matches!(now, None | Some(Land::NoCode | Land::Main));

    match op {
        Op::Commit(_) if task && !changes.staged.is_empty() => {
            let left = changes
                .files
                .iter()
                .filter(|file| changes.staged.get(&file.path) != Some(&true))
                .count();

            Some(match (left, now) {
                (count_value, _) if count_value > 0 => Land::Dirty(count_value),
                (_, Some(Land::Unpushed(count_2))) => Land::Unpushed(count_2 + 1),
                _ => Land::Unpushed(1),
            })
        }
        Op::Push | Op::OpenPr if matches!(now, Some(Land::Unpushed(_))) => Some(Land::Unmerged),
        _ => None,
    }
}
pub(crate) fn run_op(dir: &Path, op: &Op) -> Result<String, String> {
    // a push: the checkout's push lock first (an agent's / shell mode's push runs: 0 push)
    let _lock = match (op.pushes(), crate::infra::pushlock::git_dir(dir, &[])) {
        (true, Some(directory)) => Some(
            crate::infra::pushlock::acquire_in(&directory, "you", std::process::id())
                .map_err(|height| crate::infra::pushlock::busy_text(&height))?,
        ),
        _ => None,
    };

    match op {
        Op::StageFiles(ps) => {
            let mut args = vec!["add", "-A", "--"];

            args.extend(ps.iter().map(String::as_str));
            git(dir, &args)
        }
        Op::UnstageFiles(ps) => {
            let mut args = vec!["restore", "--staged", "--"];

            args.extend(ps.iter().map(String::as_str));

            git(dir, &args).or_else(|_| {
                let mut args = vec!["rm", "--cached", "-q", "--"];

                args.extend(ps.iter().map(String::as_str));
                git(dir, &args)
            })
        }
        Op::StageLines(items) => stage_selected_lines(dir, items)?,
        Op::Stash { thread, rev, items } => stash_items(dir, thread, rev, items).map(|_| String::new()),
        // the note says more than the label (cascaded files, dropped commits)
        Op::Uncommit { thread, head, picks } => {
            return commits::uncommit(dir, picks, head, thread).map(|user| user.note());
        }
        Op::PushTo(oid) => commits::push_to(dir, oid),
        Op::Stage(path_2) => git(dir, &["add", "-A", "--", path_2]),
        Op::Unstage(path_3) => git(dir, &["restore", "--staged", "--", path_3])
            .or_else(|_| git(dir, &["rm", "--cached", "-q", "--", path_3])),
        Op::Commit(msg) => commit_message(dir, msg)?,
        Op::Push => git(dir, &["push", "-u", "origin", "HEAD"]),
        Op::Update(base) => {
            // `origin/main`: fetch origin first; a local branch: 0 fetch
            let remote = base.split_once('/').map(|(result, _)| result).unwrap_or_default();

            if git(dir, &["remote"])
                .unwrap_or_default()
                .lines()
                .any(|result_value| result_value == remote)
            {
                git(dir, &["fetch", "-q", remote])?;
            }

            git(dir, &["-c", "core.editor=true", "rebase", base])
        }
        Op::Continue => git(dir, &["-c", "core.editor=true", "rebase", "--continue"]),
        Op::Abort => git(dir, &["rebase", "--abort"]),
        Op::StageHunk(path_4, line_value) | Op::UnstageHunk(path_4, line_value) => {
            apply_hunk(dir, op, path_4, *line_value)?
        }
        Op::OpenPr | Op::Merge => github_operation(dir, op)?,
    }
    .map(|_| op.label())
}

fn github_operation(dir: &Path, op: &Op) -> Result<Result<String, String>, String> {
    if *op == Op::OpenPr {
        git(dir, &["push", "-u", "origin", "HEAD"])?;
    }

    let args: &[&str] = if *op == Op::OpenPr {
        &["pr", "create", "--fill"]
    } else {
        &["pr", "merge", "--squash", "--auto"]
    };

    let output_value = Command::new("gh")
        .current_dir(dir)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|error_5| format!("gh: {error_5}"))?;

    Ok(if output_value.status.success() {
        Ok(String::from_utf8_lossy(&output_value.stdout).into_owned())
    } else {
        Err(String::from_utf8_lossy(&output_value.stderr)
            .lines()
            .find(|line_2| !line_2.trim().is_empty())
            .unwrap_or("gh failed")
            .to_string())
    })
}

fn commit_message(dir: &Path, msg: &String) -> Result<Result<String, String>, String> {
    use std::io::Write;
    let mut character = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["commit", "-q", "-F", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error_value| format!("git: {error_value}"))?;

    if let Some(mut i) = character.stdin.take() {
        let _ = i.write_all(msg.as_bytes());
    }

    let out = character
        .wait_with_output()
        .map_err(|error_2| format!("git: {error_2}"))?;

    Ok(if out.status.success() {
        Ok(String::new())
    } else {
        let err = String::from_utf8_lossy(&out.stderr).into_owned() + &String::from_utf8_lossy(&out.stdout);

        Err(err
            .lines()
            .find(|line| !line.trim().is_empty())
            .unwrap_or("commit failed")
            .to_string())
    })
}

fn apply_hunk(dir: &Path, op: &Op, path_4: &str, line_value: u32) -> Result<Result<String, String>, String> {
    use std::io::Write;
    let unstage = matches!(op, Op::UnstageHunk(..));
    let patch = hunk_patch(dir, path_4, line_value, unstage)?;
    let mut args = vec!["apply", "--cached", "--unidiff-zero"];

    if unstage {
        args.push("-R");
    }

    args.push("-");
    let mut character_value = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(&args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error_3| format!("git: {error_3}"))?;

    if let Some(mut i) = character_value.stdin.take() {
        let _ = i.write_all(patch.as_bytes());
    }

    let out = character_value
        .wait_with_output()
        .map_err(|error_4| format!("git: {error_4}"))?;

    Ok(if out.status.success() {
        Ok(String::new())
    } else {
        Err(String::from_utf8_lossy(&out.stderr)
            .lines()
            .next()
            .unwrap_or("git apply failed")
            .to_string())
    })
}

fn stage_selected_lines(dir: &Path, items: &Vec<(String, patch::Pick)>) -> Result<Result<String, String>, String> {
    let mut count = 0;

    for (path, pick) in items {
        let diff = file_diff(dir, None, path)?;

        if let Some(path_value) = patch::pick(&diff, pick, patch::Dir::Forward) {
            git_in(dir, &["apply", "--cached", "-"], &path_value).map_err(|error| format!("{path}: {error}"))?;
            count += 1;
        }
    }

    Ok(if count == 0 {
        Err("0 unstaged changes in the selected lines".into())
    } else {
        Ok(String::new())
    })
}
