use super::{Duration, Host, Instant, KID_DONE, KID_LOST, KID_TEXT, Kind, json, store};

impl Host {
    /// A sub-thread whose agents are done (1+ turns, 0 busy, 0 waiting message or ask, idle for
    /// `KID_DONE)`: its result (the last agent text) to the thread it runs for, then it goes (thread
    /// deleted, host ends).
    pub(crate) fn kid_check(&mut self) {
        let Ok(thread) = self.thread() else { return };
        let Some(main) = thread.of.clone() else { return };
        let idle = self.kid.turns > 0
            && !self.sessions.values().any(|session| session.busy)
            && self.later.is_empty()
            && self.queue.is_empty()
            && self.asks.is_empty()
            && !self.subagents.any()
            && !self.setup_running;

        if !idle {
            self.kid.idle_at = None;
            return;
        }

        let since = *self.kid.idle_at.get_or_insert_with(Instant::now);

        if since.elapsed() < KID_DONE {
            return;
        }

        // a monitor or background command of its agents runs: not done (its result comes later)
        if crate::domain::monitors::watching(&self.id, &self.sessions.keys().cloned().collect::<Vec<_>>()) {
            self.kid.idle_at = None;
            return;
        }

        let events = store::events_tail(&self.id, 1 << 20);

        // its result: the last message of its agents (the agent that wrote it: the message's)
        let last = events.iter().rev().find_map(|event| match &event.kind {
            Kind::Text { text } if !text.trim().is_empty() => {
                Some((event.agent.clone().unwrap_or_default(), text.trim().to_string()))
            }
            _ => None,
        });
        let lead = thread.agents.first().cloned().unwrap_or_default();

        let (state, agent, text) = match (&self.kid.failed, last) {
            (Some(failure), _) => ("failed", lead, failure.clone()),
            (None, Some((agent, text))) => ("done", agent, text),
            (None, None) => ("done", lead, "0 text from its agents".into()),
        };

        let text: String = text.chars().take(KID_TEXT).collect();
        let _ = crate::engine::client::call(
            &main,
            &json!({"op": "sub_done", "id": self.id, "title": thread.title, "agent": agent, "state": state, "text": text}),
        );
        let _ = store::delete(&self.id);

        self.stopping = true;
    }

    /// Every 5 s, while sub-threads of this thread run: one that is gone (its thread deleted
    /// without its end) or whose host is down `KID_LOST` after its start ends as failed (thread
    /// deleted); the held messages go once 0 runs. With or without an open view.
    pub(crate) fn kids_check(&mut self) {
        if self.kids_at.elapsed() < Duration::from_secs(5) {
            return;
        }

        self.kids_at = Instant::now();
        let Ok(thread) = self.thread() else { return };

        check_child_hosts(thread, self);

        self.deliver_later();
    }

    /// Sub-threads of this thread run: the host stays (it holds messages, it watches them).
    pub(crate) fn has_kids(&self) -> bool {
        self.thread().is_ok_and(|thread| !thread.kids.is_empty())
    }
}

fn delete_lost_child(id: &str, child: store::Thread) -> (std::string::String, std::string::String, &str) {
    let _ = store::delete(id);

    (
        child.title,
        child.agents.first().cloned().unwrap_or_default(),
        "its host ended before its agents were done",
    )
}

fn report_lost_child(id: &str, why: Option<(String, String, &str)>, host: &mut Host) {
    if let Some((title, agent, text)) = why {
        let _ = host
            .cmd(&json!({"op": "sub_done", "id": id, "title": title, "agent": agent, "state": "failed", "text": text}));
    }
}

fn check_child_hosts(thread: store::Thread, host: &mut Host) {
    for id in thread.kids {
        let why = match store::load(&id) {
            None => Some((
                String::new(),
                String::new(),
                "the sub-thread is gone (deleted before its end)",
            )),
            Some(child)
                if !crate::engine::client::alive(&id)
                    && crate::engine::now_ms().saturating_sub(child.created) > KID_LOST =>
            {
                Some(delete_lost_child(&id, child))
            }
            Some(_) => None,
        };

        report_lost_child(&id, why, host);
    }
}
