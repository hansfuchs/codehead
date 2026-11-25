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
    let dirs: Vec<PathBuf> = rd.flatten().map(|error| error.path()).collect();

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

    if let Ok(mut character) = child {
        std::thread::spawn(move || character.wait());
    }
}
/// The delete row of thread ID, from its record: its linked worktree (on disk), else its dir (in
/// place: delete keeps the dir; an archived thread: its branch stays).
pub(crate) fn row_of(id: &str) -> Option<Worktree> {
    let thread_value = store::load(id)?;
    let wt = thread_value
        .worktree
        .clone()
        .filter(|path_value| Path::new(path_value).is_dir());
    let lite = Thread {
        id: thread_value.id.clone(),
        title: thread_value.title.clone(),
        state: thread_value.state.clone(),
        dir: Some(thread_value.dir.clone()),
        worktree: thread_value.worktree.clone(),
        branch: thread_value.branch.clone(),
        repo: thread_value.repo.clone(),
        ..Default::default()
    };
    let repo = repo_of_thread(&lite);

    let branch = match &wt {
        Some(path_2) => thread_value
            .branch
            .clone()
            .filter(|bytes| !bytes.is_empty())
            .unwrap_or_else(|| {
                Some(current_branch(Path::new(path_2)))
                    .filter(|bytes_value| !bytes_value.is_empty())
                    .unwrap_or_else(|| "(detached)".into())
            }),
        None => "in place".into(),
    };

    Some(Worktree {
        path: wt.clone().unwrap_or_else(|| thread_value.dir.clone()),
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

    let wt = thread
        .worktree
        .clone()
        .filter(|path_value| Path::new(path_value).is_dir());

    // checks first: a refused archive leaves the thread as it was
    let branch = match &wt {
        Some(path_2) => match archive_some_path_2(&thread, path_2) {
            Ok(value) => value,
            Err(value) => return value,
        },
        None => None,
    };

    // archived at once: every list read from now on shows it there (a failure below: back)
    store::update(&thread.id, |text| text.state = "archived".into())?;
    let mut done = Vec::new();
    let repo =
        repo_root(Path::new(&thread.dir)).or_else(|| wt.as_deref().and_then(|path_3| repo_of(Path::new(path_3))));
    let out = (|| {
        if client::alive(&thread.id) {
            let _ = client::send_within(
                &thread.id,
                &serde_json::json!({"op": "stop"}),
                std::time::Duration::from_secs(3),
            );
        }

        let (Some(path_4), Some(bytes_2)) = (&wt, &branch) else {
            return Ok(());
        };

        let path = Path::new(path_4);

        if let Some((label, ok, _)) = crate::infra::worktree_setup::run_teardown(&thread) {
            done.push(format!("{label}: {}", if ok { "ran" } else { "failed" }));
        }

        let (_, dirty, _) = git(path, &["status", "--porcelain"]);
        let count_value = dirty.lines().count();

        if let Some(value) = reject_dirty_worktree(&mut done, bytes_2, path, count_value) {
            return value;
        }

        let main = repo_of(path)
            .or_else(|| repo.clone())
            .ok_or_else(|| format!("{path_4}: 0 git repo"))?;

        drop_worktree(&main, path)?;
        done.push(format!("worktree removed · branch {bytes_2} kept"));
        Ok(())
    })();

    save_archived_thread(&thread, branch.as_ref(), done, repo, out)
}

fn reject_dirty_worktree(
    done: &mut Vec<String>,
    bytes_2: &String,
    path: &Path,
    count_value: usize,
) -> Option<Result<(), String>> {
    if count_value > 0 {
        let msg = format!("codehead: {bytes_2} before archive");
        let (ok, _, err) = git(path, &["stash", "push", "--include-untracked", "-m", &msg]);

        if let Some(value) = check_worktree_removal(ok, &err) {
            return Some(value);
        }

        done.push(format!("{count_value} uncommitted files → git stash \"{msg}\""));
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
        let _ = store::update(&thread.id, |text_value| text_value.state.clone_from(&thread.state));

        return Err(error);
    }

    let repo_s = repo.map(|result| result.display().to_string());

    store::update(&thread.id, |text_2| {
        text_2.state = "archived".into();

        if let Some(bytes_3) = &branch {
            text_2.worktree = None;
            text_2.worktree_pending = Some((*bytes_3).clone());
            text_2.port = None;
            text_2.setup = None;
        }

        text_2.resume.clear();

        if text_2.repo.is_none() {
            text_2.repo = repo_s;
        }
    })?;

    done.insert(0, "archived".into());

    Ok(done.join(" · "))
}

fn archive_some_path_2(thread: &store::Thread, path_2: &String) -> Result<Option<String>, Result<String, String>> {
    let bytes_value = thread
        .branch
        .clone()
        .filter(|bytes| !bytes.is_empty())
        .unwrap_or_else(|| current_branch(Path::new(path_2)));

    if bytes_value.is_empty() || bytes_value == "(detached)" {
        return Err(Err(
            "detached HEAD: 0 branch keeps its commits. check out a branch first".into(),
        ));
    }

    Ok(Some(bytes_value))
}

fn check_worktree_removal(ok: bool, err: &str) -> Option<Result<(), String>> {
    if !ok {
        return Some(Err(format!("stash failed: {}", err.trim())));
    }

    None
}

/// Thread ID back from the archive: open; its host starts and makes the worktree again (its
/// branch), the 1st turn waits for it.
pub(crate) fn unarchive(id: &str) -> Result<String, String> {
    let thread_value = store::update(id, |thread| {
        if thread.state == "archived" {
            thread.state = "open".into();
        }
    })?;

    match &thread_value.worktree_pending {
        Some(bytes) => {
            client::ensure(&thread_value.id)?;
            Ok(format!("unarchived · worktree on {bytes}: the host makes it"))
        }
        None => Ok("unarchived".into()),
    }
}
/// Save edit E of thread ID. A live host: agents that leave / join go through it (their
/// sessions stop / start); else thread.json only. Ok: what changed.
pub(crate) fn edit(id: &str, error: &Edit) -> Result<String, String> {
    let thread = store::load(id).ok_or_else(|| format!("no thread {id}"))?;
    let changes = edit_changes(&thread, error);

    apply_edit(&thread, error, changes)
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

fn edit_changes(thread: &store::Thread, error: &Edit) -> EditChanges {
    let mut changed: Vec<&'static str> = Vec::new();
    let title = error.title.trim().to_string();
    let title_new = !title.is_empty() && title != thread.title.lines().next().unwrap_or_default();

    if title_new {
        changed.push("title");
    }

    let projects_new = match &error.projects {
        Some(path) => !thread.owner_choices.projects_set || *path != thread.projects,
        None => thread.owner_choices.projects_set,
    };

    if projects_new {
        changed.push("projects");
    }

    let repo = edited_repo(thread, error);

    if repo != thread.repo {
        changed.push("repo");
    }

    let keep: Vec<store::Checkout> = thread
        .checkouts
        .iter()
        .filter(|character| error.checkouts.contains(&character.dir))
        .cloned()
        .collect();

    if keep.len() != thread.checkouts.len() {
        changed.push("linked repos");
    }

    let gone: Vec<String> = thread
        .agents
        .iter()
        .filter(|agent| !error.agents.contains(agent))
        .cloned()
        .collect();
    let new: Vec<String> = error
        .agents
        .iter()
        .filter(|agent_value| !thread.agents.contains(agent_value))
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

fn edited_repo(thread: &store::Thread, error: &Edit) -> Option<String> {
    let derived = repo_root(Path::new(&thread.dir))
        .or_else(|| {
            thread
                .worktree
                .as_deref()
                .and_then(|path_value| repo_root(Path::new(path_value)))
        })
        .map(|path_2| path_2.display().to_string());

    // the pick of the repo of its dirs: 0 override
    error
        .repo
        .clone()
        .filter(|result| !result.is_empty() && Some(result) != derived.as_ref())
}

fn apply_edit(thread: &store::Thread, error: &Edit, changes: EditChanges) -> Result<String, String> {
    let EditChanges {
        mut changed,
        title,
        projects,
        repo,
        checkouts,
        agents,
    } = changes;

    if error.agents.is_empty() {
        return Err("1 agent at least".into());
    }

    let live = client::alive(&thread.id);

    if live {
        for agent_2 in &agents.gone {
            client::call(&thread.id, &serde_json::json!({"op": "leave", "agent": agent_2}))?;
        }

        for agent_3 in &agents.new {
            client::call(&thread.id, &serde_json::json!({"op": "join", "agent": agent_3}))?;
        }
    }

    if !agents.gone.is_empty() || !agents.new.is_empty() || error.agents != thread.agents {
        changed.push("agents");
    }

    store::update(&thread.id, |text| {
        if let Some(title) = &title {
            text.title.clone_from(title);
            text.owner_choices.title_set = true;
        }

        if projects {
            text.owner_choices.projects_set = error.projects.is_some();

            set_project_choices(error, text);
        }

        text.repo.clone_from(&repo);
        text.checkouts.clone_from(&checkouts);

        // the order of E (lead first); a join of a live host added its agent already
        text.agents.clone_from(&error.agents);
    })?;

    Ok(if changed.is_empty() {
        "0 changes".into()
    } else {
        format!("saved: {}", changed.join(", "))
    })
}

fn set_project_choices(error: &Edit, text: &mut store::Thread) {
    if let Some(path_3) = &error.projects {
        text.projects.clone_from(path_3);
    }
}
