use super::{Command, FileDiff, Kind, Path, Stdio, git, parse, patch, stash};

/// `git ARGS` in DIR with STDIN as its input.
pub(crate) fn git_in(dir: &Path, args: &[&str], stdin: &str) -> Result<String, String> {
    use std::io::Write;
    let mut child = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("git: {error}"))?;

    if let Some(mut pipe) = child.stdin.take() {
        let _ = pipe.write_all(stdin.as_bytes());
    }

    let out = child.wait_with_output().map_err(|error| format!("git: {error}"))?;

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
/// The diff of PATH in DIR: against REV (None: the index) to the worktree. A file outside the
/// index (and outside REV): a new-file diff.
pub(crate) fn file_diff(dir: &Path, rev: Option<&str>, path: &str) -> Result<String, String> {
    let in_index = git(dir, &["ls-files", "--error-unmatch", "--", path]).is_ok();
    let in_rev = rev.is_some_and(|rev| git(dir, &["cat-file", "-e", &format!("{rev}:{path}")]).is_ok());

    if in_index || in_rev {
        let mut args = vec!["diff", "--no-color", "--no-ext-diff", "-U3"];

        args.extend(rev);
        args.extend(["--", path]);
        return git(dir, &args);
    }

    // `--no-index` exits 1 on a difference
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args([
            "diff",
            "--no-index",
            "--no-color",
            "--no-ext-diff",
            "--",
            "/dev/null",
            path,
        ])
        .stdin(Stdio::null())
        .output()
        .map_err(|error| format!("git: {error}"))?;

    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}
/// PICK (on the worktree's DIFF) on the index's diff CACHED: the picked lines that are staged, by
/// text, in order. None: 0 of them.
pub(crate) fn index_pick(diff: &str, cached: &str, pick: &patch::Pick) -> Option<patch::Pick> {
    let (Some(wt), Some(ix)) = (parse(diff).into_iter().next(), parse(cached).into_iter().next()) else {
        return None;
    };

    if pick.all {
        return Some(patch::Pick {
            all: true,
            ..Default::default()
        });
    }

    // each change as (an add, new-side number at it, text), as `pick_of` numbers them
    let changes = |file: &FileDiff| -> Vec<(bool, u32, String)> {
        let mut list = Vec::new();

        for hunk in &file.hunks {
            let new_line = hunk.new;

            push_hunk_changes(&mut list, hunk, new_line);
        }

        list
    };

    let picked: Vec<(bool, String)> = changes(&wt)
        .into_iter()
        .filter(|(add, line, text)| {
            if *add {
                pick.adds.contains(line)
            } else {
                pick.dels.contains(&(*line, text.clone()))
            }
        })
        .map(|(add, _, text)| (add, text))
        .collect();

    let mut out = patch::Pick::default();
    let mut j = 0;

    for (add, line, text) in changes(&ix) {
        if let Some(offset) = picked[j..]
            .iter()
            .position(|(picked_add, picked_text)| *picked_add == add && *picked_text == text)
        {
            j += offset + 1;

            insert_pick(&mut out, add, line, text);
        }
    }

    (!out.is_empty()).then_some(out)
}

fn push_hunk_changes(list: &mut Vec<(bool, u32, String)>, hunk: &super::Hunk, mut new_line: u32) {
    for line in &hunk.lines {
        match line.kind {
            Kind::Add => {
                list.push((true, new_line, line.text.clone()));
                new_line += 1;
            }
            Kind::Ctx => new_line += 1,
            Kind::Del => list.push((false, new_line, line.text.clone())),
        }
    }
}

fn insert_pick(out: &mut patch::Pick, add: bool, line: u32, text: String) {
    if add {
        out.adds.insert(line);
    } else {
        out.dels.insert((line, text));
    }
}
/// Stash ITEMS of DIR (diffs against REV) in THREAD's stash: each picked part out of the worktree
/// (and out of the index, where it is staged), its patch in a new entry. Ok: the entries' count.
pub(crate) fn stash_items(
    dir: &Path,
    thread: &str,
    rev: &str,
    items: &[(String, String, patch::Pick)],
) -> Result<usize, String> {
    let mut made = Vec::new();

    for (path, lines, pick) in items {
        let diff = file_diff(dir, Some(rev), path)?;

        let Some(reverse_patch) = patch::pick(&diff, pick, patch::Dir::Reverse) else {
            continue;
        };

        // staged too: out of the index. The index has its own lines (0 unstaged one): the same
        // picks on its diff, else 0 match there
        let cached = git(
            dir,
            &[
                "diff",
                "--cached",
                "--no-color",
                "--no-ext-diff",
                "-U3",
                rev,
                "--",
                path,
            ],
        )
        .unwrap_or_default();
        let staged = index_pick(&diff, &cached, pick)
            .and_then(|staged_pick| patch::pick(&cached, &staged_pick, patch::Dir::Reverse));

        git_in(dir, &["apply", "-R", "-"], &reverse_patch).map_err(|error| format!("{path}: {error}"))?;

        if let Some(staged_patch) = staged {
            git_in(dir, &["apply", "-R", "--cached", "-"], &staged_patch)
                .map_err(|error| format!("{path} (index): {error}"))?;
        }

        let agent = crate::engine::edits::writer(thread, dir, path);
        let id = format!("{}{}", stash::new_id(), made.len());

        made.push(stash::Entry {
            id,
            path: path.clone(),
            lines: lines.clone(),
            patch: reverse_patch,
            agent,
            ts: crate::engine::now_ms(),
            ..Default::default()
        });
    }

    if made.is_empty() {
        return Err("0 changes to stash".into());
    }

    let count = made.len();
    let mut all = stash::read(thread);

    all.extend(made);
    stash::write(thread, &all);

    Ok(count)
}
