use super::{Duration, Host, INTERRUPTED, Instant, Kind, Out, json, roster, session_ended, store};

impl Host {
    pub(crate) fn harness(&mut self, agent: String, output: Out) {
        self.last = Instant::now();

        match output {
            Out::Event(kind) => {
                record_harness_event(&agent, kind, self);
            }
            Out::Delta(text) => {
                self.push(&json!({"op": "delta", "agent": agent, "text": text}));
                self.live.entry(agent).or_default().push_str(&text);
            }
            // the plan's windows are the account's: 1 file for every host, under the harness
            Out::Limits(limits) => {
                // an agent gone from the roster: Claude Code, the 1st harness to report windows
                let harness = harness_of(&agent).unwrap_or_else(|| "claude".into());

                crate::engine::limits::record(&harness, &limits);
            }
            Out::Mcp(harness, rows) => crate::engine::servers::record(&harness, rows),
            Out::Usage(usage) => {
                self.turn_use.insert(agent, usage);
            }
            Out::Control(id, req) => handle_control_request(&agent, id, req, self),
            Out::Exit(err) => {
                handle_harness_exit(&agent, &err, self);
            }
        }
    }

    /// Claude Code agents: their subagent files (transcripts, nested ones, ends).
    pub(crate) fn scan_subs(&mut self) {
        self.sub_scan = Instant::now();
        let Ok(thread) = self.thread() else { return };
        let sessions: Vec<(String, String)> = thread
            .sessions
            .iter()
            .filter(|(agent, _)| roster::get(agent).is_some_and(|profile| profile.harness == "claude"))
            .map(|(agent, session)| (agent.clone(), session.clone()))
            .collect();
        let root = crate::infra::config::home().join(".claude/projects");

        for (agent, kind) in self.subagents.scan(&root, &sessions) {
            self.log(Some(&agent), kind);
        }
    }

    /// Turns a crashed host left open (a `turn_start` with 0 `turn_end`): closed at start.
    pub(crate) fn close_open_turns(&self) {
        let mut open: Vec<String> = Vec::new();

        collect_open_turns(&mut open, self);

        for agent in open {
            self.log(Some(&agent), session_ended());
        }
    }
}

/// The harness of AGENT (`claude`, `codex`, …): its plan windows and $ sums go under it. None:
/// the agent is gone from the roster.
fn harness_of(agent: &str) -> Option<String> {
    roster::get(agent).and_then(|profile| profile.harness.split('/').next().map(String::from))
}

fn kill_unresponsive_session(killed: bool, live: &mut super::Live) {
    if killed {
        live.session.kill();
    }
}

fn handle_harness_exit(agent: &str, err: &str, host: &mut Host) {
    host.live.remove(agent);

    finish_failed_session(agent, err, host);

    for (master, kind) in host
        .subagents
        .exit(agent, &format!("the harness process of {agent} ended"))
    {
        host.log(Some(&master), kind);
    }

    host.asks.retain(|_, (asker, _)| *asker != agent);
}

fn finish_failed_session(agent: &str, err: &str, host: &mut Host) {
    let Some(mut live) = host.sessions.remove(agent) else {
        return;
    };

    // stdout closed or the setup failed: the process can still run. A short
    // grace, then kill: a blocking wait here stops the host loop.
    let at = Instant::now();
    let mut status = None;

    while at.elapsed() < Duration::from_secs(2) {
        match live.session.child.try_wait() {
            Ok(Some(exited)) => {
                status = Some(exited);
                break;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(_) => break,
        }
    }

    let killed = status.is_none();

    kill_unresponsive_session(killed, &mut live);

    let status = status.and_then(|exit| exit.code());

    if live.busy || killed || status.is_some_and(|code| code != 0) {
        let tail: Vec<&str> = err
            .lines()
            .rev()
            .take(6)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();

        let message = error_message(status, &tail);

        host.log(Some(agent), Kind::Error { message });

        close_failed_turn(agent, host, &live);
    }
}

fn error_message(status: Option<i32>, tail: &[&str]) -> String {
    if tail.is_empty() {
        format!("session ended (exit {status:?})")
    } else {
        tail.join("\n")
    }
}

fn close_failed_turn(agent: &str, host: &mut Host, live: &super::Live) {
    if live.busy {
        host.log(Some(agent), session_ended());
    }
}

fn record_harness_event(agent: &String, mut kind: Kind, host: &mut Host) {
    let mut pending_resume: Option<String> = None;

    if matches!(kind, Kind::Text { .. } | Kind::Tool { .. } | Kind::TurnEnd { .. }) {
        host.live.remove(agent);
    }

    // an interrupted turn: Claude Code ends it with `error_during_execution`
    if let Kind::TurnEnd { error, .. } = &mut kind
        && let Some(live) = host.sessions.get_mut(agent)
        && std::mem::take(&mut live.interrupted)
        && error.is_some()
    {
        *error = Some(INTERRUPTED.into());
    }

    match &kind {
        Kind::Session { session, .. } => {
            // stream-json repeats its init line every turn: 1 event per new session
            if host
                .thread()
                .is_ok_and(|thread| thread.sessions.get(agent) == Some(session))
            {
                return;
            }

            let (agent_name, session_id) = (agent.clone(), session.clone());

            let _ = store::update(&host.id, move |thread| {
                thread.fork.retain(|forked| *forked != agent_name);
                thread.sessions.insert(agent_name, session_id);
            });
        }
        Kind::Text { .. } | Kind::Thought { .. } | Kind::Tool { .. } => {
            // an edit in another repo / worktree: that checkout joins the thread
            if let Kind::Tool { name, input, .. } = &kind
                && let Ok(thread) = host.thread()
            {
                crate::engine::checkouts::note(&thread, name, input);
            }

            // a queued message runs as its own turn: it starts with output
            if let Some(live) = host.sessions.get_mut(agent)
                && !live.busy
            {
                live.busy = true;
                host.log(Some(agent), Kind::TurnStart);
            }
        }
        Kind::TurnEnd { cost, error, .. } => {
            record_turn_end(agent, host, &mut pending_resume, *cost, error.as_deref());
        }
        _ => {}
    }

    // a task tool call starts a subagent, its result ends it
    let subs = host.subagents.event(agent, &kind);
    let boundary = matches!(kind, Kind::ToolResult { .. } | Kind::TurnEnd { .. });

    host.log(Some(agent), kind);

    if boundary {
        host.flow_boundary(agent);
    }

    for (master, event) in subs {
        host.log(Some(&master), event);
    }

    // a usage limit ended the turn: the resume goes in the log after the turn end
    if let Some(limit_error) = pending_resume {
        host.schedule_resume(agent, &limit_error);
    }
}

fn record_turn_end(agent: &str, host: &mut Host, pending_resume: &mut Option<String>, cost: f64, error: Option<&str>) {
    host.turn_use.remove(agent);

    if let Some(live) = host.sessions.get_mut(agent) {
        live.busy = false;
    }

    // a sub-thread: its turns count towards its end; an error fails it
    host.kid.turns += 1;

    if error.is_some() {
        host.kid.failed = error.map(String::from);
    }

    if let Some(limit_error) = error.filter(|message| crate::domain::chat::usage_limit(message)) {
        *pending_resume = Some(limit_error.to_string());
    }

    // the harness's $: the base of the $ per % of its plan windows
    if let Some(harness) = harness_of(agent) {
        crate::engine::limits::add_cost(&harness, cost);
    }

    // a turn ends: its open asks are moot (the view drops them too)
    host.asks.retain(|_, (asker, _)| asker.as_str() != agent);

    // its edits go in the log of edits (the next note of every other agent)
    if let Ok(thread) = host.thread() {
        let idle = !host.sessions.values().any(|live| live.busy);

        crate::engine::edits::turn_end(&host.id, &thread.work_dir(), agent, idle);
    }
}

fn handle_control_request(agent: &String, id: String, req: serde_json::Value, host: &mut Host) {
    // ACP agents: the mode decides what runs without the owner, auto-allowed MCP
    // servers run always
    if let Some(live) = host.sessions.get_mut(agent)
        && live.session.host_allows()
        && let Some(profile) = roster::get(agent)
        && let tool = req["tool_name"].as_str().unwrap_or_default()
        && (crate::engine::guard::auto_allow(
            &profile.mode,
            tool,
            req["input"]["_kind"].as_str().unwrap_or_default(),
            &req["input"],
        ) || crate::engine::servers::acp_auto_allowed(tool, &crate::engine::servers::for_agent(&profile)))
    {
        if let Err(error) = live.session.allow(&id, &json!({})) {
            host.log(
                Some(agent),
                Kind::Error {
                    message: format!("auto-allow: {error}"),
                },
            );
        }

        return;
    }

    if req["subtype"] == "can_use_tool" {
        // an ask of a subagent: its name and master go with it
        let mut input = req["input"].clone();

        if let Some((_, subagent)) = req["agent_id"].as_str().and_then(|task| host.subagents.by_task(task))
            && input.is_object()
        {
            input["_subagent"] = json!(subagent.description);
        }

        host.log(
            Some(agent),
            Kind::Ask {
                id: id.clone(),
                tool: req["tool_name"].as_str().unwrap_or_default().into(),
                input,
            },
        );
        host.asks.insert(id, (agent.clone(), req));
    }
}

fn collect_open_turns(open: &mut Vec<String>, host: &Host) {
    for event in store::events(&host.id) {
        let Some(agent) = event.agent else { continue };

        match event.kind {
            Kind::TurnStart if !open.contains(&agent) => open.push(agent),
            Kind::TurnEnd { .. } | Kind::Leave => open.retain(|other| *other != agent),
            _ => {}
        }
    }
}
