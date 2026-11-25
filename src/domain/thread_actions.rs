use super::{
    Command, Edit, Path, PathBuf, Stdio, Thread, Worktree, client, current_branch, drop_worktree, git, repo_of,
    repo_of_thread, repo_root, store,
};

/// Stop thread ID's host, delete its records.
pub(crate) fn delete_thread(id: &str) -> bool {
    if client::alive(id) {
        let _ = client::send(id, &serde_json::json!({"op": "stop"}));
    }

    store::delete(id).is_ok()
}
/// Dir of worktree dirs on their way out (`drop_worktree`): their files go in the background.
pub(crate) fn trash_dir() -> PathBuf {
    crate::engine::data_dir().join("trash")
}
/// Delete the trash's dirs (left over ones too) in 1 `rm -rf` of its own process group: a
/// closed terminal or a quit UI never stops it.
pub(crate) fn empty_trash() {
    use std::os::unix::process::CommandExt;
    let Ok(rd) = std::fs::read_dir(trash_dir()) else { return };
    let dirs: Vec<PathBuf> = rd.flatten().map(|entry| entry.path()).collect();

    if dirs.is_empty() {
        return;
    }

    let child = Command::new("rm")
        .arg("-rf")
        .args(&dirs)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn();

    if let Ok(mut rm) = child {
        std::thread::spawn(move || rm.wait());
    }
}
/// The delete row of thread ID, from its record: its linked worktree (on disk), else its dir (in
/// place: delete keeps the dir; an archived thread: its branch stays).
pub(crate) fn row_of(id: &str) -> Option<Worktree> {
    let thread = store::load(id)?;
    let wt = thread.worktree.clone().filter(|dir| Path::new(dir).is_dir());
    let lite = Thread {
        id: thread.id.clone(),
        title: thread.title.clone(),
        state: thread.state.clone(),
        dir: Some(thread.dir.clone()),
        worktree: thread.worktree.clone(),
        branch: thread.branch.clone(),
        repo: thread.repo.clone(),
        ..Default::default()
    };
    let repo = repo_of_thread(&lite);

    let branch = match &wt {
        Some(dir) => thread
            .branch
            .clone()
            .filter(|branch| !branch.is_empty())
            .unwrap_or_else(|| {
                Some(current_branch(Path::new(dir)))
                    .filter(|branch| !branch.is_empty())
                    .unwrap_or_else(|| "(detached)".into())
            }),
        None => "in place".into(),
    };

    Some(Worktree {
        path: wt.clone().unwrap_or_else(|| thread.dir.clone()),
        branch,
        linked: wt.is_some(),
        thread: Some(lite),
        unpushed: 0,
        when: String::new(),
        repo,
    })
}
/// Archive thread ID: its host stops; a linked worktree goes (the teardown script first,
/// uncommitted files → git stash), its branch stays in `worktree_pending`: the next host start (an
/// unarchive, an open) makes the worktree again. In place: the thread only. Ok: what happened.
pub(crate) fn archive(id: &str) -> Result<String, String> {
    let thread = store::load(id).ok_or_else(|| format!("no thread {id}"))?;

    if thread.state == "archived" {
        return Ok("archived already".into());
    }

    if !thread.kids.is_empty() {
        return Err(format!("{} sub-threads run: stop them first", thread.kids.len()));
    }

    let wt = thread.worktree.clone().filter(|dir| Path::new(dir).is_dir());

    // checks first: a refused archive leaves the thread as it was
    let branch = match &wt {
        Some(dir) => match archive_branch(&thread, dir) {
            Ok(branch) => branch,
            Err(refusal) => return refusal,
        },
        None => None,
    };

    // archived at once: every list read from now on shows it there (a failure below: back)
    store::update(&thread.id, |record| record.state = "archived".into())?;
    let mut done = Vec::new();
    let repo = repo_root(Path::new(&thread.dir)).or_else(|| wt.as_deref().and_then(|dir| repo_of(Path::new(dir))));
    let out = (|| {
        if client::alive(&thread.id) {
            let _ = client::send_within(
                &thread.id,
                &serde_json::json!({"op": "stop"}),
                std::time::Duration::from_secs(3),
            );
        }

        let (Some(dir), Some(branch)) = (&wt, &branch) else {
            return Ok(());
        };

        let path = Path::new(dir);

        if let Some((label, ok, _)) = crate::infra::worktree_setup::run_teardown(&thread) {
            done.push(format!("{label}: {}", if ok { "ran" } else { "failed" }));
        }

        let (_, dirty, _) = git(path, &["status", "--porcelain"]);
        let dirty_files = dirty.lines().count();

        if let Some(failed) = stash_dirty_worktree(&mut done, branch, path, dirty_files) {
            return failed;
        }

        let main = repo_of(path)
            .or_else(|| repo.clone())
            .ok_or_else(|| format!("{dir}: 0 git repo"))?;

        drop_worktree(&main, path)?;
        done.push(format!("worktree removed · branch {branch} kept"));
        Ok(())
    })();

    save_archived_thread(&thread, branch.as_ref(), done, repo, out)
}

fn stash_dirty_worktree(
    done: &mut Vec<String>,
    branch: &String,
    path: &Path,
    dirty_files: usize,
) -> Option<Result<(), String>> {
    if dirty_files > 0 {
        let msg = format!("codehead: {branch} before archive");
        let (ok, _, err) = git(path, &["stash", "push", "--include-untracked", "-m", &msg]);

        if let Some(failed) = stash_failure(ok, &err) {
            return Some(failed);
        }

        done.push(format!("{dirty_files} uncommitted files → git stash \"{msg}\""));
    }

    None
}

fn save_archived_thread(
    thread: &store::Thread,
    branch: Option<&String>,
    mut done: Vec<String>,
    repo: Option<PathBuf>,
    out: Result<(), String>,
) -> Result<String, String> {
    if let Err(error) = out {
        let _ = store::update(&thread.id, |record| record.state.clone_from(&thread.state));

        return Err(error);
    }

    let repo_s = repo.map(|repo| repo.display().to_string());

    store::update(&thread.id, |record| {
        record.state = "archived".into();

        if let Some(branch) = &branch {
            record.worktree = None;
            record.worktree_pending = Some((*branch).clone());
            record.port = None;
            record.setup = None;
        }

        record.resume.clear();

        if record.repo.is_none() {
            record.repo = repo_s;
        }
    })?;

    done.insert(0, "archived".into());

    Ok(done.join(" · "))
}

fn archive_branch(thread: &store::Thread, dir: &String) -> Result<Option<String>, Result<String, String>> {
    let branch = thread
        .branch
        .clone()
        .filter(|branch| !branch.is_empty())
        .unwrap_or_else(|| current_branch(Path::new(dir)));

    if branch.is_empty() || branch == "(detached)" {
        return Err(Err(
            "detached HEAD: 0 branch keeps its commits. check out a branch first".into(),
        ));
    }

    Ok(Some(branch))
}

fn stash_failure(ok: bool, err: &str) -> Option<Result<(), String>> {
    if !ok {
        return Some(Err(format!("stash failed: {}", err.trim())));
    }

    None
}

/// Thread ID back from the archive: open; its host starts and makes the worktree again (its
/// branch), the 1st turn waits for it.
pub(crate) fn unarchive(id: &str) -> Result<String, String> {
    let thread = store::update(id, |record| {
        if record.state == "archived" {
            record.state = "open".into();
        }
    })?;

    match &thread.worktree_pending {
        Some(branch) => {
            client::ensure(&thread.id)?;
            Ok(format!("unarchived · worktree on {branch}: the host makes it"))
        }
        None => Ok("unarchived".into()),
    }
}
/// Save edit E of thread ID. A live host: agents that leave / join go through it (their
/// sessions stop / start); else thread.json only. Ok: what changed.
pub(crate) fn edit(id: &str, edit: &Edit) -> Result<String, String> {
    let thread = store::load(id).ok_or_else(|| format!("no thread {id}"))?;
    let changes = edit_changes(&thread, edit);

    apply_edit(&thread, edit, changes)
}

struct AgentChanges {
    gone: Vec<String>,
    new: Vec<String>,
}

struct EditChanges {
    changed: Vec<&'static str>,
    title: Option<String>,
    projects: bool,
    repo: Option<String>,
    checkouts: Vec<store::Checkout>,
    agents: AgentChanges,
}

fn edit_changes(thread: &store::Thread, edit: &Edit) -> EditChanges {
    let mut changed: Vec<&'static str> = Vec::new();
    let title = edit.title.trim().to_string();
    let title_new = !title.is_empty() && title != thread.title.lines().next().unwrap_or_default();

    if title_new {
        changed.push("title");
    }

    let projects_new = match &edit.projects {
        Some(projects) => !thread.owner_choices.projects_set || *projects != thread.projects,
        None => thread.owner_choices.projects_set,
    };

    if projects_new {
        changed.push("projects");
    }

    let repo = edited_repo(thread, edit);

    if repo != thread.repo {
        changed.push("repo");
    }

    let keep: Vec<store::Checkout> = thread
        .checkouts
        .iter()
        .filter(|checkout| edit.checkouts.contains(&checkout.dir))
        .cloned()
        .collect();

    if keep.len() != thread.checkouts.len() {
        changed.push("linked repos");
    }

    let gone: Vec<String> = thread
        .agents
        .iter()
        .filter(|agent| !edit.agents.contains(agent))
        .cloned()
        .collect();
    let new: Vec<String> = edit
        .agents
        .iter()
        .filter(|agent| !thread.agents.contains(agent))
        .cloned()
        .collect();

    EditChanges {
        changed,
        title: title_new.then_some(title),
        projects: projects_new,
        repo,
        checkouts: keep,
        agents: AgentChanges { gone, new },
    }
}

fn edited_repo(thread: &store::Thread, edit: &Edit) -> Option<String> {
    let derived = repo_root(Path::new(&thread.dir))
        .or_else(|| {
            thread
                .worktree
                .as_deref()
                .and_then(|worktree| repo_root(Path::new(worktree)))
        })
        .map(|root| root.display().to_string());

    // the pick of the repo of its dirs: 0 override
    edit.repo
        .clone()
        .filter(|repo| !repo.is_empty() && Some(repo) != derived.as_ref())
}

fn apply_edit(thread: &store::Thread, edit: &Edit, changes: EditChanges) -> Result<String, String> {
    let EditChanges {
        mut changed,
        title,
        projects,
        repo,
        checkouts,
        agents,
    } = changes;

    if edit.agents.is_empty() {
        return Err("1 agent at least".into());
    }

    let live = client::alive(&thread.id);

    if live {
        for agent in &agents.gone {
            client::call(&thread.id, &serde_json::json!({"op": "leave", "agent": agent}))?;
        }

        for agent in &agents.new {
            client::call(&thread.id, &serde_json::json!({"op": "join", "agent": agent}))?;
        }
    }

    if !agents.gone.is_empty() || !agents.new.is_empty() || edit.agents != thread.agents {
        changed.push("agents");
    }

    store::update(&thread.id, |record| {
        if let Some(title) = &title {
            record.title.clone_from(title);
            record.owner_choices.title_set = true;
        }

        if projects {
            record.owner_choices.projects_set = edit.projects.is_some();

            set_project_choices(edit, record);
        }

        record.repo.clone_from(&repo);
        record.checkouts.clone_from(&checkouts);

        // the order of E (lead first); a join of a live host added its agent already
        record.agents.clone_from(&edit.agents);
    })?;

    Ok(if changed.is_empty() {
        "0 changes".into()
    } else {
        format!("saved: {}", changed.join(", "))
    })
}

fn set_project_choices(edit: &Edit, record: &mut store::Thread) {
    if let Some(projects) = &edit.projects {
        record.projects.clone_from(projects);
    }
}
