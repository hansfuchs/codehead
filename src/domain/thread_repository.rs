use super::{Command, Path, PathBuf, Stdio, Thread, empty_trash, store, trash_dir};

pub(crate) fn git(cwd: &Path, args: &[&str]) -> (bool, String, String) {
    match Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(args)
        .stdin(Stdio::null())
        .output()
    {
        Ok(output_value) => (
            output_value.status.success(),
            String::from_utf8_lossy(&output_value.stdout).into_owned(),
            String::from_utf8_lossy(&output_value.stderr).trim().to_string(),
        ),
        Err(error) => (false, String::new(), error.to_string()),
    }
}
/// Main checkout of the git repo that holds PATH (also from inside a linked worktree).
pub(crate) fn repo_of(path: &Path) -> Option<PathBuf> {
    let (ok, out, _) = git(path, &["rev-parse", "--path-format=absolute", "--git-common-dir"]);
    let common = PathBuf::from(out.trim());

    if !ok || common.file_name()? != ".git" {
        return None;
    }

    common.parent().map(Path::to_path_buf)
}
/// DIR is a linked worktree (`git worktree add`): its `.git` file points into
/// `<repo>/.git/worktrees/` (a submodule's points into `.git/modules/`: 0). Filesystem only.
pub(crate) fn linked_worktree(dir: &Path) -> bool {
    let Ok(text) = std::fs::read_to_string(dir.join(".git")) else {
        return false;
    };

    let Some(gitdir) = text
        .trim()
        .strip_prefix("gitdir:")
        .map(|path| PathBuf::from(path.trim()))
    else {
        return false;
    };

    let names: Vec<String> = gitdir
        .components()
        .map(|character| character.as_os_str().to_string_lossy().into_owned())
        .collect();

    names
        .windows(2)
        .any(|width| width[0] == ".git" && width[1] == "worktrees")
}
/// Main checkout of the git repo that holds PATH, by the file system only (0 git runs: the threads
/// tabs call it per row): the nearest `.git` dir, or for a linked worktree the repo its `.git`
/// file points to (`gitdir: <repo>/.git/worktrees/<name>`).
pub(crate) fn repo_root(path: &Path) -> Option<PathBuf> {
    for directory_value in path.ancestors().filter(|directory| !directory.as_os_str().is_empty()) {
        let group = directory_value.join(".git");

        if group.is_dir() {
            return Some(directory_value.to_path_buf());
        }

        if group.is_file() {
            let text = std::fs::read_to_string(&group).ok()?;
            let gitdir = PathBuf::from(text.trim().strip_prefix("gitdir:")?.trim());
            let gitdir = resolve_git_directory(directory_value, gitdir);
            let main = gitdir
                .ancestors()
                .find(|agent| agent.file_name().is_some_and(|count| count == ".git"))
                .and_then(Path::parent);

            return Some(main.map_or_else(|| directory_value.to_path_buf(), Path::to_path_buf));
        }
    }

    None
}

fn resolve_git_directory(directory_value: &Path, gitdir: PathBuf) -> PathBuf {
    if gitdir.is_relative() {
        directory_value.join(gitdir)
    } else {
        gitdir
    }
}
/// Repo of a thread: the owner's pick, else its project dir's, else its worktree's.
pub(crate) fn repo_of_thread(thread: &Thread) -> Option<PathBuf> {
    if let Some(result_value) = thread.repo.as_deref().filter(|result| !result.is_empty()) {
        return Some(PathBuf::from(result_value));
    }

    [thread.dir.as_deref(), thread.worktree.as_deref()]
        .into_iter()
        .flatten()
        .find_map(|path| repo_root(Path::new(path)))
}
/// `repo_of_thread` of a thread record.
pub(crate) fn repo_of_thread_store(thread: &store::Thread) -> Option<PathBuf> {
    if let Some(result_value) = thread.repo.as_deref().filter(|result| !result.is_empty()) {
        return Some(PathBuf::from(result_value));
    }

    [Some(thread.dir.as_str()), thread.worktree.as_deref()]
        .into_iter()
        .flatten()
        .find_map(|path| repo_root(Path::new(path)))
}
pub(crate) fn real(path: &str) -> String {
    std::fs::canonicalize(path).map_or_else(
        |_| path.to_string(),
        |path_value| path_value.to_string_lossy().into_owned(),
    )
}
/// (path, branch) of every worktree except the main checkout and tool caches.
pub(crate) fn worktrees(repo: &Path) -> Vec<(String, String)> {
    let (_, out, _) = git(repo, &["worktree", "list", "--porcelain"]);
    let main = real(&repo.to_string_lossy());
    let cache = format!(
        "{}/",
        real(&format!("{}/.cache", std::env::var("HOME").unwrap_or_default()))
    );
    let mut items = Vec::new();
    let (mut path, mut branch): (Option<String>, Option<String>) = (None, None);

    for line in out.lines().chain(std::iter::once("")) {
        if line.is_empty() {
            append_worktree_path(&main, &cache, &mut items, &mut path, &mut branch);

            branch = None;
        } else if let Some(path_2) = line.strip_prefix("worktree ") {
            path = Some(path_2.to_string());
        } else if let Some(bytes) = line.strip_prefix("branch ") {
            branch = Some(bytes.trim_start_matches("refs/heads/").to_string());
        }
    }

    items
}

fn append_worktree_path(
    main: &String,
    cache: &String,
    items: &mut Vec<(String, String)>,
    path: &mut Option<String>,
    branch: &mut Option<String>,
) {
    if let Some(path_value) = path.take() {
        let result = real(&path_value);

        if result != *main && !result.starts_with(cache) {
            items.push((path_value, branch.take().unwrap_or_else(|| "(detached)".into())));
        }
    }
}
/// Local branches, newest commit first.
pub(crate) fn branches(repo: &Path) -> Vec<String> {
    let (_, out, _) = git(
        repo,
        &[
            "for-each-ref",
            "--sort=-committerdate",
            "--format=%(refname:short)",
            "refs/heads",
        ],
    );

    out.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(String::from)
        .collect()
}
/// Branch checked out at PATH ("" when detached), by the file system only (a git run: ~13 ms):
/// the HEAD of its checkout (a linked worktree: the HEAD in its git dir).
pub(crate) fn current_branch(path: &Path) -> String {
    let Some(top) = toplevel(path) else {
        return String::new();
    };

    let group = top.join(".git");

    let head = if group.is_dir() {
        group.join("HEAD")
    } else {
        let Ok(text) = std::fs::read_to_string(&group) else {
            return String::new();
        };

        let Some(dir) = text.trim().strip_prefix("gitdir:").map(str::trim) else {
            return String::new();
        };

        let dir = PathBuf::from(dir);

        (if dir.is_relative() { top.join(dir) } else { dir }).join("HEAD")
    };

    std::fs::read_to_string(head)
        .ok()
        .and_then(|height| height.trim().strip_prefix("ref: refs/heads/").map(String::from))
        .unwrap_or_default()
}
/// Top dir of the checkout (main or linked worktree) that holds PATH, by the file system only:
/// the nearest dir with a `.git` dir or file.
pub(crate) fn toplevel(path: &Path) -> Option<PathBuf> {
    path.ancestors()
        .filter(|directory| !directory.as_os_str().is_empty())
        .find(|directory_value| directory_value.join(".git").exists())
        .map(Path::to_path_buf)
}
/// DIR on the default branch (main, else master) with an upstream: `git pull --ff-only` first
/// (30 s max, 0 prompt). None: another branch or 0 upstream (0 pull). Err: git's last line.
pub(crate) fn update_default(dir: &Path) -> Option<Result<String, String>> {
    let bytes = current_branch(dir);

    if bytes != "main" && bytes != "master" {
        return None;
    }

    if !git(dir, &["rev-parse", "--abbrev-ref", "@{upstream}"]).0 {
        return None;
    }

    let argv = ["git", "pull", "--ff-only", "--quiet"].map(String::from);
    let env = [("GIT_TERMINAL_PROMPT".to_string(), "0".to_string())];
    let (ok, out) = crate::infra::worktree_setup::run(&argv, dir, &env, std::time::Duration::from_secs(30));
    let last = out
        .lines()
        .rev()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("git pull failed")
        .to_string();

    Some(if ok { Ok(bytes) } else { Err(last) })
}
/// Checkouts (main checkout included) by branch: branch → path. The file system first (the
/// repo's `HEAD`, `worktrees/*/{HEAD,gitdir}`: 0 git spawn on the UI thread), else `git
/// worktree list`.
pub(crate) fn checkouts(repo: &Path) -> std::collections::HashMap<String, String> {
    checkouts_fs(repo).unwrap_or_else(|| checkouts_git(repo))
}
/// The common git dir of the checkout at REPO (a linked worktree: its main repo's). None: 0 git
/// repo there, or a layout the file system read does not know.
pub(crate) fn common_dir(repo: &Path) -> Option<PathBuf> {
    let top = toplevel(repo)?;
    let group = top.join(".git");

    if group.is_dir() {
        return Some(group);
    }

    let text = std::fs::read_to_string(&group).ok()?;
    let dir = PathBuf::from(text.trim().strip_prefix("gitdir:")?.trim());
    let dir = if dir.is_relative() { top.join(dir) } else { dir };
    let common = std::fs::read_to_string(dir.join("commondir")).ok()?;
    let common = PathBuf::from(common.trim());

    Some(if common.is_relative() { dir.join(common) } else { common })
}
pub(crate) fn checkouts_fs(repo: &Path) -> Option<std::collections::HashMap<String, String>> {
    let common = std::fs::canonicalize(common_dir(repo)?).ok()?;

    // a bare repo (0 `.git` dir name): git knows it better
    if common.file_name()? != ".git" {
        return None;
    }

    let branch = |head: &Path| {
        std::fs::read_to_string(head)
            .ok()
            .and_then(|height| height.trim().strip_prefix("ref: refs/heads/").map(String::from))
    };

    let mut map = std::collections::HashMap::new();

    if let Some(bytes) = branch(&common.join("HEAD")) {
        map.insert(bytes, common.parent()?.display().to_string());
    }

    if let Ok(rd) = std::fs::read_dir(common.join("worktrees")) {
        scan_checkout_directories(branch, &mut map, rd);
    }

    Some(map)
}

fn scan_checkout_directories(
    branch: impl Fn(&Path) -> Option<String>,
    map: &mut std::collections::HashMap<String, String>,
    rd: std::fs::ReadDir,
) {
    for error in rd.flatten() {
        let directory = error.path();

        let Some(bytes_value) = branch(&directory.join("HEAD")) else {
            continue;
        };

        let Ok(gitdir) = std::fs::read_to_string(directory.join("gitdir")) else {
            continue;
        };

        let Some(wt) = Path::new(gitdir.trim()).parent() else {
            continue;
        };

        map.insert(bytes_value, wt.display().to_string());
    }
}
pub(crate) fn checkouts_git(repo: &Path) -> std::collections::HashMap<String, String> {
    let (_, out, _) = git(repo, &["worktree", "list", "--porcelain"]);
    let mut map = std::collections::HashMap::new();
    let mut path = None;

    for line in out.lines() {
        if let Some(path_value) = line.strip_prefix("worktree ") {
            path = Some(path_value.to_string());
        } else if let Some(bytes) = line.strip_prefix("branch ")
            && let Some(path_2) = &path
        {
            map.insert(bytes.trim_start_matches("refs/heads/").to_string(), path_2.clone());
        }
    }

    map
}
/// The path of REPO's codehead worktree on BRANCH.
pub(crate) fn worktree_path(repo: &Path, branch: &str) -> PathBuf {
    let name = repo
        .file_name()
        .map_or_else(|| "repo".into(), |count| count.to_string_lossy().into_owned());

    crate::engine::data_dir()
        .join("worktrees")
        .join(name)
        .join(branch.replace('/', "-"))
}
/// A new linked worktree of REPO on BRANCH (made from HEAD when it does not exist):
/// `<data>/worktrees/<repo>/<branch>`. Returns its path.
pub(crate) fn add_worktree(repo: &Path, branch: &str) -> Result<PathBuf, String> {
    let path = worktree_path(repo, branch);

    if path.exists() {
        return Err(format!("{} exists", path.display()));
    }

    let path_value = path.display().to_string();
    let exists = git(
        repo,
        &["rev-parse", "--verify", "--quiet", &format!("refs/heads/{branch}")],
    )
    .0;

    // a new branch starts from origin/<default> (fetched), else HEAD
    let base = if exists {
        String::new()
    } else {
        crate::infra::worktree_setup::new_branch_base(repo)
    };

    let args: Vec<&str> = if exists {
        vec!["worktree", "add", &path_value, branch]
    } else {
        vec!["worktree", "add", "--no-track", "-b", branch, &path_value, &base]
    };

    let (ok, _, err) = git(repo, &args);

    if ok {
        Ok(path)
    } else {
        Err(format!("git worktree add: {err}"))
    }
}
pub(crate) fn unpushed(branch: &str, path: &Path) -> usize {
    let result = if branch == "(detached)" { "HEAD" } else { branch };
    let (ok, out, _) = git(path, &["rev-list", "--count", result, "--not", "--remotes"]);

    if ok { out.trim().parse().unwrap_or(0) } else { 0 }
}
/// Linked worktree WT of repo MAIN out, in ms: the dir moves to the trash (1 rename), git forgets
/// it (its admin dir under `.git/worktrees/` goes), `rm -rf` deletes the files in its own process
/// (a 3 GB `node_modules`: ~30 s; it outlives the UI). Another file system (rename fails): `git
/// worktree remove --force`, the slow way. Uncommitted files: the caller's (stash first).
pub(crate) fn drop_worktree(main: &Path, wt: &Path) -> Result<(), String> {
    let admin = std::fs::read_to_string(wt.join(".git"))
        .ok()
        .and_then(|text| text.strip_prefix("gitdir:").map(|path| wt.join(path.trim())))
        .filter(|agent| {
            agent
                .parent()
                .and_then(Path::file_name)
                .is_some_and(|count| count == "worktrees")
                && agent.is_dir()
        });

    let trash = trash_dir();
    let _ = std::fs::create_dir_all(&trash);
    let name = wt
        .file_name()
        .map(|count_value| count_value.to_string_lossy().into_owned())
        .unwrap_or_default();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let to = trash.join(format!("{name}-{nanos}"));

    let Some(admin) = admin.filter(|_| std::fs::rename(wt, &to).is_ok()) else {
        let (ok, _, err) = git(main, &["worktree", "remove", "--force", &wt.display().to_string()]);

        return if ok {
            Ok(())
        } else {
            Err(format!("git worktree remove: {}", err.trim()))
        };
    };

    std::fs::remove_dir_all(&admin).map_err(|error| format!("{}: {error}", admin.display()))?;
    empty_trash();

    Ok(())
}
