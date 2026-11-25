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
    let mut plan = Plan {
        dir: repo.to_path_buf(),
        label: String::new(),
        prep: None,
        stash: false,
        worktree: None,
    };
    let repo_arg = repo.display().to_string();

    match target {
        Target::Checkout { branch, switch, stash } => {
            // git first: switch the main checkout (existing branch not checked out anywhere)
            match threads::checkouts(repo).get(branch) {
                Some(dir) => plan.dir = PathBuf::from(dir),
                None if *switch => {
                    plan.prep = Some(["git", "-C", &repo_arg, "switch", branch].map(String::from).to_vec());
                    plan.stash = *stash;
                }
                None => {}
            }

            plan.label.clone_from(branch);
        }
        Target::NewBranch(branch) => {
            plan.prep = Some(
                ["git", "-C", &repo_arg, "switch", "-c", branch]
                    .map(String::from)
                    .to_vec(),
            );
            plan.label.clone_from(branch);
        }
        Target::Branch(branch) => {
            plan.worktree = Some(branch.clone());
            plan.label.clone_from(branch);
        }
        Target::Worktree { path, branch } => {
            plan.dir = PathBuf::from(path);
            plan.label.clone_from(branch);
        }
        Target::Dir(dir) => {
            plan.dir = PathBuf::from(dir);
            plan.label = plan
                .dir
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
        }
        Target::InPlace => {
            plan.dir = cwd.to_path_buf();
            plan.label = cwd
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
        }
    }

    plan
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
    start: &Start,
    plan: &Plan,
    host: impl FnOnce(&str) -> Result<(), String>,
) -> Result<(Thread, Option<String>), String> {
    prepare_target(start, plan)?;

    engine::trace("start: git ready");
    let title = start.message.lines().next().unwrap_or_default().to_string();
    let mut thread = Thread::new(&title, start.message, &plan.dir, start.agents.to_vec());

    thread.owner_choices.title_set = start.title_set;

    if let Some(projects) = start.projects {
        thread.projects = projects.to_vec();
        thread.owner_choices.projects_set = true;
    }

    // a thread on main in an existing checkout: the host pulls main (ff only) before the 1st turn,
    // the view opens at once; a failed pull: the agents get a note on it
    thread.pull = plan.worktree.is_none()
        && matches!(start.target, Target::Checkout { .. } | Target::Worktree { .. })
        && (plan.label == "main" || plan.label == "master");

    if let Some(branch) = &plan.worktree {
        // the host makes it (fetch, `worktree add`, local files, port, setup) before the 1st turn
        let path = threads::worktree_path(start.repo, branch);

        if path.exists() {
            return Err(format!("{} exists", path.display()));
        }

        thread.worktree_pending = Some(branch.clone());
        thread.branch = Some(branch.clone());
    } else if !plan.label.is_empty() && start.repo.join(".git").exists() {
        thread.branch = Some(threads::current_branch(&plan.dir)).filter(|branch| !branch.is_empty());
    }

    // each agent's flow: its start step (the 1st, else the owner's pick) goes to the agent with
    // the task (the host's `prompt`)
    if !start.flows.is_empty() {
        set_flows(start, &mut thread);
    }

    thread.kickoff = true;
    thread.save()?;
    engine::trace("start: thread saved");

    // the host sends the task to every picked agent when it starts. 0 host (a failed spawn): the
    // thread opens with the task as its prompt draft, never lost
    let note = match host(&thread.id) {
        Ok(()) => None,
        Err(error) => {
            // the owner sends the draft: 0 2nd copy from a host started later
            thread.kickoff = false;
            let _ = engine::store::update(&thread.id, |saved| saved.kickoff = false);

            crate::domain::drafts::save_prompt_draft(&thread.id, start.message);
            Some(format!(
                "task not sent: {} · the task waits in the prompt · ctrl+enter: send",
                error.lines().next().unwrap_or_default()
            ))
        }
    };

    engine::trace("start: host started");

    Ok((thread, note))
}

fn set_flows(start: &Start<'_>, thread: &mut Thread) {
    let lib = engine::flows::Library::load(Some(&thread.work_dir()));

    for (agent, slug, step) in start.flows {
        if let Some(flow) = lib.flow(slug) {
            thread.set_flow(
                agent,
                Some(engine::flows::run::FlowState::new(flow, &lib, *step, engine::now_ms())),
            );
        }
    }
}

/// Branch name of a checkout target (stash message).
pub(crate) fn target_label(target: &Target) -> String {
    match target {
        Target::Checkout { branch, .. } => branch.clone(),
        _ => String::new(),
    }
}

fn prepare_target(start: &Start, plan: &Plan) -> Result<(), String> {
    let repo = start.repo.display().to_string();

    if let Some(argv) = &plan.prep {
        let run = |argv: &[String]| match Command::new(&argv[0]).args(&argv[1..]).stdin(Stdio::null()).output() {
            Ok(output) if output.status.success() => Ok(()),
            Ok(output) => Err(format!(
                "{}: {}",
                argv.join(" "),
                String::from_utf8_lossy(&output.stderr).trim()
            )),
            Err(error) => Err(format!("{}: {error}", argv.join(" "))),
        };
        let mut res = run(argv);

        if res.is_err() && plan.stash {
            let stash: Vec<String> = [
                "git",
                "-C",
                &repo,
                "stash",
                "push",
                "--include-untracked",
                "-m",
                &format!("codehead: before switch to {}", target_label(start.target)),
            ]
            .map(String::from)
            .to_vec();

            res = run(&stash).and_then(|()| run(argv));
        }

        res?;
    }

    Ok(())
}
