//! Thread start: the work between the new-thread form's submit and the thread view. Budget: 20 ms,
//! like every UI step (perf case `thread_start_submit_to_1st_frame`).

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::app::Target;
use crate::engine::{self, store::Thread};
use crate::threads;

/// The git work of a start, before the thread exists.
pub struct Plan {
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
pub fn plan(repo: &Path, cwd: &Path, target: &Target) -> Plan {
    let mut p = Plan { dir: repo.to_path_buf(), label: String::new(), prep: None, stash: false, worktree: None };
    let repo_arg = repo.display().to_string();
    match target {
        Target::Checkout { branch: b, switch, stash } => {
            // git first: switch the main checkout (existing branch not checked out anywhere)
            match threads::checkouts(repo).get(b) {
                Some(d) => p.dir = PathBuf::from(d),
                None if *switch => {
                    p.prep = Some(["git", "-C", &repo_arg, "switch", b].map(String::from).to_vec());
                    p.stash = *stash;
                }
                None => {}
            }
            p.label = b.clone();
        }
        Target::NewBranch(b) => {
            p.prep = Some(["git", "-C", &repo_arg, "switch", "-c", b].map(String::from).to_vec());
            p.label = b.clone();
        }
        Target::Branch(b) => {
            p.worktree = Some(b.clone());
            p.label = b.clone();
        }
        Target::Worktree { path, branch } => {
            p.dir = PathBuf::from(path);
            p.label = branch.clone();
        }
        Target::Dir(d) => {
            p.dir = PathBuf::from(d);
            p.label = p.dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        }
        Target::InPlace => {
            p.dir = cwd.to_path_buf();
            p.label = cwd.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        }
    }
    p
}

/// The form's outcome: what to start, where.
pub struct Start<'a> {
    pub repo: &'a Path,
    pub target: &'a Target,
    pub message: &'a str,
    pub agents: &'a [String],
    /// (agent, flow slug, start step) per agent with a flow.
    pub flows: &'a [(String, String, usize)],
}

/// Start thread S along PLAN: the branch switch (if any), thread.json, its host (HOST: starts the
/// host of a thread id, 0 wait). The host makes the worktree, pulls main, runs the setup and
/// sends the task (`kickoff`). Err: the switch or the save failed, 0 thread. Note: 0 host; the
/// task waits as the thread's prompt draft.
pub fn launch(s: &Start, plan: &Plan, host: impl FnOnce(&str) -> Result<(), String>) -> Result<(Thread, Option<String>), String> {
    let repo = s.repo.display().to_string();
    if let Some(g) = &plan.prep {
        let run = |g: &[String]| match Command::new(&g[0]).args(&g[1..]).stdin(Stdio::null()).output() {
            Ok(o) if o.status.success() => Ok(()),
            Ok(o) => Err(format!("{}: {}", g.join(" "), String::from_utf8_lossy(&o.stderr).trim())),
            Err(e) => Err(format!("{}: {e}", g.join(" "))),
        };
        let mut res = run(g);
        if res.is_err() && plan.stash {
            let st: Vec<String> = ["git", "-C", &repo, "stash", "push", "--include-untracked", "-m", &format!("codehead: before switch to {}", target_label(s.target))].map(String::from).to_vec();
            res = run(&st).and_then(|_| run(g));
        }
        res?;
    }
    engine::trace("start: git ready");
    let title = s.message.lines().next().unwrap_or_default().to_string();
    let mut t = Thread::new(&title, s.message, &plan.dir, s.agents.to_vec());
    // a thread on main in an existing checkout: the host pulls main (ff only) before the 1st turn,
    // the view opens at once; a failed pull: the agents get a note on it
    t.pull = plan.worktree.is_none() && matches!(s.target, Target::Checkout { .. } | Target::Worktree { .. }) && (plan.label == "main" || plan.label == "master");
    if let Some(b) = &plan.worktree {
        // the host makes it (fetch, `worktree add`, local files, port, setup) before the 1st turn
        let p = threads::worktree_path(s.repo, b);
        if p.exists() {
            return Err(format!("{} exists", p.display()));
        }
        t.worktree_pending = Some(b.clone());
        t.branch = Some(b.clone());
    } else if !plan.label.is_empty() && s.repo.join(".git").exists() {
        t.branch = Some(threads::current_branch(&plan.dir)).filter(|b| !b.is_empty());
    }
    // each agent's flow: its start step (the 1st, else the owner's pick) goes to the agent with
    // the task (the host's `prompt`)
    if !s.flows.is_empty() {
        let lib = engine::flows::Library::load(Some(&t.work_dir()));
        for (agent, slug, start) in s.flows {
            if let Some(fl) = lib.flow(slug) {
                t.set_flow(agent, Some(engine::flows::run::FlowState::new(fl, &lib, *start, engine::now_ms())));
            }
        }
    }
    t.kickoff = true;
    t.save()?;
    engine::trace("start: thread saved");
    // the host sends the task to every picked agent when it starts. 0 host (a failed spawn): the
    // thread opens with the task as its prompt draft, never lost
    let note = match host(&t.id) {
        Ok(()) => None,
        Err(e) => {
            // the owner sends the draft: 0 2nd copy from a host started later
            t.kickoff = false;
            let _ = engine::store::update(&t.id, |t| t.kickoff = false);
            crate::view::save_prompt_draft(&t.id, s.message);
            Some(format!("task not sent: {} · the task waits in the prompt · ctrl+enter: send", e.lines().next().unwrap_or_default()))
        }
    };
    engine::trace("start: host started");
    Ok((t, note))
}

/// Branch name of a checkout target (stash message).
pub fn target_label(t: &Target) -> String {
    match t {
        Target::Checkout { branch, .. } => branch.clone(),
        _ => String::new(),
    }
}
