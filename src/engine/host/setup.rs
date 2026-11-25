use super::{Host, Input, Kind, Path, PathBuf, Sender, json, pin_branch, store};

impl Host {
    /// Thread T's setup is pending: it runs on a thread (a `setup` tool run in the log); the
    /// messages wait until it ends.
    pub(crate) fn start_setup(&mut self, thread: &store::Thread, tx: Sender<Input>) {
        let Some(wt) = thread.worktree.as_deref() else {
            return;
        };

        let Some((_, label, _)) = crate::infra::worktree_setup::script(Path::new(wt), "setup") else {
            let _ = store::update(&self.id, |stored| stored.setup = None);

            return;
        };

        self.setup_running = true;
        self.log(
            Some("setup"),
            Kind::Tool {
                id: "setup".into(),
                name: "Bash".into(),
                input: json!({"command": label, "description": "worktree setup: the 1st turn waits for it"}),
            },
        );
        let thread = thread.clone();

        std::thread::spawn(move || {
            let (label, ok, out) = crate::infra::worktree_setup::run_setup(&thread)
                .unwrap_or_else(|| ("setup".into(), true, String::new()));
            let _ = tx.send(Input::Setup(label, ok, out));
        });
    }
    /// The setup ended: its result in the log, the waiting messages go out (a failed setup: with
    /// a note on it).
    pub(crate) fn setup_done(&mut self, label: &str, ok: bool, out: String) {
        self.setup_running = false;
        self.log(
            Some("setup"),
            Kind::ToolResult {
                id: "setup".into(),
                ok,
                output: out,
            },
        );
        let status = if ok { "done" } else { "failed" };
        let _ = store::update(&self.id, |thread| thread.setup = Some(status.into()));
        let note = (!ok).then(|| format!("[codehead: the worktree setup (`{label}`) failed; its output is in the thread. Make the environment work first.]"));

        self.release_queue(note.as_deref());
    }
    /// A new thread (`kickoff`): its task to its agents, as the owner's `prompt` (the UI started
    /// the host and opened the view at once, 0 wait for a reply).
    pub(crate) fn kickoff(&mut self, thread: &store::Thread) {
        if !thread.kickoff {
            return;
        }

        // off first: a host that dies after this line never sends the task twice
        let _ = store::update(&self.id, |stored| stored.kickoff = false);

        if let Err(error) = self.cmd(&json!({"op": "prompt", "to": thread.agents, "text": thread.task})) {
            self.log(
                None,
                Kind::Error {
                    message: format!("task not sent: {error}"),
                },
            );
        }

        self.deliver_later();
    }
    /// A new thread's worktree (`worktree_pending`): `git fetch` + `git worktree add`, the local
    /// files, a port, off the host loop; messages wait for its end.
    pub(crate) fn start_worktree(&mut self, thread: &store::Thread, tx: Sender<Input>) {
        let Some(branch) = thread.worktree_pending.clone() else {
            return;
        };

        self.setup_running = true;
        self.log(Some("setup"), Kind::Tool { id: "worktree".into(), name: "Bash".into(), input: json!({"command": format!("git worktree add {branch}"), "description": "the thread's worktree: the 1st turn waits for it"}) });
        let repo = PathBuf::from(&thread.dir);

        std::thread::spawn(move || {
            let result = prepare_worktree(&repo, &branch);

            let _ = tx.send(Input::Worktree(result));
        });
    }
    /// The worktree is there (or failed): in thread.json, then its setup script, else the waiting
    /// messages go out (a failed worktree: with a note, the agents are in the main checkout).
    pub(crate) fn worktree_done(&mut self, created: Result<(PathBuf, u16), String>, tx: Sender<Input>) {
        let branch = self
            .thread()
            .ok()
            .and_then(|thread| thread.worktree_pending)
            .unwrap_or_default();

        match created {
            Ok((path, port)) => {
                finish_worktree_setup(tx, &branch, &path, port, self);
            }
            Err(error) => {
                self.log(
                    Some("setup"),
                    Kind::ToolResult {
                        id: "worktree".into(),
                        ok: false,
                        output: error.clone(),
                    },
                );
                let dir = self.thread().map(|thread| thread.dir).unwrap_or_default();
                let _ = store::update(&self.id, |stored| stored.worktree_pending = None);

                self.setup_running = false;
                let note = format!(
                    "[codehead: the worktree for branch `{branch}` failed ({error}): you are in the main checkout {dir}. Ask the owner before you edit files.]"
                );

                self.release_queue(Some(&note));
            }
        }
    }

    /// The agents of the waiting messages: their sessions start now (a harness boots for seconds),
    /// the messages still wait. 0 boot while the worktree is not there (the session's dir).
    pub(crate) fn boot_queued(&mut self) {
        if self.thread().is_ok_and(|thread| thread.worktree_pending.is_some()) {
            return;
        }

        let mut agents: Vec<String> = self.queue.iter().map(|(agent, _)| agent.clone()).collect();

        agents.sort();
        agents.dedup();

        for agent in agents {
            // an error shows at the delivery (the message's own error event)
            let _ = self.ensure(&agent);
        }
    }
    /// A thread on main: `git pull --ff-only` off the host loop; messages wait for its end.
    pub(crate) fn start_pull(&mut self, thread: &store::Thread, tx: Sender<Input>) {
        self.setup_running = true;
        self.log(Some("setup"), Kind::Tool { id: "pull".into(), name: "Bash".into(), input: json!({"command": "git pull --ff-only", "description": "main up to date: the 1st turn waits for it"}) });
        let dir = thread.work_dir();

        std::thread::spawn(move || {
            let (ok, out) = match crate::domain::threads::update_default(&dir) {
                None => (true, "0 pull: not on main / master with an upstream".to_string()),
                Some(Ok(branch)) => (true, format!("{branch} up to date")),
                Some(Err(error)) => (false, error),
            };

            let _ = tx.send(Input::Pull(ok, out));
        });
    }
    /// The pull ended: its result in the log, the waiting messages go out (a failed pull: with a
    /// note on it, the agents work on the local main).
    pub(crate) fn pull_done(&mut self, ok: bool, out: String) {
        self.setup_running = false;
        self.log(
            Some("setup"),
            Kind::ToolResult {
                id: "pull".into(),
                ok,
                output: out,
            },
        );
        let _ = store::update(&self.id, |thread| thread.pull = false);
        let note = (!ok).then_some(
            "[codehead: `git pull --ff-only` of main failed (output in the thread): main may be behind its upstream.]",
        );

        self.release_queue(note);
    }
}

fn set_pending_setup(setup: bool, thread: &mut store::Thread) {
    if setup {
        thread.setup = Some("pending".into());
    }
}

fn finish_worktree_setup(tx: Sender<Input>, branch: &String, worktree: &Path, port: u16, host: &mut Host) {
    host.log(
        Some("setup"),
        Kind::ToolResult {
            id: "worktree".into(),
            ok: true,
            output: format!("{} on {branch}", worktree.display()),
        },
    );
    let setup = crate::infra::worktree_setup::script(worktree, "setup").is_some();
    let path = worktree.display().to_string();

    let updated = store::update(&host.id, |thread| {
        thread.worktree = Some(path);
        thread.branch = Some(branch.clone());
        thread.port = Some(port);
        thread.worktree_pending = None;

        set_pending_setup(setup, thread);
    });

    match updated {
        Ok(thread) if setup => {
            host.setup_running = false;
            host.start_setup(&thread, tx);

            if host.setup_running {
                // the sessions boot during the setup (the worktree is there)
                host.boot_queued();
            } else {
                host.release_queue(None);
            }
        }
        _ => {
            host.setup_running = false;
            host.release_queue(None);
        }
    }
}

fn prepare_worktree(repo: &Path, branch: &str) -> Result<(PathBuf, u16), String> {
    crate::domain::threads::add_worktree(repo, branch).map(|path| {
        // the worktree stays on the branch; local files (.env …), a port block
        pin_branch(&path, branch);
        crate::infra::worktree_setup::copy_includes(repo, &path);
        (path, crate::infra::worktree_setup::free_port(&store::all()))
    })
}
