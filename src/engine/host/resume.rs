use super::{Host, Instant, Kind, RESUME_TEXT, resume_at, store};

impl Host {
    /// AGENT stopped on usage limit ERR: `continue` at the window's reset.
    pub(crate) fn schedule_resume(&mut self, agent: &str, err: &str) {
        let at = resume_at(err, crate::engine::now_ms());

        self.resume.insert(agent.to_string(), at);
        let name = agent.to_string();

        let _ = store::update(&self.id, move |thread| {
            thread.resume.insert(name, at);
        });

        self.log(Some(agent), Kind::Resume { at });
    }

    /// A message for AGENT came first: its resume is off.
    pub(crate) fn cancel_resume(&mut self, agent: &str) {
        remove_pending_resume(agent, self);
    }

    /// Resumes due: `continue` to each agent (a message of codehead in the log).
    pub(crate) fn run_resumes(&mut self) {
        let now = crate::engine::now_ms();
        let due: Vec<String> = self
            .resume
            .iter()
            .filter(|(_, at)| **at <= now)
            .map(|(agent, _)| agent.clone())
            .collect();

        for agent in due {
            self.cancel_resume(&agent);
            self.last = Instant::now();
            self.log(
                None,
                Kind::Prompt {
                    from: "codehead".into(),
                    to: vec![agent.clone()],
                    text: "continue · the usage limit window reset".into(),
                },
            );

            deliver_resumed_turn(&agent, self);
        }
    }
}

fn deliver_resumed_turn(agent: &str, host: &mut Host) {
    if let Err(error) = host.deliver(agent, RESUME_TEXT) {
        host.log(
            Some(agent),
            Kind::Error {
                message: format!("auto resume: {error}"),
            },
        );
    }
}

fn remove_pending_resume(agent: &str, host: &mut Host) {
    if host.resume.remove(agent).is_some() {
        let name = agent.to_string();

        let _ = store::update(&host.id, move |thread| {
            thread.resume.remove(&name);
        });

        host.log(Some(agent), Kind::Resume { at: 0 });
    }
}
