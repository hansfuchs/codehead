use super::{Duration, Exit, Msg, View, store};
use std::fmt::Write;

impl View {
    pub(super) fn branch(&mut self) -> bool {
        if !self.thread.busy.is_empty() {
            self.toast("branch: wait for the turn to end");
            return false;
        }

        if self.forking.is_some() {
            self.toast("branch: on its way");
            return false;
        }

        let (tx, rx) = crate::infra::bus::channel();
        let id = self.id.clone();

        let copy = move || {
            let _ = tx.send(store::fork(&id));
        };

        self.forking = Some(rx);

        // tests: at once
        if cfg!(test) {
            copy();
            return self.fork_due();
        }

        self.toast("branch: copying the thread…");
        std::thread::spawn(copy);

        false
    }

    /// The `>branch` copy is done: true → the view opens it (EXIT).
    pub(super) fn fork_due(&mut self) -> bool {
        let Some(rx) = &self.forking else { return false };
        let Ok(result) = rx.try_recv() else { return false };

        self.forking = None;

        match result {
            Ok(thread) => {
                self.exit = Exit::Open {
                    id: thread.id.clone(),
                    dir: thread.work_dir(),
                    title: thread.title.clone(),
                    note: format!("branched from {}", &self.id[..self.id.len().min(12)]),
                };
                true
            }
            Err(error) => {
                self.toast(format!("branch: {error}"));
                false
            }
        }
    }

    /// `> handoff`: the review model writes the handoff document (on a thread); its end opens
    /// the new thread with the document as its prompt draft.
    /// `>reload`: the idle sessions of the thread's agents end; each agent's next message starts
    /// a new harness process (resumed: the conversation stays) with the MCP servers and settings
    /// of now. A busy agent: after its turn.
    pub(super) fn reload_sessions(&mut self) {
        let Some(thread) = store::load(&self.id) else { return };
        let (idle, busy): (Vec<String>, Vec<String>) = thread
            .agents
            .iter()
            .cloned()
            .partition(|agent| !self.thread.busy.contains(agent));
        let id = self.id.clone();

        // a host call per agent (5 s max each): off the UI thread, the result as a toast
        let run = move || {
            // 0 host: 0 session runs, the next message starts a new one anyway
            reload_sessions_done(idle, &(busy), &(id))
        };

        if cfg!(test) {
            let message = run();

            return self.toast(message);
        }

        let tx = self.tx.clone();

        std::thread::spawn(move || {
            let _ = tx.send(Msg::Toast(run()));
        });
    }

    pub(super) fn handoff(&mut self) {
        if !self.thread.busy.is_empty() {
            self.toast("handoff: wait for the turn to end");
            return;
        }

        if self.handing.is_some() {
            self.toast("handoff: on its way");
            return;
        }

        let Some(model) = crate::infra::config::review_model() else {
            return self.toast("handoff: the review model is off (setting review)");
        };

        let (tx, rx) = crate::infra::bus::channel();
        let id = self.id.clone();

        std::thread::spawn(move || {
            let _ = tx.send(crate::domain::handoff::write(&id, &model));
        });

        self.handing = Some(rx);
        self.toast("handoff: writing the document…");
    }

    /// The handoff document came: the new thread opens. True: the view ends.
    pub(super) fn handoff_due(&mut self) -> bool {
        let Some(rx) = &self.handing else { return false };
        let Ok(result) = rx.try_recv() else { return false };

        self.handing = None;

        let started = result.and_then(|doc| {
            store::load(&self.id)
                .ok_or_else(|| "thread gone".to_string())
                .and_then(|old| crate::domain::handoff::start(&old, &doc))
        });

        match started {
            Ok(thread) => {
                self.exit = Exit::Open {
                    id: thread.id.clone(),
                    dir: thread.work_dir(),
                    title: thread.title.clone(),
                    note: "handoff: edit the document, add the next goal, send".into(),
                };
                true
            }
            Err(error) => {
                self.toast(format!("handoff: {error}"));
                false
            }
        }
    }
}

fn reload_sessions_done(idle: Vec<String>, busy: &[String], id: &str) -> String {
    let done: Vec<String> = idle
        .into_iter()
        .filter(|agent| {
            crate::engine::client::send_within(
                id,
                &serde_json::json!({"op": "reload", "agent": agent}),
                Duration::from_secs(5),
            )
            .is_ok()
        })
        .collect();

    let mut msg = if done.is_empty() {
        "reload: 0 sessions ran · the next message starts new ones".to_string()
    } else {
        format!("reload: {} · the next message starts a new session", done.join(", "))
    };

    if !busy.is_empty() {
        let _ = write!(msg, " · mid-turn: {} (>reload after the turn)", busy.join(", "));
    }

    msg
}
