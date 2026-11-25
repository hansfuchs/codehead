use std::ops::ControlFlow;

use super::{Host, Kind, Value, json, moved_reply, roster, store};

impl Host {
    /// A flow line of AGENT's flow in the log (the event is the agent's). Steps and waits name
    /// their flow after the mark (`▶ chore · step 1/3 implement · @ADA`).
    pub(crate) fn flow_log(&self, agent: &str, op: &str, text: String) {
        let name = self
            .thread()
            .ok()
            .and_then(|thread| thread.flow_of(agent))
            .map(|state| state.title)
            .filter(|title| !title.is_empty());

        let text = match (op, name, text.split_once(' ')) {
            ("step" | "wait", Some(name), Some((mark, rest))) => format!("{mark} {name} · {rest}"),
            _ => text,
        };

        self.log(Some(agent), Kind::Flow { op: op.into(), text });
    }

    /// AGENT reached a tool boundary (or its turn ended): a pending `now` switch of its flow stops
    /// its running turn and applies.
    pub(crate) fn flow_boundary(&mut self, agent: &str) {
        let Ok(thread) = self.thread() else { return };

        let Some(switch) = thread.flow_of(agent).and_then(|state| state.pending) else {
            return;
        };

        if let Some(live) = self.sessions.get_mut(agent).filter(|live| live.busy)
            && live.session.interrupt().is_ok()
        {
            live.interrupted = true;
        }

        if self.sessions.get(agent).is_some_and(|live| live.interrupted) {
            self.asks.retain(|_, (asker, _)| asker != agent);
        }

        if let Err(error) = flow_apply(agent, &switch, None, self) {
            self.flow_log(agent, "warn", format!("switch to {}: {error}", switch.flow));
        }

        self.deliver_later();
    }

    /// The flows of thread T whose current step went to 0 agent yet (a flow set with the task):
    /// each step goes with TEXT under it (the agent's 1st message). Agents in TO: (agent, step)
    /// for the caller to send in place of TEXT; the others get theirs on their own. Marks the steps
    /// sent.
    pub(crate) fn flow_unsent(&mut self, thread: &store::Thread, to: &[String], text: &str) -> Vec<(String, String)> {
        let mut out = Vec::new();

        prepare_unsent_steps(thread, to, text, &mut out, self);

        out
    }

    /// The `flow_*` ops. Owner ops name the flow's agent (`agent`; see `flow_agent`).
    pub(crate) fn flow_cmd(&mut self, op: &str, request: &Value) -> Result<Value, String> {
        let text = |key: &str| request[key].as_str().unwrap_or_default().to_string();
        let now = crate::engine::now_ms();

        match op {
            // the owner sets / switches an agent's flow: MODE `now` (at the agent's next tool
            // boundary), `after` (once its current flow ends); 0 flow running: at once
            "flow_set" => command_flow_set(request, text, self),
            "flow_cancel" => {
                let agent = flow_agent(request, self)?;
                let thread = self.thread()?;
                let mut state = thread.flow_of(&agent).ok_or_else(|| format!("@{agent} has 0 flow"))?;
                let what = state
                    .pending
                    .take()
                    .or(state.queued.take())
                    .ok_or("0 switch to cancel")?;

                flow_save(&agent, &state, self)?;
                self.flow_log(&agent, "set", format!("switch of @{agent} to {} canceled", what.flow));
                Ok(json!({}))
            }
            // an agent reports the current step of its flow done (`flow_step` tool)
            "flow_step" => advance_agent_flow(request, text, now, self),
            // the owner answers the wait: CHOICE `approve` / an outcome (gate), `again` / `next`
            // (loop max); NOTE: the owner's words to the agent
            "flow_answer" => command_flow_answer(request, text, now, self),
            "flow_skip" | "flow_jump" => command_flow_skip(op, request, now, self),
            "flow_clear" => {
                let agent = flow_agent(request, self)?;

                self.flow_log(&agent, "clear", format!("flow of @{agent} removed"));
                store::update(&self.id, |thread| thread.set_flow(&agent, None))?;
                Ok(json!({}))
            }
            // an agent proposes another flow for itself: a line for the owner
            "flow_suggest" => command_flow_suggest(text, self),
            _ => Err(format!("unknown op {op:?}")),
        }
    }
}

fn skip_sent_step(state: &crate::engine::flows::run::FlowState) -> ControlFlow<()> {
    if state.sent || state.finished || state.wait.is_some() {
        return ControlFlow::Break(());
    }

    ControlFlow::Continue(())
}

/// AGENT's flow: the thread, the flow's state, the flow, its nodes.
fn flow_ctx(
    agent: &str,
    host: &Host,
) -> Result<
    (
        store::Thread,
        crate::engine::flows::run::FlowState,
        crate::engine::flows::Flow,
        Vec<crate::engine::flows::Resolved>,
    ),
    String,
> {
    let thread = host.thread()?;
    let state = thread.flow_of(agent).ok_or_else(|| format!("@{agent} has 0 flow"))?;
    let lib = crate::engine::flows::Library::load(Some(&thread.work_dir()));
    let flow = lib
        .flow(&state.flow)
        .cloned()
        .ok_or_else(|| format!("flow {} not found", state.flow))?;
    let nodes = crate::engine::flows::resolve(&flow, &lib);

    Ok((thread, state, flow, nodes))
}

fn flow_save(agent: &str, state: &crate::engine::flows::run::FlowState, host: &Host) -> Result<(), String> {
    let (agent, state) = (agent.to_string(), state.clone());

    store::update(&host.id, move |thread| thread.set_flow(&agent, Some(state))).map(|_| ())
}

/// The agent whose flow an owner op means: `agent`, else the only agent with a flow, else
/// (0 flow) the lead. 2+ flows and 0 `agent`: an error that names them.
fn flow_agent(request: &Value, host: &Host) -> Result<String, String> {
    if let Some(agent) = request["agent"].as_str().filter(|agent| !agent.is_empty()) {
        return Ok(agent.to_string());
    }

    let thread = host.thread()?;
    let flows = thread.flow_map();

    match flows.len() {
        0 => thread
            .lead()
            .map(String::from)
            .ok_or_else(|| "the thread has 0 agents".into()),
        1 => Ok(flows.into_keys().next().unwrap_or_default()),
        _ => Err(format!(
            "2+ agents have a flow ({}): name 1 with `agent`",
            flows
                .keys()
                .map(|agent| format!("@{agent}"))
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

/// The current node of AGENT's flow starts: the agent gets its text. CALLER: the agent whose
/// `flow_step` led here (its text goes back in the tool result, not as a message). NOTE: the
/// owner's words on top. Returns the reply for the caller.
fn flow_enter(agent: &str, caller: Option<&str>, note: Option<&str>, host: &mut Host) -> Result<Value, String> {
    let (thread, mut state, flow, nodes) = flow_ctx(agent, host)?;

    if state.finished {
        return flow_finished(agent, caller, host);
    }

    let Some(step) = nodes.get(state.node) else {
        return Ok(moved_reply(None, None, None));
    };

    host.flow_log(
        agent,
        "step",
        format!("▶ step {}/{} {} · @{agent}", state.node + 1, nodes.len(), step.title),
    );
    let mut message = crate::engine::flows::run::step_message(&flow.name, &nodes, state.node, &state, None);

    if let Some(note) = note.filter(|note| !note.trim().is_empty()) {
        message = format!("[the owner] {}\n\n{message}", note.trim());
    }

    // a thread with 0 message yet: the step goes with the task (the `prompt` op)
    let started = !thread.sessions.is_empty()
        || store::events_tail(&host.id, 1 << 20)
            .iter()
            .any(|event| matches!(&event.kind, Kind::Prompt { .. }));

    if !started {
        return Ok(moved_reply(None, None, None));
    }

    state.sent = true;
    flow_save(agent, &state, host)?;

    if caller == Some(agent) {
        return Ok(moved_reply(Some(&message), None, None));
    }

    host.later.push((agent.to_string(), message));

    Ok(moved_reply(None, Some(&agent.to_string()), None))
}

/// AGENT's flow ended: the queued switch starts, else the end in the log.
fn flow_finished(agent: &str, caller: Option<&str>, host: &mut Host) -> Result<Value, String> {
    let thread = host.thread()?;
    let state = thread.flow_of(agent).unwrap_or_default();

    if let Some(switch) = state.queued.clone() {
        return flow_apply(agent, &switch, caller, host);
    }

    host.flow_log(
        agent,
        "finished",
        format!("✓ flow {} of @{agent} finished", state.title),
    );
    Ok(json!({"text": null, "handed": null, "wait": null, "finished": true}))
}

/// AGENT's flow switches to SWITCH: the new flow's state, its start node enters.
fn flow_apply(
    agent: &str,
    switch: &crate::engine::flows::run::Switch,
    caller: Option<&str>,
    host: &mut Host,
) -> Result<Value, String> {
    let thread = host.thread()?;

    if !thread.agents.iter().any(|member| member == agent) {
        return Err(format!("@{agent} is not on the thread"));
    }

    let lib = crate::engine::flows::Library::load(Some(&thread.work_dir()));
    let flow = lib
        .flow(&switch.flow)
        .cloned()
        .ok_or_else(|| format!("flow {} not found", switch.flow))?;
    let state = crate::engine::flows::run::FlowState::new(&flow, &lib, switch.start, crate::engine::now_ms());
    let old = thread.flow_of(agent);
    let from = old
        .as_ref()
        .filter(|previous| !previous.finished || previous.queued.is_some())
        .map(|previous| previous.title.clone());

    flow_save(agent, &state, host)?;
    let nodes = crate::engine::flows::resolve(&flow, &lib);
    let at = nodes.get(state.node).map(|node| node.title.clone()).unwrap_or_default();

    match from {
        Some(title) if title != flow.name || old.as_ref().is_some_and(|previous| previous.flow != flow.slug) => {
            host.flow_log(
                agent,
                "switch",
                format!("flow of @{agent}: {title} → {} (start: {at})", flow.name),
            );
        }
        _ => host.flow_log(agent, "set", format!("flow of @{agent}: {} (start: {at})", flow.name)),
    }

    flow_enter(agent, caller, None, host)
}

fn command_flow_set(request: &Value, text: impl Fn(&str) -> String, host: &mut Host) -> Result<Value, String> {
    let agent = flow_agent(request, host)?;
    let switch = crate::engine::flows::run::Switch {
        flow: text("flow"),
        start: request["start"].as_u64().unwrap_or(0) as usize,
    };
    let thread = host.thread()?;
    let lib = crate::engine::flows::Library::load(Some(&thread.work_dir()));
    let flow = lib
        .flow(&switch.flow)
        .ok_or_else(|| format!("flow {} not found", switch.flow))?;
    let switch = crate::engine::flows::run::Switch {
        flow: flow.slug.clone(),
        ..switch
    };
    let name = flow.name.clone();
    let current = thread.flow_of(&agent);
    let running = current.as_ref().is_some_and(|saved| !saved.finished);

    match (running, text("mode").as_str()) {
        (true, "after") => {
            let mut state = current.unwrap_or_default();

            state.queued = Some(switch);
            flow_save(&agent, &state, host)?;
            host.flow_log(
                &agent,
                "queued",
                format!("queued: flow {name} after {} (@{agent})", state.title),
            );
            Ok(json!({"queued": name}))
        }
        (true, _) if host.sessions.get(&agent).is_some_and(|live| live.busy) => {
            let mut state = current.unwrap_or_default();

            state.pending = Some(switch);
            flow_save(&agent, &state, host)?;
            host.flow_log(
                &agent,
                "pending",
                format!("switch @{agent} to {name}: its running turn stops at its next tool boundary"),
            );
            Ok(json!({"pending": name}))
        }
        _ => flow_apply(&agent, &switch, None, host),
    }
}

fn advance_agent_flow(
    request: &Value,
    text: impl Fn(&str) -> String,
    now: u64,
    host: &mut Host,
) -> Result<Value, String> {
    let agent = text("agent");
    let (_, mut state, _, nodes) =
        flow_ctx(&agent, host).map_err(|_| format!("@{agent} has 0 flow: 0 step to report"))?;

    if state.finished {
        return Err("the flow finished: 0 step left".into());
    }

    if state.pending.is_some() {
        return Err("the owner switches the flow: stop and wait".into());
    }

    let step = nodes.get(state.node).cloned().ok_or("0 current step")?;
    let pass = request["pass_gate"].as_bool().unwrap_or(false);

    if pass && roster::get(&agent).is_none_or(|profile| profile.mode != "autopilot") {
        return Err("pass_gate: only an agent on autopilot passes a gate; wait for the owner".into());
    }

    let note = text("note");
    let moved = state.step_done(&nodes, request["again"].as_bool().unwrap_or(false), pass, now);

    flow_save(&agent, &state, host)?;

    let tail = if note.is_empty() {
        String::new()
    } else {
        format!(" · {note}")
    };

    match moved {
        crate::engine::flows::run::Moved::Wait(wait) => Ok(report_owner_wait(host, &agent, &step, &tail, &wait)),
        crate::engine::flows::run::Moved::Finished => {
            host.flow_log(
                &agent,
                "step",
                format!("✓ step {}/{} {}{tail}", nodes.len(), nodes.len(), step.title),
            );
            flow_finished(&agent, Some(&agent), host)
        }
        crate::engine::flows::run::Moved::To(_) => {
            if pass && step.gate {
                host.flow_log(
                    &agent,
                    "warn",
                    format!("gate {} passed by @{agent} (autopilot){tail}", step.title),
                );
            } else if !tail.is_empty() {
                host.flow_log(&agent, "step", format!("✓ {}{tail}", step.title));
            }

            flow_enter(&agent, Some(&agent), None, host)
        }
    }
}

fn report_owner_wait(
    host: &mut Host,
    agent: &str,
    step: &crate::engine::flows::Resolved,
    tail: &String,
    wait: &crate::engine::flows::run::Wait,
) -> serde_json::Value {
    let what = match wait {
        crate::engine::flows::run::Wait::Gate => {
            let outcomes: Vec<&str> = std::iter::once("approve")
                .chain(step.edges.iter().map(|(outcome, _)| outcome.as_str()))
                .collect();

            format!(
                "⏸ gate {}: waits for the owner ({}){tail}",
                step.title,
                outcomes.join(" · ")
            )
        }
        crate::engine::flows::run::Wait::LoopMax => format!(
            "⏸ loop of {}: max rounds reached · 1 more round, or move on{tail}",
            step.title
        ),
    };

    host.flow_log(agent, "wait", what);
    moved_reply(None, None, Some("owner"))
}

fn save_unsent_step(agent: &str, state: &crate::engine::flows::run::FlowState, host: &mut Host) -> ControlFlow<()> {
    if flow_save(agent, state, host).is_err() {
        return ControlFlow::Break(());
    }

    ControlFlow::Continue(())
}

fn command_flow_suggest(text: impl Fn(&str) -> String, host: &mut Host) -> Result<Value, String> {
    let thread = host.thread()?;
    let lib = crate::engine::flows::Library::load(Some(&thread.work_dir()));

    let flow = lib.flow(&text("flow")).ok_or_else(|| {
        format!(
            "flow {} not found. flows: {}",
            text("flow"),
            lib.flows
                .iter()
                .map(|listed| listed.slug.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )
    })?;

    host.flow_log(
        &text("agent"),
        "suggest",
        format!(
            "@{} suggests flow {} · {} · agents box (tab) → flow to switch",
            text("agent"),
            flow.name,
            text("reason")
        ),
    );
    Ok(json!({}))
}

fn command_flow_skip(op: &str, request: &Value, now: u64, host: &mut Host) -> Result<Value, String> {
    let agent = flow_agent(request, host)?;
    let (_, mut state, _, nodes) = flow_ctx(&agent, host)?;
    let from = nodes.get(state.node).map(|node| node.title.clone()).unwrap_or_default();

    if op == "flow_skip" {
        state.skip(&nodes, now);
        flow_save(&agent, &state, host)?;
        host.flow_log(&agent, "skip", format!("owner skipped {from}"));
    } else {
        let to = request["node"].as_u64().ok_or("flow_jump: `node` required")? as usize;

        state.jump(&nodes, to, now)?;
        flow_save(&agent, &state, host)?;
        host.flow_log(&agent, "jump", format!("owner: {from} → {}", nodes[to].title));
    }

    if state.finished {
        return flow_finished(&agent, None, host);
    }

    flow_enter(&agent, None, None, host)
}

fn prepare_unsent_steps(
    thread: &store::Thread,
    to: &[String],
    text: &str,
    out: &mut Vec<(String, String)>,
    host: &mut Host,
) {
    for (agent, saved) in thread.flow_map() {
        if let ControlFlow::Break(()) = skip_sent_step(&saved) {
            continue;
        }

        let Ok((_, mut state, flow, nodes)) = flow_ctx(&agent, host) else {
            continue;
        };

        let Some(step) = nodes.get(state.node) else { continue };
        let message = crate::engine::flows::run::step_message(&flow.name, &nodes, state.node, &state, Some(text));

        host.flow_log(
            &agent,
            "step",
            format!("▶ step {}/{} {} · @{agent}", state.node + 1, nodes.len(), step.title),
        );
        state.sent = true;

        if let ControlFlow::Break(()) = save_unsent_step(&agent, &state, host) {
            continue;
        }

        if to.contains(&agent) {
            out.push((agent, message));
        } else {
            host.later.push((agent, message));
        }
    }
}

fn finish_answered_flow(
    agent: &str,
    state: &crate::engine::flows::run::FlowState,
    host: &mut Host,
) -> Option<Result<Value, String>> {
    if state.finished {
        return Some(flow_finished(agent, None, host));
    }

    None
}

fn command_flow_answer(
    request: &Value,
    text: impl Fn(&str) -> String,
    now: u64,
    host: &mut Host,
) -> Result<Value, String> {
    let agent = flow_agent(request, host)?;
    let (_, mut state, _, nodes) = flow_ctx(&agent, host)?;
    let choice = text("choice");
    let title = nodes.get(state.node).map(|node| node.title.clone()).unwrap_or_default();

    state.answer(&nodes, &choice, now)?;
    flow_save(&agent, &state, host)?;
    host.flow_log(&agent, "answer", format!("owner: {choice} · {title}"));

    if let Some(reply) = finish_answered_flow(&agent, &state, host) {
        return reply;
    }

    flow_enter(&agent, None, Some(&text("note")), host)
}
