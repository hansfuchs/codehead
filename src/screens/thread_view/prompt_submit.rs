use super::{Duration, HANDOFF_CHARS, Instant, Msg, Pane, Pasted, Path, View, chat, client, json, store};
use std::fmt::Write;

impl View {
    pub(super) fn submit(&mut self) {
        let mut text = crate::ui::textarea::without_empty_items(&self.prompt.input.text()).to_string();

        if text.trim().is_empty() {
            return;
        }

        // shell mode: `!command` runs here, 0 message
        if let Some(cmd) = text.strip_prefix('!') {
            submit_shell(self, cmd);
            return;
        }

        // `#name` of a shortcut: its text
        text = crate::domain::shortcuts::expand(&text);

        text = with_view_context(self, text);

        let files: Vec<String> = self
            .prompt
            .attachments
            .iter()
            .filter(|(marker, _)| text.contains(marker.as_str()))
            .map(|(marker, path)| format!("- {marker}: {}", path.display()))
            .collect();

        if !files.is_empty() {
            text = format!("{text}\n\n{}\n{}", chat::ATTACHMENTS, files.join("\n"));
        }

        // `>sub task`: a sub-thread does the task, this thread waits
        if let Some(task) = crate::features::activity_rows::subthreads::task_of(&text) {
            submit_subtask(self, task);
            return;
        }

        self.prompt.attachments.clear();
        self.prompt.input.set_text("");
        self.prompt.hist_at = None;
        self.stream.back = 0;
        let replacing = self
            .prompt
            .replace
            .take()
            .filter(|(new, old)| text.contains(&format!("@{new}{}", crate::ui::mention::replaces_note(old))));

        // the `(replaces @old)` note: for the owner only
        let text = crate::ui::mention::strip_replaces(&text);

        if let Some((new, old)) = replacing {
            // OLD mid-turn: the replace waits for its turn to end (esc interrupts it)
            submit_replace(self, text, new, old);
            return;
        }

        self.send_prompt(&text);
    }

    /// An owner message: TEXT to its @names, else to the agents of the owner's last message.
    pub(super) fn send_prompt(&mut self, text: &str) {
        // @name of any agent: the host routes (and brings a new one in); else the default
        let named = named_prompt_agents(self, text);

        self.bring_front(&named);
        self.prompt.pending.push((text.to_string(), Instant::now(), None));

        // the owner's shell commands since the last message: to the agents along
        let context = if self.shell_state.shell_ctx.is_empty() {
            String::new()
        } else {
            format!(
                "[codehead: the owner ran these shell commands since the last message]\n\n{}",
                std::mem::take(&mut self.shell_state.shell_ctx).join("\n\n")
            )
        };

        // a flow wait's reply: to that flow's agent (+ the @named ones)
        if let Some(agent) = self.agents.reply_to.take() {
            let to = reply_recipients(named, agent);

            self.host(
                "send",
                json!({"op": "prompt", "text": text, "to": to, "context": context}),
            );
        } else if named.is_empty() {
            self.host(
                "send",
                json!({"op": "prompt", "text": text, "to": self.default_to(), "context": context}),
            );
        } else {
            self.host("send", json!({"op": "prompt", "text": text, "context": context}));
        }
    }

    /// `>sub TASK`: a new thread for TASK in this thread's checkout (same dir, worktree, branch,
    /// port), 0 history of this one, TASK's @agents (else the agents of the last message). This
    /// thread's host holds its agents' messages until the sub-thread reports its end.
    pub(super) fn start_sub(&mut self, task: String) {
        let Some(main) = store::load(&self.id) else { return };
        let named = named_prompt_agents(self, &task);
        let agents = if named.is_empty() { self.default_to() } else { named };

        if agents.is_empty() {
            self.toast(">sub: 0 agent · @name one".to_string());
            return;
        }

        let first: String = task
            .lines()
            .find(|line| !line.trim().is_empty())
            .unwrap_or_default()
            .trim()
            .to_string();
        let title: String = first.chars().take(60).collect();
        let mut thread = store::Thread::new(&title, &task, Path::new(&main.dir), agents.clone());

        thread.worktree.clone_from(&main.worktree);
        thread.branch.clone_from(&main.branch);
        thread.port = main.port;
        thread.of = Some(main.id.clone());

        if let Err(error) = thread.save() {
            self.toast(format!(">sub: {error}"));
            return;
        }

        let who = format!("@{}", agents.join(" @"));

        self.host(
            "sub_start",
            json!({"op": "sub_start", "id": thread.id, "title": title, "text": format!("{who} · {first}")}),
        );
        self.subthreads
            .started(crate::features::activity_rows::subthreads::Kid {
                id: thread.id.clone(),
                title: title.clone(),
                agents: agents.clone(),
                start: thread.created,
                busy: false,
                asking: false,
                watches: 0,
                steps: Vec::new(),
            });
        let context = format!(
            "[codehead: you run in a sub-thread of the thread “{}”: the same checkout ({}), 0 history of it. The message below is your whole task. Do it, then end with your result as your last message: the thread waits for you; it gets that message.]",
            main.title.lines().next().unwrap_or_default(),
            main.work_dir().display()
        );

        // its host starts in the background: the view never waits for a harness (tests: 0 host)
        let id = thread.id.clone();

        if !cfg!(test) {
            spawn_sub_host(id, agents, task, context);
        }

        self.toast(format!("sub-thread “{title}” started · {who}"));
    }

    /// Stop sub-thread KID: its host ends, its end reported as stopped, the thread deleted.
    pub(super) fn stop_sub(&mut self, kid: &str) {
        let (title, agent) = self
            .subthreads
            .kids
            .iter()
            .find(|sub| sub.id == kid)
            .map(|sub| (sub.title.clone(), sub.agents.first().cloned().unwrap_or_default()))
            .unwrap_or_default();

        self.host("sub_done", json!({"op": "sub_done", "id": kid, "title": title, "agent": agent, "state": "stopped", "text": "stopped by the owner"}));

        // its host stops (2 s max), then its records go: off the UI thread
        let kid_id = kid.to_string();

        let gone = move || {
            let _ = crate::engine::client::send_within(&kid_id, &json!({"op": "stop"}), Duration::from_secs(2));
            let _ = store::delete(&kid_id);
        };

        if cfg!(test) {
            gone();
        } else {
            std::thread::spawn(gone);
        }

        self.subthreads.gone(kid);
        self.toast(format!("sub-thread “{title}” stopped"));
    }

    /// Lost subagent I: its master picks up its work (its transcript, its task), as a message of
    /// the owner to the master.
    pub(super) fn pick_up(&mut self, i: usize) {
        let Some(subagent) = self.thread.subs.get(i).cloned() else {
            return;
        };

        let mut text = format!(
            "@{} your subagent “{}” ended without a report ({}). Pick up its work from its context.",
            subagent.master,
            subagent.description,
            subagent.result.lines().next().unwrap_or("lost")
        );

        if !subagent.transcript.is_empty() {
            let _ = write!(
                text,
                " Its transcript: `{}` (its last steps show where it stopped).",
                subagent.transcript
            );
        }

        if !subagent.prompt.trim().is_empty() {
            let quoted = quote_lines(subagent.prompt.trim());
            let _ = write!(text, "\n\nIts task was:\n\n{quoted}");
        }

        text.push_str("\nFinish the task yourself, or start a new subagent with this context.");
        self.prompt.pending.push((text.clone(), Instant::now(), None));
        self.host("send", json!({"op": "prompt", "text": text, "to": [subagent.master]}));
        self.toast(format!("{} picks up “{}”", subagent.master, subagent.description));
    }

    /// NEW takes over from OLD: a handoff file (task, recent messages, changed files), OLD
    /// leaves, NEW gets TEXT with the file's path.
    pub(super) fn run_replace(&mut self, new: String, old: String, text: &str) {
        let convo = self.thread.last_of(&old, HANDOFF_CHARS);
        let task = store::load(&self.id).map(|thread| thread.task).unwrap_or_default();
        let dir = crate::infra::config::home().join(".cache/codehead/handoff");
        let file = dir.join(format!(
            "{}-{}-{new}.md",
            &self.id[..self.id.len().min(12)],
            crate::engine::now_ms() / 1000
        ));
        let body = text.strip_prefix(&format!("@{new}")).map_or(text, str::trim_start);
        let prompt = format!(
            "{body}\n\n(You take over this thread from {old}. Read {} first: the task, the recent messages, the changed files.)",
            file.display()
        );
        let (id, tx, cwd) = (self.id.clone(), self.tx.clone(), self.cwd.clone());

        // git status, the handoff file, the host calls: off the UI thread
        let replacement = Replacement {
            new,
            old,
            convo,
            task,
            dir,
            file,
            prompt,
            id,
            tx,
            cwd,
        };

        std::thread::spawn(move || replacement.run());

        self.toast("replacing…");
    }

    pub(super) fn paste(&mut self, text: &str) {
        match self.prompt.paster.paste(text) {
            Pasted::Text(pasted) => self.prompt.input.paste(&pasted),
            Pasted::File { path, marker } => {
                self.prompt.input.insert(&marker);
                self.prompt.attachments.push((marker, path));
            }
            Pasted::Nothing => self.toast("clipboard: 0 image, 0 text"),
        }
    }

    pub(super) fn history_step(&mut self, up: bool) {
        if self.prompt.history.is_empty() {
            return;
        }

        let at = match (self.prompt.hist_at, up) {
            (None, true) => {
                self.prompt.draft = self.prompt.input.text();
                Some(self.prompt.history.len() - 1)
            }
            (None, false) => return,
            (Some(0), true) => Some(0),
            (Some(i), true) => Some(i - 1),
            (Some(i), false) if i + 1 < self.prompt.history.len() => Some(i + 1),
            (Some(_), false) => None,
        };

        self.prompt.hist_at = at;
        let text = at.map_or_else(|| self.prompt.draft.clone(), |i| self.prompt.history[i].clone());

        self.prompt.input.load_text(&text);
    }
}

struct Replacement {
    new: String,
    old: String,
    convo: String,
    task: String,
    dir: std::path::PathBuf,
    file: std::path::PathBuf,
    prompt: String,
    id: String,
    tx: crate::infra::bus::Tx<Msg>,
    cwd: std::path::PathBuf,
}
impl Replacement {
    fn run(self) {
        let Self {
            new,
            old,
            convo,
            task,
            dir,
            file,
            prompt,
            id,
            tx,
            cwd,
        } = self;
        let changed = crate::domain::threads::git(&cwd, &["status", "--short"]).1;
        let changed = status_lines(&changed);
        let doc = format!(
            "# Handoff: {old} → {new}\n\n{old} worked on this thread before you. It left the thread; you take over.\n\n## Task of the thread\n\n{task}\n\n## Recent messages of {old}\n\n{convo}\n\n## Changed files (git status --short in {})\n\n```\n{}```\n",
            cwd.display(),
            if changed.is_empty() {
                "0 changes\n".to_string()
            } else {
                changed
            }
        );
        let _ = std::fs::create_dir_all(&dir);

        if let Err(error) = std::fs::write(&file, doc) {
            let _ = tx.send(Msg::Done("replace", Err(format!("handoff file: {error}"))));

            return;
        }

        let replaced = client::call(&id, &json!({"op": "leave", "agent": old, "to": new}))
            .and_then(|_| client::call(&id, &json!({"op": "prompt", "to": [new], "text": prompt})));
        let _ = tx.send(Msg::Done("replace", replaced));
    }
}

fn submit_replace(view: &mut View, text: String, new: String, old: String) {
    if view.thread.busy.contains(&old) {
        view.toast(format!(
            "{new} replaces {old} after {old}'s turn · esc: interrupt {old}"
        ));
        view.prompt.queued = Some((new, old, text));
    } else {
        view.run_replace(new, old, &text);
    }
}

fn with_view_context(view: &mut View, mut text: String) -> String {
    // from the changes view: the selected lines and the picked comments go along (once)
    if view.panes.pane == Pane::Changes
        && let Some(context) = view.panes.changes.context()
    {
        text = format!("{text}\n\n{context}");
        view.mini_chat_state.mini_chat = true;
        view.panes.changes.took_context();
    }

    // from the merge tab: the pull request in view
    if view.panes.pane == Pane::Merge
        && let Some(context) = view.merge_state.merge.context()
    {
        text = format!("{text}\n\n{context}");
    }

    text
}

fn named_prompt_agents(view: &View, text: &str) -> Vec<String> {
    let names: Vec<String> = view
        .agents
        .names
        .iter()
        .map(|(name, _)| name.clone())
        .chain(view.agents.members.iter().cloned())
        .collect();

    crate::engine::host::mentions(text, &names)
}

fn submit_shell(view: &mut View, cmd: &str) {
    let cmd = cmd.trim().to_string();

    if cmd.is_empty() {
        return;
    }

    view.prompt.input.set_text("");
    view.prompt.hist_at = None;
    view.shell_state.shell_at = None;
    view.stream.back = 0;
    view.run_shell(cmd);
}
fn submit_subtask(view: &mut View, task: String) {
    if task.is_empty() {
        view.toast(">sub: the task goes after it (@agent, #shortcut, any text)".to_string());
        return;
    }

    view.prompt.attachments.clear();
    view.prompt.input.set_text("");
    view.prompt.hist_at = None;
    view.stream.back = 0;
    view.start_sub(task);
}
fn reply_recipients(mut recipients: Vec<String>, agent: String) -> Vec<String> {
    if !recipients.contains(&agent) {
        recipients.insert(0, agent);
    }

    recipients
}
fn spawn_sub_host(id: String, agents: Vec<String>, task: String, context: String) {
    std::thread::spawn(move || {
        let _ = crate::engine::client::call(
            &id,
            &json!({"op": "prompt", "to": agents, "text": task, "context": context}),
        );
    });
}

fn quote_lines(text: &str) -> String {
    let mut quoted = String::new();

    for line in text.lines() {
        let _ = writeln!(quoted, "> {line}");
    }

    quoted
}
fn status_lines(text: &str) -> String {
    let mut changed = String::new();

    for line in text.lines().take(80) {
        let _ = writeln!(changed, "{line}");
    }

    changed
}
