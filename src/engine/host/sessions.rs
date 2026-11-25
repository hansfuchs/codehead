use super::{Event, Host, Kind, Live, PathBuf, Sender, Value, harness, json, roster, session_ended, socket, store};

impl Host {
    pub(crate) fn log(&self, agent: Option<&str>, kind: Kind) {
        if let Err(error) = store::append(&self.id, &Event::new(agent, kind)) {
            eprintln!("log: {error}");
        }

        self.push(&json!({"op": "wake"}));
    }
    /// A view subscribed: the live text so far first, then each push.
    pub(crate) fn subscribe(&self, text_value: Sender<String>) {
        for (agent, text) in &self.live {
            let _ = text_value.send(json!({"op": "live", "agent": agent, "text": text}).to_string());
        }

        self.subs.borrow_mut().push(text_value);
    }
    /// Send line V to each subscribed view; closed views go.
    pub(crate) fn push(&self, value: &Value) {
        let line = value.to_string();

        self.subs.borrow_mut().retain(|text| text.send(line.clone()).is_ok());
    }
    pub(crate) fn thread(&self) -> Result<store::Thread, String> {
        store::load(&self.id).ok_or_else(|| format!("thread {} gone", self.id))
    }
    /// AGENT on the thread (joins it first), with a running session.
    pub(crate) fn ensure(&mut self, agent: &str) -> Result<(), String> {
        let mut thread_value = self.thread()?;

        if !thread_value.agents.iter().any(|agent_value| agent_value == agent) {
            roster::get(agent)
                .filter(|agent_2| !agent_2.archived)
                .ok_or_else(|| format!("no agent {agent}"))?;
            thread_value = store::update(&self.id, |thread_2| thread_2.agents.push(agent.to_string()))?;
            self.log(Some(agent), Kind::Join);
        }

        let agent_3 = roster::get(agent).ok_or_else(|| format!("no agent {agent}"))?;
        let mcp = crate::engine::servers::stamp(&agent_3);

        // the MCP servers changed: an idle session restarts (resumed: the conversation stays)
        let ready = self
            .sessions
            .get(agent)
            .is_some_and(|line| line.busy || line.mcp == mcp);

        if ready {
            return Ok(());
        }

        remove_idle_session(agent, self);

        let roster = roster::active();
        let system = harness::system_prompt(&agent_3, &thread_value, &roster);
        let dir = thread_value.work_dir();

        if let Some(path) = crate::engine::checkouts::prune(&thread_value) {
            thread_value = path;
        }

        let extra: Vec<PathBuf> = thread_value
            .all_checkouts()
            .into_iter()
            .skip(1)
            .map(|path_value| PathBuf::from(path_value.dir))
            .filter(|directory| directory.is_dir())
            .collect();
        let spec = harness::Spec {
            agent: &agent_3,
            thread: &self.id,
            dir: &dir,
            resume: thread_value.sessions.get(agent).map(String::as_str),
            fork: thread_value.fork.iter().any(|agent_4| agent_4 == agent),
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
        if let Some(value) = append_resume_context(agent, text, self) {
            return value;
        }

        self.ensure(agent)?;
        let mut text = text.to_string();

        // a new turn: edits in the worktree since the agent's last turn, not its own, go first
        append_edit_context(agent, &mut text, self)?;

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
                Some(count) => format!("{count}\n\n{text}"),
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

        for (agent, line) in sessions {
            stop_session(&agent, line, self);
        }

        let _ = std::fs::remove_file(socket(&self.id));
    }
}

/// A message to AGENT did not go (its harness did not start, …): the error in the log; a
/// sub-thread fails with it (its end comes, 0 turn ever will).
fn undelivered(agent: &str, error: String, model_state: &mut Host) {
    model_state.kid.failed = Some(error.clone());
    model_state.kid.turns += 1;
    model_state.log(Some(agent), Kind::Error { message: error });
}

fn remove_idle_session(agent: &str, model_state: &mut Host) {
    if let Some(mut line_value) = model_state.sessions.remove(agent) {
        line_value.session.kill();
    }
}

fn append_edit_context(agent: &str, text: &mut String, model_state: &mut Host) -> Result<(), String> {
    if model_state.sessions.get(agent).is_some_and(|line| !line.busy) {
        let others = model_state
            .sessions
            .iter()
            .any(|(count, line_value)| count != agent && line_value.busy);
        let dir = model_state.thread()?.work_dir();

        if let Some(count_value) = crate::engine::edits::turn_start(&model_state.id, &dir, agent, others) {
            model_state.log(
                Some(agent),
                Kind::Edits {
                    summary: count_value.summary,
                },
            );
            *text = format!("{}\n\n{text}", count_value.text);
        }
    }

    Ok(())
}

fn append_resume_context(agent: &str, text: &str, model_state: &mut Host) -> Option<Result<(), String>> {
    if model_state
        .thread()
        .is_ok_and(|thread_value| !thread_value.kids.is_empty())
    {
        if model_state.held.is_empty() {
            model_state.log(
                None,
                Kind::SubThread {
                    id: String::new(),
                    title: String::new(),
                    state: "held".into(),
                    text: "messages wait until the sub-thread ends (same checkout)".into(),
                },
            );
        }

        model_state.held.push((agent.to_string(), text.to_string()));
        return Some(Ok(()));
    }

    None
}

fn deliver_or_fail(agent: &str, text: &str, model_state: &mut Host) {
    if let Err(error) = model_state.deliver(agent, text) {
        undelivered(agent, error, model_state);
    }
}

fn stop_session(agent: &str, mut line: Live, host: &mut Host) {
    line.session.kill();

    // a turn the stop cut: closed in the log (else the view shows it at work forever)
    if line.busy {
        host.log(Some(agent), session_ended());
    }
}
