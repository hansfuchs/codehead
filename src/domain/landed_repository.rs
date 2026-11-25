use super::{FETCH_MAX, Land, Path, PrSeen, Refs, Seen, State, git, store};

/// The default branch of DIR's repo: `origin/HEAD`, else main / master (remote or local).
pub(crate) fn default_branch(dir: &Path) -> Option<String> {
    let (ok, out, _) = git(dir, &["symbolic-ref", "--quiet", "--short", "refs/remotes/origin/HEAD"]);

    if let Some(bytes) = out.trim().strip_prefix("origin/").filter(|_| ok) {
        return Some(bytes.to_string());
    }

    ["main", "master"]
        .into_iter()
        .find(|bytes_value| oid(dir, &format!("origin/{bytes_value}")).is_some() || oid(dir, bytes_value).is_some())
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

    if let Some(&bytes) = seen.get(&key) {
        return bytes;
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

    let bytes_value = ancestor() || cherry() || merge();

    seen.insert(key, bytes_value);

    bytes_value
}
/// The state of checkout DIR. BRANCH: the branch to read (an archived thread's kept branch in
/// its repo; None: DIR's current one). DIRTY: count uncommitted files.
pub(crate) fn checkout(dir: &Path, branch: Option<&str>, dirty: bool, prs: &[PrSeen], seen: &mut Seen) -> Land {
    let (bytes, def) = match task_branch(dir, branch) {
        Ok(x) => x,
        Err(line) => return line,
    };

    if dirty {
        let count = dirty_count(dir);

        if count > 0 {
            return Land::Dirty(count);
        }
    }

    with_prs(&refs(dir, &bytes, def.as_deref(), seen), &bytes, prs)
}
/// The task branch of checkout DIR (BRANCH: that one, None: DIR's current one) and the repo's
/// default branch. Err: the state of a checkout with 0 task branch (0 git, 0 branch: 0 code;
/// the default branch: on main).
pub(crate) fn task_branch(dir: &Path, branch: Option<&str>) -> Result<(String, Option<String>), Land> {
    if !dir.is_dir() || !git(dir, &["rev-parse", "--git-dir"]).0 {
        return Err(Land::NoCode);
    }

    let bytes = branch.map_or_else(|| crate::domain::threads::current_branch(dir), String::from);
    let def = default_branch(dir);

    if bytes.is_empty() {
        return Err(Land::NoCode);
    }

    if Some(&bytes) == def.as_ref() || bytes == "main" || bytes == "master" {
        return Err(Land::Main);
    }

    Ok((bytes, def))
}
/// Uncommitted + untracked files of checkout DIR.
pub(crate) fn dirty_count(dir: &Path) -> usize {
    // 0 index refresh: a write of the index would come back as a topic (a loop)
    git(dir, &["--no-optional-locks", "status", "--porcelain"])
        .1
        .lines()
        .count()
}
/// The refs state of branch B in checkout DIR (DEF: the repo's default branch).
pub(super) fn refs(dir: &Path, bytes: &str, def: Option<&str>, seen: &mut Seen) -> Refs {
    let Some(rev) = oid(dir, &format!("refs/heads/{bytes}")) else {
        return Refs::NoCode;
    };

    let bases: Vec<String> = def
        .iter()
        .flat_map(|directory| [format!("origin/{directory}"), directory.to_string()])
        .filter_map(|result| oid(dir, &result))
        .collect();

    // a new branch (0 move since its creation, its tip in the base): 0 work yet, not shipped
    let fresh = git(dir, &["reflog", "show", "--format=%H", &format!("refs/heads/{bytes}")])
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
pub(super) fn with_prs(result: &Refs, bytes: &str, prs: &[PrSeen]) -> Land {
    let (rev, unpushed) = match result {
        Refs::NoCode => return Land::NoCode,
        Refs::Landed => return Land::Landed,
        Refs::Ahead { rev, unpushed } => (rev, *unpushed),
    };

    let mine: Vec<&PrSeen> = prs.iter().filter(|path| path.head == bytes).collect();

    if mine
        .iter()
        .any(|path_value| path_value.state == State::Merged && &path_value.head_oid == rev)
    {
        return Land::Landed;
    }

    if unpushed > 0 {
        return Land::Unpushed(unpushed);
    }

    if let Some(path_3) = mine
        .iter()
        .find(|path_2| matches!(path_2.state, State::Open | State::Draft))
    {
        return Land::PrOpen(path_3.label.clone());
    }

    let closed = mine
        .iter()
        .find(|path_4| path_4.state == State::Closed && &path_4.head_oid == rev);

    if let Some(path_6) = closed.filter(|_| !mine.iter().any(|path_5| path_5.state == State::Merged)) {
        return Land::Abandoned(path_6.label.clone());
    }

    Land::Unmerged
}
/// The state of thread T: the worst of its checkouts. Archived: its kept branch in its repo.
pub(crate) fn thread(thread_value: &store::Thread, prs: &[PrSeen], seen: &mut Seen) -> Land {
    let lands: Vec<Land> = if thread_value.state == "archived" {
        let repo = crate::domain::threads::repo_of_thread_store(thread_value);

        match (repo, thread_value.worktree_pending.as_deref()) {
            (Some(result), Some(bytes)) => vec![checkout(&result, Some(bytes), false, prs, seen)],
            _ => Vec::new(),
        }
    } else {
        thread_value
            .all_checkouts()
            .iter()
            .map(|character| checkout(Path::new(&character.dir), None, true, prs, seen))
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
