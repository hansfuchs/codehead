//! Thread start: the work between the new-thread form's submit and the thread view. Budget: 20 ms,
//! like every UI step (perf case `thread_start_submit_to_1st_frame`).

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::domain::threads;
use crate::engine::{self, store::Thread};
use crate::screens::start_screen::Target;

/// The git work of a start, before the thread exists.
pub(crate) struct Plan {
    /// The thread's dir.
    pub dir: PathBuf,
    /// The branch label (status line; main / master: the host pulls).
    pub label: String,
    /// `git switch` argv for the main checkout.
    pub prep: Option<Vec<String>>,
    /// The switch fails on conflicting local changes: `git stash`, then the switch again.
    pub stash: bool,
    /// A new linked worktree for this branch.
    pub worktree: Option<String>,
}

/// The plan of TARGET in REPO (CWD: the dir of an in-place start).
pub(crate) fn plan(repo: &Path, cwd: &Path, target: &Target) -> Plan {
    let mut path_value = Plan {
        dir: repo.to_path_buf(),
        label: String::new(),
        prep: None,
        stash: false,
        worktree: None,
    };
    let repo_arg = repo.display().to_string();

    match target {
        Target::Checkout {
            branch: bytes,
            switch,
            stash,
        } => {
            // git first: switch the main checkout (existing branch not checked out anywhere)
            match threads::checkouts(repo).get(bytes) {
                Some(directory) => path_value.dir = PathBuf::from(directory),
                None if *switch => {
                    path_value.prep = Some(["git", "-C", &repo_arg, "switch", bytes].map(String::from).to_vec());
                    path_value.stash = *stash;
                }
                None => {}
            }

            path_value.label.clone_from(bytes);
        }
        Target::NewBranch(bytes_value) => {
            path_value.prep = Some(
                ["git", "-C", &repo_arg, "switch", "-c", bytes_value]
                    .map(String::from)
                    .to_vec(),
            );
            path_value.label.clone_from(bytes_value);
        }
        Target::Branch(bytes_2) => {
            path_value.worktree = Some(bytes_2.clone());
            path_value.label.clone_from(bytes_2);
        }
        Target::Worktree { path, branch } => {
            path_value.dir = PathBuf::from(path);
            path_value.label.clone_from(branch);
        }
        Target::Dir(directory_value) => {
            path_value.dir = PathBuf::from(directory_value);
            path_value.label = path_value
                .dir
                .file_name()
                .map(|count| count.to_string_lossy().into_owned())
                .unwrap_or_default();
        }
        Target::InPlace => {
            path_value.dir = cwd.to_path_buf();
            path_value.label = cwd
                .file_name()
                .map(|count_value| count_value.to_string_lossy().into_owned())
                .unwrap_or_default();
        }
    }

    path_value
}

/// The form's outcome: what to start, where.
pub(crate) struct Start<'a> {
    pub repo: &'a Path,
    pub target: &'a Target,
    pub message: &'a str,
    pub agents: &'a [String],
    /// (agent, flow slug, start step) per agent with a flow.
    pub flows: &'a [(String, String, usize)],
    /// The owner's projects (None: auto, the agents pick).
    pub projects: Option<&'a [String]>,
    /// The owner wrote the title: the agents' `set_title` leaves it.
    pub title_set: bool,
}

/// Start thread S along PLAN: the branch switch (if any), thread.json, its host (HOST: starts the
/// host of a thread id, 0 wait). The host makes the worktree, pulls main, runs the setup and
/// sends the task (`kickoff`). Err: the switch or the save failed, 0 thread. Note: 0 host; the
/// task waits as the thread's prompt draft.
pub(crate) fn launch(
    text: &Start,
    plan: &Plan,
    host: impl FnOnce(&str) -> Result<(), String>,
) -> Result<(Thread, Option<String>), String> {
    prepare_target(text, plan)?;

    engine::trace("start: git ready");
    let title = text.message.lines().next().unwrap_or_default().to_string();
    let mut thread = Thread::new(&title, text.message, &plan.dir, text.agents.to_vec());

    thread.owner_choices.title_set = text.title_set;

    if let Some(path) = text.projects {
        thread.projects = path.to_vec();
        thread.owner_choices.projects_set = true;
    }

    // a thread on main in an existing checkout: the host pulls main (ff only) before the 1st turn,
    // the view opens at once; a failed pull: the agents get a note on it
    thread.pull = plan.worktree.is_none()
        && matches!(text.target, Target::Checkout { .. } | Target::Worktree { .. })
        && (plan.label == "main" || plan.label == "master");

    if let Some(bytes) = &plan.worktree {
        // the host makes it (fetch, `worktree add`, local files, port, setup) before the 1st turn
        let path_value = threads::worktree_path(text.repo, bytes);

        if path_value.exists() {
            return Err(format!("{} exists", path_value.display()));
        }

        thread.worktree_pending = Some(bytes.clone());
        thread.branch = Some(bytes.clone());
    } else if !plan.label.is_empty() && text.repo.join(".git").exists() {
        thread.branch = Some(threads::current_branch(&plan.dir)).filter(|bytes_value| !bytes_value.is_empty());
    }

    // each agent's flow: its start step (the 1st, else the owner's pick) goes to the agent with
    // the task (the host's `prompt`)
    if !text.flows.is_empty() {
        launch_lib(text, &mut thread);
    }

    thread.kickoff = true;
    thread.save()?;
    engine::trace("start: thread saved");

    // the host sends the task to every picked agent when it starts. 0 host (a failed spawn): the
    // thread opens with the task as its prompt draft, never lost
    let note = match host(&thread.id) {
        Ok(()) => None,
        Err(error_value) => {
            // the owner sends the draft: 0 2nd copy from a host started later
            thread.kickoff = false;
            let _ = engine::store::update(&thread.id, |thread_value| thread_value.kickoff = false);

            crate::domain::drafts::save_prompt_draft(&thread.id, text.message);
            Some(format!(
                "task not sent: {} · the task waits in the prompt · ctrl+enter: send",
                error_value.lines().next().unwrap_or_default()
            ))
        }
    };

    engine::trace("start: host started");

    Ok((thread, note))
}

fn launch_lib(text: &Start<'_>, thread: &mut Thread) {
    let lib = engine::flows::Library::load(Some(&thread.work_dir()));

    for (agent, slug, start) in text.flows {
        if let Some(fl) = lib.flow(slug) {
            thread.set_flow(
                agent,
                Some(engine::flows::run::FlowState::new(fl, &lib, *start, engine::now_ms())),
            );
        }
    }
}

/// Branch name of a checkout target (stash message).
pub(crate) fn target_label(thread: &Target) -> String {
    match thread {
        Target::Checkout { branch, .. } => branch.clone(),
        _ => String::new(),
    }
}

fn prepare_target(text: &Start, plan: &Plan) -> Result<(), String> {
    let repo = text.repo.display().to_string();

    if let Some(group) = &plan.prep {
        let run = |group_value: &[String]| match Command::new(&group_value[0])
            .args(&group_value[1..])
            .stdin(Stdio::null())
            .output()
        {
            Ok(output) if output.status.success() => Ok(()),
            Ok(output_value) => Err(format!(
                "{}: {}",
                group_value.join(" "),
                String::from_utf8_lossy(&output_value.stderr).trim()
            )),
            Err(error) => Err(format!("{}: {error}", group_value.join(" "))),
        };
        let mut res = run(group);

        if res.is_err() && plan.stash {
            let st: Vec<String> = [
                "git",
                "-C",
                &repo,
                "stash",
                "push",
                "--include-untracked",
                "-m",
                &format!("codehead: before switch to {}", target_label(text.target)),
            ]
            .map(String::from)
            .to_vec();

            res = run(&st).and_then(|()| run(group));
        }

        res?;
    }

    Ok(())
}
