use super::{HashSet, Path, Uncommitted, git, run, short, upstream};

/// The backup ref of THREAD's last uncommit (the old HEAD).
pub(crate) fn backup_ref(thread: &str) -> String {
    let safe: String = thread
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '-' || character == '_' {
                character
            } else {
                '-'
            }
        })
        .collect();

    format!("refs/codehead/uncommit/{safe}")
}
/// Uncommit PICKS (oids) in DIR: they leave the history, the later commits get replayed on top
/// of what stays. The worktree stays byte for byte; the picked changes show as uncommitted. A
/// later commit's change that conflicts with the change taken out: out of the history too
/// (cascade). HEAD must still be HEAD (else an agent committed meanwhile: 0 change).
pub(crate) fn uncommit(dir: &Path, picks: &[String], head: &str, thread: &str) -> Result<Uncommitted, String> {
    let now = git(dir, &["rev-parse", "HEAD"])?.trim().to_string();

    if now != head {
        return Err("HEAD moved meanwhile: read again (r), pick again".into());
    }

    if picks.is_empty() {
        return Err("0 commits picked".into());
    }

    if crate::domain::changes::stopped(dir) {
        return Err("a rebase / merge stopped: finish it first".into());
    }

    if let Some(tracking) = upstream(dir) {
        let local: HashSet<String> = git(dir, &["rev-list", "HEAD", &format!("^{tracking}")])?
            .lines()
            .map(str::to_string)
            .collect();

        if let Some(pushed) = picks.iter().find(|oid| !local.contains(*oid)) {
            return Err(format!("{}: pushed (0 uncommit)", short(pushed)));
        }
    }

    // the first-parent chain from HEAD down to the oldest pick
    let picked: HashSet<&str> = picks.iter().map(String::as_str).collect();
    let chain = git(dir, &["rev-list", "--first-parent", "--parents", "-n2000", "HEAD"])?;
    let range: Vec<(String, Vec<String>)> = Vec::new();
    let found = 0;

    replay_uncommitted_history(dir, thread, &now, &picked, &chain, range, found)
}

fn replay_uncommitted_history(
    dir: &Path,
    thread: &str,
    now: &str,
    picked: &HashSet<&str>,
    chain: &str,
    mut range: Vec<(String, Vec<String>)>,
    mut found: usize,
) -> Result<Uncommitted, String> {
    for line in chain.lines() {
        let mut it = line.split_whitespace();
        let Some(oid) = it.next() else { continue };
        let parents: Vec<String> = it.map(str::to_string).collect();

        if parents.len() > 1 {
            return Err(format!("{}: a merge commit in the range (0 uncommit)", short(oid)));
        }

        if let Some(error) = count_picked(picked, &mut found, oid, &parents) {
            return error;
        }

        range.push((oid.to_string(), parents));

        if found == picked.len() {
            break;
        }
    }

    if found < picked.len() {
        return Err("a picked commit is not on the branch's first-parent line".into());
    }

    range.reverse();
    let mut cur = range[0].1[0].clone();
    let mut done = Uncommitted {
        count: picked.len(),
        ..Default::default()
    };

    for (oid, parents) in &range {
        if picked.contains(oid.as_str()) {
            continue;
        }

        let tree = replay(dir, &cur, oid, &parents[0], &mut done)?;
        let cur_tree = git(dir, &["rev-parse", &format!("{cur}^{{tree}}")])?.trim().to_string();

        if tree == cur_tree {
            done.dropped.push(short(oid).to_string());
            continue;
        }

        cur = recommit(dir, oid, &tree, &cur)?;
    }

    move_head(dir, now, &cur, thread, &mut done)?;

    Ok(done)
}

fn count_picked(
    picked: &HashSet<&str>,
    found: &mut usize,
    oid: &str,
    parents: &[String],
) -> Option<Result<Uncommitted, String>> {
    if picked.contains(oid) {
        if let Some(error) = reject_root_commit(oid, parents) {
            return Some(error);
        }

        *found += 1;
    }

    None
}

fn reject_root_commit(oid: &str, parents: &[String]) -> Option<Result<Uncommitted, String>> {
    if parents.is_empty() {
        return Some(Err(format!("{}: the root commit (0 uncommit)", short(oid))));
    }

    None
}

/// HEAD from OLD to NEW with the index along, under the index lock (0 agent `git commit` / `add`
/// between the two): the new index = a 2-way merge (`read-tree -m OLD NEW`) of a copy of the
/// index (staged entries, skip-worktree bits, stat data stay; a path both staged and changed by
/// the move: the index stays as it was, DONE says so). The old HEAD: THREAD's backup ref.
pub(crate) fn move_head(dir: &Path, old: &str, new: &str, thread: &str, done: &mut Uncommitted) -> Result<(), String> {
    let index =
        std::path::PathBuf::from(git(dir, &["rev-parse", "--path-format=absolute", "--git-path", "index"])?.trim());
    let lock = index.with_extension("lock");
    let tmp = index.with_extension("codehead-uncommit");

    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&lock)
        .map_err(|_| "the index is locked (an agent's git runs): try again".to_string())?;

    // the lock is ours until the rename puts it in the index's place (then another git may take a
    // new one: 0 remove after that)
    let mut renamed = false;
    let result = (|| {
        // a missing index (0 file yet): the merge starts from an empty one
        let _ = std::fs::remove_file(&tmp);

        if index.exists() {
            std::fs::copy(&index, &tmp).map_err(|error| format!("index copy: {error}"))?;
        }

        let env = [("GIT_INDEX_FILE", tmp.to_string_lossy().into_owned())];
        let merged = run(dir, &["read-tree", "-m", old, new], &env, None).is_ok();

        git(
            dir,
            &[
                "update-ref",
                "--create-reflog",
                "-m",
                "codehead: uncommit backup",
                &backup_ref(thread),
                old,
            ],
        )?;
        git(dir, &["update-ref", "-m", "codehead: uncommit", "HEAD", new, old])
            .map_err(|error| format!("HEAD moved meanwhile: {error}"))?;

        // HEAD moved: the new index in place through the lock file (as git writes it); a write
        // error: the old index stays (the note says so), 0 Err after HEAD moved
        renamed = merged && std::fs::copy(&tmp, &lock).is_ok() && std::fs::rename(&lock, &index).is_ok();
        done.index_kept = !renamed;
        Ok(())
    })();

    let _ = std::fs::remove_file(&tmp);

    if !renamed {
        let _ = std::fs::remove_file(&lock);
    }

    if result.is_ok() {
        let _ = git(dir, &["update-index", "-q", "--refresh"]);
    }

    result
}
/// Commit C (parent P) replayed on CUR: its tree. A conflicted path keeps CUR's version (C's
/// change of it: out of the history, noted in DONE).
pub(crate) fn replay(
    dir: &Path,
    cur: &str,
    commit: &str,
    parent: &str,
    done: &mut Uncommitted,
) -> Result<String, String> {
    let base = format!("--merge-base={parent}");

    let (tree, conflicted) = match run(
        dir,
        &[
            "merge-tree",
            "--write-tree",
            "--name-only",
            "--no-messages",
            &base,
            cur,
            commit,
        ],
        &[],
        None,
    ) {
        Ok(output) => (output.lines().next().unwrap_or_default().to_string(), Vec::new()),
        Err((1, output, _)) => parse_conflicts(&output),
        // git < 2.40 (0 `--merge-base`): the newest commits still uncommit (0 replay)
        Err((_, _, error)) => {
            return Err(format!(
                "merge-tree (git 2.40+; older git: pick the newest commits only): {error}"
            ));
        }
    };

    if conflicted.is_empty() {
        return Ok(tree);
    }

    // a temp index: the merged tree, each conflicted path as CUR has it
    let idx = std::env::temp_dir().join(format!("codehead-uncommit-{}-{}", std::process::id(), short(commit)));
    let env = [("GIT_INDEX_FILE", idx.to_string_lossy().into_owned())];
    let result = (|| {
        run(dir, &["read-tree", &tree], &env, None).map_err(|error| error.2)?;

        restore_conflicted_paths(dir, cur, commit, done, &conflicted, &env)?;

        run(dir, &["write-tree"], &env, None)
            .map(|output| output.trim().to_string())
            .map_err(|error| error.2)
    })();
    let _ = std::fs::remove_file(&idx);

    result
}

fn restore_conflicted_paths(
    dir: &Path,
    cur: &str,
    commit: &str,
    done: &mut Uncommitted,
    conflicted: &[String],
    env: &[(&str, String); 1],
) -> Result<(), String> {
    for path in conflicted {
        let ls = git(dir, &["ls-tree", cur, "--", path])?;

        match ls.split_whitespace().collect::<Vec<_>>().as_slice() {
            [mode, _, blob, ..] => {
                stage_blob(dir, env, path, mode, blob)?;
            }
            _ => {
                run(dir, &["update-index", "--force-remove", "--", path], env, None).map_err(|error| error.2)?;
            }
        }

        done.cascaded.push((short(commit).to_string(), path.clone()));
    }

    Ok(())
}

fn stage_blob(dir: &Path, env: &[(&str, String); 1], path: &String, mode: &&str, blob: &&str) -> Result<(), String> {
    run(
        dir,
        &["update-index", "--add", "--cacheinfo", &format!("{mode},{blob},{path}")],
        env,
        None,
    )
    .map_err(|error| error.2)?;
    Ok(())
}

fn parse_conflicts(output: &str) -> (String, Vec<String>) {
    let mut lines = output.lines();
    let tree = lines.next().unwrap_or_default().to_string();
    let mut paths: Vec<String> = Vec::new();

    for path in lines.take_while(|line| !line.is_empty()) {
        if !paths.iter().any(|known| known == path) {
            paths.push(path.to_string());
        }
    }

    (tree, paths)
}
/// A copy of commit C with TREE on PARENT: C's author, committer, dates, message.
pub(crate) fn recommit(dir: &Path, commit: &str, tree: &str, parent: &str) -> Result<String, String> {
    let meta = git(
        dir,
        &[
            "log",
            "-1",
            "--format=%an%x00%ae%x00%ad%x00%cn%x00%ce%x00%cd",
            "--date=raw",
            commit,
        ],
    )?;
    let fields: Vec<&str> = meta.trim_end_matches('\n').split('\0').collect();

    if fields.len() < 6 {
        return Err(format!("{}: 0 metadata", short(commit)));
    }

    // %B ends in a line break of its own: 1 line break kept (0 more per rewrite)
    let msg = format!(
        "{}\n",
        git(dir, &["log", "-1", "--format=%B", commit])?.trim_end_matches('\n')
    );
    let env = [
        ("GIT_AUTHOR_NAME", fields[0].to_string()),
        ("GIT_AUTHOR_EMAIL", fields[1].to_string()),
        ("GIT_AUTHOR_DATE", fields[2].to_string()),
        ("GIT_COMMITTER_NAME", fields[3].to_string()),
        ("GIT_COMMITTER_EMAIL", fields[4].to_string()),
        ("GIT_COMMITTER_DATE", fields[5].to_string()),
    ];

    run(dir, &["commit-tree", tree, "-p", parent, "-F", "-"], &env, Some(&msg))
        .map(|output| output.trim().to_string())
        .map_err(|error| format!("{}: {}", short(commit), error.2))
}
