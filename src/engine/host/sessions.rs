use super::{Event, Host, Kind, Live, PathBuf, Sender, Value, harness, json, roster, session_ended, socket, store};

impl Host {
    pub(crate) fn log(&self, agent: Option<&str>, kind: Kind) {
        if let Err(error) = store::append(&self.id, &Event::new(agent, kind)) {
            eprintln!("log: {error}");
        }

        self.push(&json!({"op": "wake"}));
    }
    /// A view subscribed: the live text so far first, then each push.
    pub(crate) fn subscribe(&self, subscriber: Sender<String>) {
        for (agent, text) in &self.live {
            let _ = subscriber.send(json!({"op": "live", "agent": agent, "text": text}).to_string());
        }

        self.subs.borrow_mut().push(subscriber);
    }
    /// Send VALUE as 1 line to each subscribed view; closed views go.
    pub(crate) fn push(&self, value: &Value) {
        let line = value.to_string();

        self.subs
            .borrow_mut()
            .retain(|subscriber| subscriber.send(line.clone()).is_ok());
    }
    pub(crate) fn thread(&self) -> Result<store::Thread, String> {
        store::load(&self.id).ok_or_else(|| format!("thread {} gone", self.id))
    }
    /// AGENT on the thread (joins it first), with a running session.
    pub(crate) fn ensure(&mut self, agent: &str) -> Result<(), String> {
        let mut thread = self.thread()?;

        if !thread.agents.iter().any(|member| member == agent) {
            roster::get(agent)
                .filter(|profile| !profile.archived)
                .ok_or_else(|| format!("no agent {agent}"))?;
            thread = store::update(&self.id, |stored| stored.agents.push(agent.to_string()))?;
            self.log(Some(agent), Kind::Join);
        }

        let profile = roster::get(agent).ok_or_else(|| format!("no agent {agent}"))?;
        let mcp = crate::engine::servers::stamp(&profile);

        // the MCP servers changed: an idle session restarts (resumed: the conversation stays)
        let ready = self
            .sessions
            .get(agent)
            .is_some_and(|live| live.busy || live.mcp == mcp);

        if ready {
            return Ok(());
        }

        remove_idle_session(agent, self);

        let roster = roster::active();
        let system = harness::system_prompt(&profile, &thread, &roster);
        let dir = thread.work_dir();

        if let Some(pruned) = crate::engine::checkouts::prune(&thread) {
            thread = pruned;
        }

        let extra: Vec<PathBuf> = thread
            .all_checkouts()
            .into_iter()
            .skip(1)
            .map(|checkout| PathBuf::from(checkout.dir))
            .filter(|path| path.is_dir())
            .collect();
        let spec = harness::Spec {
            agent: &profile,
            thread: &self.id,
            dir: &dir,
            resume: thread.sessions.get(agent).map(String::as_str),
            fork: thread.fork.iter().any(|forked| forked == agent),
            system: &system,
            exe: &self.exe,
            extra: &extra,
        };
        let session = harness::spawn(&spec, self.harness_tx.clone())?;

        self.sessions.insert(
            agent.to_string(),
            Live {
                session,
                busy: false,
                interrupted: false,
                mcp,
            },
        );

        Ok(())
    }

    pub(crate) fn deliver(&mut self, agent: &str, text: &str) -> Result<(), String> {
        // the worktree setup runs: the message waits for its end
        if self.setup_running {
            self.queue.push((agent.to_string(), text.to_string()));

            // its session boots meanwhile (a harness takes seconds): only the message waits
            self.boot_queued();
            return Ok(());
        }

        // a sub-thread works in the same checkout: the message waits for its end (`sub_done`)
        if let Some(held) = hold_for_sub_thread(agent, text, self) {
            return held;
        }

        self.ensure(agent)?;
        let mut text = text.to_string();

        // a new turn: edits in the worktree since the agent's last turn, not its own, go first
        prepend_edits(agent, &mut text, self)?;

        let live = self.sessions.get_mut(agent).ok_or("0 session")?;

        live.session.prompt(&text)?;

        if !live.busy {
            live.busy = true;
            self.log(Some(agent), Kind::TurnStart);
        }

        Ok(())
    }

    /// The waiting messages go out, each with NOTE on top.
    pub(crate) fn release_queue(&mut self, note: Option<&str>) {
        for (agent, text) in std::mem::take(&mut self.queue) {
            let text = match note {
                Some(note) => format!("{note}\n\n{text}"),
                None => text,
            };

            deliver_or_fail(&agent, &text, self);
        }
    }

    /// The messages of the last command go out (its reply went first).
    pub(crate) fn deliver_later(&mut self) {
        for (agent, text) in std::mem::take(&mut self.later) {
            deliver_or_fail(&agent, &text, self);
        }
    }
    pub(crate) fn stop(&mut self) {
        let sessions: Vec<(String, Live)> = self.sessions.drain().collect();

        for (agent, live) in sessions {
            stop_session(&agent, live, self);
        }

        let _ = std::fs::remove_file(socket(&self.id));
    }
}

/// A message to AGENT did not go (its harness did not start, …): the error in the log; a
/// sub-thread fails with it (its end comes, 0 turn ever will).
fn undelivered(agent: &str, error: String, host: &mut Host) {
    host.kid.failed = Some(error.clone());
    host.kid.turns += 1;
    host.log(Some(agent), Kind::Error { message: error });
}

fn remove_idle_session(agent: &str, host: &mut Host) {
    if let Some(mut live) = host.sessions.remove(agent) {
        live.session.kill();
    }
}

fn prepend_edits(agent: &str, text: &mut String, host: &mut Host) -> Result<(), String> {
    if host.sessions.get(agent).is_some_and(|live| !live.busy) {
        let others = host.sessions.iter().any(|(name, live)| name != agent && live.busy);
        let dir = host.thread()?.work_dir();

        if let Some(edits) = crate::engine::edits::turn_start(&host.id, &dir, agent, others) {
            host.log(Some(agent), Kind::Edits { summary: edits.summary });
            *text = format!("{}\n\n{text}", edits.text);
        }
    }

    Ok(())
}

fn hold_for_sub_thread(agent: &str, text: &str, host: &mut Host) -> Option<Result<(), String>> {
    if host.thread().is_ok_and(|thread| !thread.kids.is_empty()) {
        if host.held.is_empty() {
            host.log(
                None,
                Kind::SubThread {
                    id: String::new(),
                    title: String::new(),
                    state: "held".into(),
                    text: "messages wait until the sub-thread ends (same checkout)".into(),
                },
            );
        }

        host.held.push((agent.to_string(), text.to_string()));
        return Some(Ok(()));
    }

    None
}

fn deliver_or_fail(agent: &str, text: &str, host: &mut Host) {
    if let Err(error) = host.deliver(agent, text) {
        undelivered(agent, error, host);
    }
}

fn stop_session(agent: &str, mut live: Live, host: &mut Host) {
    live.session.kill();

    // a turn the stop cut: closed in the log (else the view shows it at work forever)
    if live.busy {
        host.log(Some(agent), session_ended());
    }
}
