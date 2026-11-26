use super::{FETCH_MAX, Land, Path, PrSeen, Refs, Seen, State, git, store};

/// The default branch of DIR's repo: `origin/HEAD`, else main / master (remote or local).
pub(crate) fn default_branch(dir: &Path) -> Option<String> {
    let (ok, out, _) = git(dir, &["symbolic-ref", "--quiet", "--short", "refs/remotes/origin/HEAD"]);

    if let Some(branch) = out.trim().strip_prefix("origin/").filter(|_| ok) {
        return Some(branch.to_string());
    }

    ["main", "master"]
        .into_iter()
        .find(|branch| oid(dir, &format!("origin/{branch}")).is_some() || oid(dir, branch).is_some())
        .map(String::from)
}
pub(crate) fn oid(dir: &Path, rev: &str) -> Option<String> {
    let (ok, out, _) = git(dir, &["rev-parse", "--verify", "--quiet", &format!("{rev}^{{commit}}")]);
    let output = out.trim().to_string();

    (ok && !output.is_empty()).then_some(output)
}
/// The content of commit REV is in commit BASE: ancestor, same patches, or a merge of REV into
/// BASE changes 0 (squash merge).
pub(crate) fn contained(dir: &Path, rev: &str, base: &str, seen: &mut Seen) -> bool {
    let key = (rev.to_string(), base.to_string());

    if let Some(&landed) = seen.get(&key) {
        return landed;
    }

    let ancestor = || git(dir, &["merge-base", "--is-ancestor", rev, base]).0;

    let cherry = || {
        let (ok, out, _) = git(dir, &["cherry", base, rev]);

        ok && out.lines().all(|line| line.starts_with('-'))
    };

    let merge = || {
        let (ok, out, _) = git(dir, &["merge-tree", "--write-tree", base, rev]);
        let tree = git(dir, &["rev-parse", &format!("{base}^{{tree}}")]).1;

        ok && out.lines().next().map(str::trim) == Some(tree.trim())
    };

    let landed = ancestor() || cherry() || merge();

    seen.insert(key, landed);

    landed
}
/// The state of checkout DIR. BRANCH: the branch to read (an archived thread's kept branch in
/// its repo; None: DIR's current one). DIRTY: count uncommitted files.
pub(crate) fn checkout(dir: &Path, branch: Option<&str>, dirty: bool, prs: &[PrSeen], seen: &mut Seen) -> Land {
    let (branch, def) = match task_branch(dir, branch) {
        Ok(found) => found,
        Err(land) => return land,
    };

    if dirty {
        let count = dirty_count(dir);

        if count > 0 {
            return Land::Dirty(count);
        }
    }

    with_prs(&refs(dir, &branch, def.as_deref(), seen), &branch, prs)
}
/// The task branch of checkout DIR (BRANCH: that one, None: DIR's current one) and the repo's
/// default branch. Err: the state of a checkout with 0 task branch (0 git, 0 branch: 0 code;
/// the default branch: on main).
pub(crate) fn task_branch(dir: &Path, branch: Option<&str>) -> Result<(String, Option<String>), Land> {
    if !dir.is_dir() || !git(dir, &["rev-parse", "--git-dir"]).0 {
        return Err(Land::NoCode);
    }

    let current = branch.map_or_else(|| crate::domain::threads::current_branch(dir), String::from);
    let def = default_branch(dir);

    if current.is_empty() {
        return Err(Land::NoCode);
    }

    if Some(&current) == def.as_ref() || current == "main" || current == "master" {
        return Err(Land::Main);
    }

    Ok((current, def))
}
/// Uncommitted + untracked files of checkout DIR.
pub(crate) fn dirty_count(dir: &Path) -> usize {
    // 0 index refresh: a write of the index would come back as a topic (a loop); every untracked
    // file on its own line: a dir that holds only a nested worktree (`.claude/` of a subagent's)
    // shows as that worktree's `dir/`, which counts 0
    let status = git(
        dir,
        &["--no-optional-locks", "status", "--porcelain", "--untracked-files=all"],
    )
    .1;

    status.lines().filter(|line| counts_as_dirty(line)).count()
}
/// LINE of `git status --porcelain`: work of the checkout (0: a nested repo, a temp file).
pub(crate) fn counts_as_dirty(line: &str) -> bool {
    let Some(path) = line.strip_prefix("?? ") else {
        return !line.is_empty();
    };

    !path.ends_with('/') && !crate::infra::stray_paths::transient(path)
}
/// The refs state of branch B in checkout DIR (DEF: the repo's default branch).
pub(super) fn refs(dir: &Path, branch: &str, def: Option<&str>, seen: &mut Seen) -> Refs {
    let Some(rev) = oid(dir, &format!("refs/heads/{branch}")) else {
        return Refs::NoCode;
    };

    let bases: Vec<String> = def
        .iter()
        .flat_map(|name| [format!("origin/{name}"), name.to_string()])
        .filter_map(|rev| oid(dir, &rev))
        .collect();

    // a new branch (0 move since its creation, its tip in the base): 0 work yet, not shipped
    let fresh = git(dir, &["reflog", "show", "--format=%H", &format!("refs/heads/{branch}")])
        .1
        .lines()
        .count()
        == 1;

    if fresh
        && bases
            .iter()
            .any(|base| git(dir, &["merge-base", "--is-ancestor", &rev, base]).0)
    {
        return Refs::NoCode;
    }

    if bases.iter().any(|base| contained(dir, &rev, base, seen)) {
        return Refs::Landed;
    }

    let unpushed = git(dir, &["rev-list", "--count", &rev, "--not", "--remotes"])
        .1
        .trim()
        .parse()
        .unwrap_or(0);

    Refs::Ahead { rev, unpushed }
}
/// The state of branch B with refs state R and the thread's PRS.
pub(super) fn with_prs(refs: &Refs, branch: &str, prs: &[PrSeen]) -> Land {
    let (rev, unpushed) = match refs {
        Refs::NoCode => return Land::NoCode,
        Refs::Landed => return Land::Landed,
        Refs::Ahead { rev, unpushed } => (rev, *unpushed),
    };

    let mine: Vec<&PrSeen> = prs.iter().filter(|pr| pr.head == branch).collect();

    if mine.iter().any(|pr| pr.state == State::Merged && &pr.head_oid == rev) {
        return Land::Landed;
    }

    if unpushed > 0 {
        return Land::Unpushed(unpushed);
    }

    if let Some(pr) = mine.iter().find(|pr| matches!(pr.state, State::Open | State::Draft)) {
        return Land::PrOpen(pr.label.clone());
    }

    let closed = mine.iter().find(|pr| pr.state == State::Closed && &pr.head_oid == rev);

    if let Some(pr) = closed.filter(|_| !mine.iter().any(|pr| pr.state == State::Merged)) {
        return Land::Abandoned(pr.label.clone());
    }

    Land::Unmerged
}
/// The state of thread T: the worst of its checkouts. Archived: its kept branch in its repo.
pub(crate) fn thread(thread: &store::Thread, prs: &[PrSeen], seen: &mut Seen) -> Land {
    let lands: Vec<Land> = if thread.state == "archived" {
        let repo = crate::domain::threads::repo_of_thread_store(thread);

        match (repo, thread.worktree_pending.as_deref()) {
            (Some(repo), Some(branch)) => vec![checkout(&repo, Some(branch), false, prs, seen)],
            _ => Vec::new(),
        }
    } else {
        thread
            .all_checkouts()
            .iter()
            .map(|co| checkout(Path::new(&co.dir), None, true, prs, seen))
            .collect()
    };

    lands.into_iter().min_by_key(Land::rank).unwrap_or(Land::NoCode)
}
/// `git fetch origin` in REPO: 0 prompt, `FETCH_MAX` at most.
pub(crate) fn fetch(repo: &Path) {
    let argv = ["git", "fetch", "--quiet", "origin"].map(String::from);
    let env = [("GIT_TERMINAL_PROMPT".to_string(), "0".to_string())];
    let _ = crate::infra::worktree_setup::run(&argv, repo, &env, FETCH_MAX);
}
