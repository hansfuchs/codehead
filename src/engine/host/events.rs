use super::{Duration, Host, INTERRUPTED, Instant, Kind, Out, json, roster, session_ended, store};
use std::ops::ControlFlow;

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
            // the plan's windows are the account's: 1 file for every host
            Out::Limits(line_4) => crate::engine::limits::record("claude", &line_4),
            Out::Mcp(harness, rows) => crate::engine::servers::record(&harness, rows),
            Out::Usage(user) => {
                self.turn_use.insert(agent, user);
            }
            Out::Control(id, req) => if let ControlFlow::Break(()) = harness_control_id_req(&agent, id, req, self) {},
            Out::Exit(err) => {
                harness_exit_err(&agent, &err, self);
            }
        }
    }

    /// Turns a crashed host left open (a `turn_start` with 0 `turn_end`): closed at start.
    /// Claude Code agents: their subagent files (transcripts, nested ones, ends).
    pub(crate) fn scan_subs(&mut self) {
        self.sub_scan = Instant::now();
        let Ok(thread_value) = self.thread() else { return };
        let sessions: Vec<(String, String)> = thread_value
            .sessions
            .iter()
            .filter(|(agent, _)| roster::get(agent).is_some_and(|x| x.harness == "claude"))
            .map(|(agent_value, text)| (agent_value.clone(), text.clone()))
            .collect();
        let root = crate::infra::config::home().join(".claude/projects");

        for (agent_2, key) in self.subagents.scan(&root, &sessions) {
            self.log(Some(&agent_2), key);
        }
    }
    pub(crate) fn close_open_turns(&self) {
        let mut open: Vec<String> = Vec::new();

        close_unfinished_turns(&mut open, self);

        for agent_2 in open {
            self.log(Some(&agent_2), session_ended());
        }
    }
}

fn kill_unresponsive_session(killed: bool, line_6: &mut super::Live) {
    if killed {
        line_6.session.kill();
    }
}

fn harness_exit_err(agent: &str, err: &str, model_state: &mut Host) {
    model_state.live.remove(agent);

    finish_failed_session(agent, err, model_state);

    for (agent_6, key_value) in model_state
        .subagents
        .exit(agent, &format!("the harness process of {agent} ended"))
    {
        model_state.log(Some(&agent_6), key_value);
    }

    model_state.asks.retain(|_, (agent_7, _)| *agent_7 != agent);
}

fn finish_failed_session(agent: &str, err: &str, model_state: &mut Host) {
    let Some(mut line_6) = model_state.sessions.remove(agent) else {
        return;
    };

    // stdout closed or the setup failed: the process can still run. A short
    // grace, then kill: a blocking wait here stops the host loop.
    let at = Instant::now();
    let mut status = None;

    while at.elapsed() < Duration::from_secs(2) {
        match line_6.session.child.try_wait() {
            Ok(Some(text_2)) => {
                status = Some(text_2);
                break;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(_) => break,
        }
    }

    let killed = status.is_none();

    kill_unresponsive_session(killed, &mut line_6);

    let status = status.and_then(|text_3| text_3.code());

    if line_6.busy || killed || status.is_some_and(|character| character != 0) {
        let tail: Vec<&str> = err
            .lines()
            .rev()
            .take(6)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();

        let msg = msg(status, &tail);

        model_state.log(Some(agent), Kind::Error { message: msg });

        close_failed_turn(agent, model_state, &line_6);
    }
}

fn msg(status: Option<i32>, tail: &[&str]) -> String {
    if tail.is_empty() {
        format!("session ended (exit {status:?})")
    } else {
        tail.join("\n")
    }
}

fn close_failed_turn(agent: &str, model_state: &mut Host, line_6: &super::Live) {
    if line_6.busy {
        model_state.log(Some(agent), session_ended());
    }
}

fn record_harness_event(agent: &String, mut kind: Kind, model_state: &mut Host) {
    let mut pending_resume: Option<String> = None;

    if matches!(kind, Kind::Text { .. } | Kind::Tool { .. } | Kind::TurnEnd { .. }) {
        model_state.live.remove(agent);
    }

    if let Kind::TurnEnd { error, .. } = &mut kind
        && let Some(line) = model_state.sessions.get_mut(agent)
        && std::mem::take(&mut line.interrupted)
        && error.is_some()
    {
        *error = Some(INTERRUPTED.into());
    }

    match &kind {
        Kind::Session { session, .. } => {
            // stream-json repeats its init line every turn: 1 event per new session
            if model_state
                .thread()
                .is_ok_and(|thread_value| thread_value.sessions.get(agent) == Some(session))
            {
                return;
            }

            let (agent_value, text_value) = (agent.clone(), session.clone());

            let _ = store::update(&model_state.id, move |thread_2| {
                thread_2.fork.retain(|x| *x != agent_value);
                thread_2.sessions.insert(agent_value, text_value);
            });
        }
        Kind::Text { .. } | Kind::Thought { .. } | Kind::Tool { .. } => {
            // an edit in another repo / worktree: that checkout joins the thread
            if let Kind::Tool { name, input, .. } = &kind
                && let Ok(thread_3) = model_state.thread()
            {
                crate::engine::checkouts::note(&thread_3, name, input);
            }

            // a queued message runs as its own turn: it starts with output
            if let Some(line_value) = model_state.sessions.get_mut(agent)
                && !line_value.busy
            {
                line_value.busy = true;
                model_state.log(Some(agent), Kind::TurnStart);
            }
        }
        Kind::TurnEnd { cost, error, .. } => {
            record_turn_end(agent, model_state, &mut pending_resume, *cost, error.as_deref());
        }
        _ => {}
    }

    let subs = model_state.subagents.event(agent, &kind);
    let boundary = matches!(kind, Kind::ToolResult { .. } | Kind::TurnEnd { .. });

    model_state.log(Some(agent), kind);

    if boundary {
        model_state.flow_boundary(agent);
    }

    for (agent_4, key) in subs {
        model_state.log(Some(&agent_4), key);
    }

    // a usage limit ended the turn: the resume goes in the log after the turn end

    // an interrupted turn: Claude Code ends it with `error_during_execution`

    // a task tool call starts a subagent, its result ends it

    if let Some(error_3) = pending_resume {
        model_state.schedule_resume(agent, &error_3);
    }
}

fn record_turn_end(
    agent: &str,
    model_state: &mut Host,
    pending_resume: &mut Option<String>,
    cost: f64,
    error: Option<&str>,
) {
    model_state.turn_use.remove(agent);

    if let Some(line_2) = model_state.sessions.get_mut(agent) {
        line_2.busy = false;
    }

    // a sub-thread: its turns count towards its end; an error fails it
    model_state.kid.turns += 1;

    if error.is_some() {
        model_state.kid.failed = error.map(String::from);
    }

    if let Some(error_2) = error.filter(|error_value| crate::domain::chat::usage_limit(error_value)) {
        *pending_resume = Some(error_2.to_string());
    }

    // Claude's $: the base of the $ per % of its plan windows
    if roster::get(agent).is_some_and(|agent_2| agent_2.harness.split('/').next() == Some("claude")) {
        crate::engine::limits::add_cost(cost);
    }

    // a turn ends: its open asks are moot (the view drops them too)
    model_state.asks.retain(|_, (agent_3, _)| agent_3.as_str() != agent);

    // its edits go in the log of edits (the next note of every other agent)
    if let Ok(thread_4) = model_state.thread() {
        let idle = !model_state.sessions.values().any(|line_3| line_3.busy);

        crate::engine::edits::turn_end(&model_state.id, &thread_4.work_dir(), agent, idle);
    }
}

fn harness_control_id_req(
    agent: &String,
    id: String,
    req: serde_json::Value,
    model_state: &mut Host,
) -> ControlFlow<()> {
    if let Some(line_5) = model_state.sessions.get_mut(agent)
        && line_5.session.host_allows()
        && let Some(agent_5) = roster::get(agent)
        && let tool = req["tool_name"].as_str().unwrap_or_default()
        && (crate::engine::guard::auto_allow(
            &agent_5.mode,
            tool,
            req["input"]["_kind"].as_str().unwrap_or_default(),
            &req["input"],
        ) || crate::engine::servers::acp_auto_allowed(tool, &crate::engine::servers::for_agent(&agent_5)))
    {
        if let Err(error_4) = line_5.session.allow(&id, &json!({})) {
            model_state.log(
                Some(agent),
                Kind::Error {
                    message: format!("auto-allow: {error_4}"),
                },
            );
        }

        return ControlFlow::Break(());
    }

    // ACP agents: the mode decides what runs without the owner, auto-allowed MCP
    // servers run always

    if req["subtype"] == "can_use_tool" {
        // an ask of a subagent: its name and master go with it
        let mut input = req["input"].clone();

        if let Some((_, result)) = req["agent_id"]
            .as_str()
            .and_then(|thread_5| model_state.subagents.by_task(thread_5))
            && input.is_object()
        {
            input["_subagent"] = json!(result.description);
        }

        model_state.log(
            Some(agent),
            Kind::Ask {
                id: id.clone(),
                tool: req["tool_name"].as_str().unwrap_or_default().into(),
                input,
            },
        );
        model_state.asks.insert(id, (agent.clone(), req));
    }

    ControlFlow::Continue(())
}

fn close_unfinished_turns(open: &mut Vec<String>, model_state: &Host) {
    for error in store::events(&model_state.id) {
        let Some(agent_value) = error.agent else { continue };

        match error.kind {
            Kind::TurnStart if !open.contains(&agent_value) => open.push(agent_value),
            Kind::TurnEnd { .. } | Kind::Leave => open.retain(|x| *x != agent_value),
            _ => {}
        }
    }
}
