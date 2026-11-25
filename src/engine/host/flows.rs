use std::ops::ControlFlow;

use super::{Host, Kind, Value, json, moved_reply, roster, store};

impl Host {
    /// A flow line of AGENT's flow in the log (the event is the agent's). Steps and waits name
    /// their flow after the mark (`▶ chore · step 1/3 implement · @ADA`).
    pub(crate) fn flow_log(&self, agent: &str, op: &str, text: String) {
        let name = self
            .thread()
            .ok()
            .and_then(|thread_value| thread_value.flow_of(agent))
            .map(|file| file.title)
            .filter(|count| !count.is_empty());

        let text = match (op, name, text.split_once(' ')) {
            ("step" | "wait", Some(count_value), Some((mark, rest))) => format!("{mark} {count_value} · {rest}"),
            _ => text,
        };

        self.log(Some(agent), Kind::Flow { op: op.into(), text });
    }
    /// AGENT reached a tool boundary (or its turn ended): a pending `now` switch of its flow stops
    /// its running turn and applies.
    pub(crate) fn flow_boundary(&mut self, agent: &str) {
        let Ok(thread_value) = self.thread() else { return };

        let Some(sw) = thread_value.flow_of(agent).and_then(|text| text.pending) else {
            return;
        };

        if let Some(line_value) = self.sessions.get_mut(agent).filter(|line| line.busy)
            && line_value.session.interrupt().is_ok()
        {
            line_value.interrupted = true;
        }

        if self.sessions.get(agent).is_some_and(|line_2| line_2.interrupted) {
            self.asks.retain(|_, (agent_value, _)| agent_value != agent);
        }

        if let Err(error) = flow_apply(agent, &sw, None, self) {
            self.flow_log(agent, "warn", format!("switch to {}: {error}", sw.flow));
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
    pub(crate) fn flow_cmd(&mut self, op: &str, character: &Value) -> Result<Value, String> {
        let text = |key: &str| character[key].as_str().unwrap_or_default().to_string();
        let now = crate::engine::now_ms();

        match op {
            // the owner sets / switches an agent's flow: MODE `now` (at the agent's next tool
            // boundary), `after` (once its current flow ends); 0 flow running: at once
            "flow_set" => flow_cmd_flow_set(character, text, self),
            "flow_cancel" => {
                let agent = flow_agent(character, self)?;
                let thread_2 = self.thread()?;
                let mut st = thread_2.flow_of(&agent).ok_or_else(|| format!("@{agent} has 0 flow"))?;
                let what = st.pending.take().or(st.queued.take()).ok_or("0 switch to cancel")?;

                flow_save(&agent, &st, self)?;
                self.flow_log(&agent, "set", format!("switch of @{agent} to {} canceled", what.flow));
                Ok(json!({}))
            }
            // an agent reports the current step of its flow done (`flow_step` tool)
            "flow_step" => advance_agent_flow(character, text, now, self),
            // the owner answers the wait: CHOICE `approve` / an outcome (gate), `again` / `next`
            // (loop max); NOTE: the owner's words to the agent
            "flow_answer" => flow_cmd_flow_answer(character, text, now, self),
            "flow_skip" | "flow_jump" => flow_cmd_flow_skip_flow_jump(op, character, now, self),
            "flow_clear" => {
                let agent = flow_agent(character, self)?;

                self.flow_log(&agent, "clear", format!("flow of @{agent} removed"));
                store::update(&self.id, |thread_3| thread_3.set_flow(&agent, None))?;
                Ok(json!({}))
            }
            // an agent proposes another flow for itself: a line for the owner
            "flow_suggest" => flow_cmd_flow_suggest(text, self),
            _ => Err(format!("unknown op {op:?}")),
        }
    }
}

fn skip_sent_step(st: &crate::engine::flows::run::FlowState) -> ControlFlow<()> {
    if st.sent || st.finished || st.wait.is_some() {
        return ControlFlow::Break(());
    }

    ControlFlow::Continue(())
}

/// AGENT's flow: the thread, the flow's state, the flow, its nodes.
fn flow_ctx(
    agent: &str,
    model_state: &Host,
) -> Result<
    (
        store::Thread,
        crate::engine::flows::run::FlowState,
        crate::engine::flows::Flow,
        Vec<crate::engine::flows::Resolved>,
    ),
    String,
> {
    let thread_value = model_state.thread()?;
    let st = thread_value
        .flow_of(agent)
        .ok_or_else(|| format!("@{agent} has 0 flow"))?;
    let lib = crate::engine::flows::Library::load(Some(&thread_value.work_dir()));
    let file = lib
        .flow(&st.flow)
        .cloned()
        .ok_or_else(|| format!("flow {} not found", st.flow))?;
    let nodes = crate::engine::flows::resolve(&file, &lib);

    Ok((thread_value, st, file, nodes))
}

fn flow_save(agent: &str, st: &crate::engine::flows::run::FlowState, model_state: &Host) -> Result<(), String> {
    let (agent, st) = (agent.to_string(), st.clone());

    store::update(&model_state.id, move |thread| thread.set_flow(&agent, Some(st))).map(|_| ())
}

/// The agent whose flow an owner op means: `agent`, else the only agent with a flow, else
/// (0 flow) the lead. 2+ flows and 0 `agent`: an error that names them.
fn flow_agent(character: &Value, model_state: &Host) -> Result<String, String> {
    if let Some(agent_value) = character["agent"].as_str().filter(|agent| !agent.is_empty()) {
        return Ok(agent_value.to_string());
    }

    let thread_value = model_state.thread()?;
    let flows = thread_value.flow_map();

    match flows.len() {
        0 => thread_value
            .lead()
            .map(String::from)
            .ok_or_else(|| "the thread has 0 agents".into()),
        1 => Ok(flows.into_keys().next().unwrap_or_default()),
        _ => Err(format!(
            "2+ agents have a flow ({}): name 1 with `agent`",
            flows
                .keys()
                .map(|agent_2| format!("@{agent_2}"))
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

/// The current node of AGENT's flow starts: the agent gets its text. CALLER: the agent whose
/// `flow_step` led here (its text goes back in the tool result, not as a message). NOTE: the
/// owner's words on top. Returns the reply for the caller.
fn flow_enter(agent: &str, caller: Option<&str>, note: Option<&str>, model_state: &mut Host) -> Result<Value, String> {
    let (thread, mut st, file, nodes) = flow_ctx(agent, model_state)?;

    if st.finished {
        return flow_finished(agent, caller, model_state);
    }

    let Some(count) = nodes.get(st.node) else {
        return Ok(moved_reply(None, None, None));
    };

    model_state.flow_log(
        agent,
        "step",
        format!("▶ step {}/{} {} · @{agent}", st.node + 1, nodes.len(), count.title),
    );
    let mut msg = crate::engine::flows::run::step_message(&file.name, &nodes, st.node, &st, None);

    if let Some(count_2) = note.filter(|count_value| !count_value.trim().is_empty()) {
        msg = format!("[the owner] {}\n\n{msg}", count_2.trim());
    }

    // a thread with 0 message yet: the step goes with the task (the `prompt` op)
    let started = !thread.sessions.is_empty()
        || store::events_tail(&model_state.id, 1 << 20)
            .iter()
            .any(|error| matches!(&error.kind, Kind::Prompt { .. }));

    if !started {
        return Ok(moved_reply(None, None, None));
    }

    st.sent = true;
    flow_save(agent, &st, model_state)?;

    if caller == Some(agent) {
        return Ok(moved_reply(Some(&msg), None, None));
    }

    model_state.later.push((agent.to_string(), msg));

    Ok(moved_reply(None, Some(&agent.to_string()), None))
}

/// AGENT's flow ended: the queued switch starts, else the end in the log.
fn flow_finished(agent: &str, caller: Option<&str>, model_state: &mut Host) -> Result<Value, String> {
    let thread_value = model_state.thread()?;
    let st = thread_value.flow_of(agent).unwrap_or_default();

    if let Some(sw) = st.queued.clone() {
        return flow_apply(agent, &sw, caller, model_state);
    }

    model_state.flow_log(agent, "finished", format!("✓ flow {} of @{agent} finished", st.title));
    Ok(json!({"text": null, "handed": null, "wait": null, "finished": true}))
}

/// AGENT's flow switches to SW: the new flow's state, its start node enters.
fn flow_apply(
    agent: &str,
    sw: &crate::engine::flows::run::Switch,
    caller: Option<&str>,
    model_state: &mut Host,
) -> Result<Value, String> {
    let thread_value = model_state.thread()?;

    if !thread_value.agents.iter().any(|agent_value| agent_value == agent) {
        return Err(format!("@{agent} is not on the thread"));
    }

    let lib = crate::engine::flows::Library::load(Some(&thread_value.work_dir()));
    let file = lib
        .flow(&sw.flow)
        .cloned()
        .ok_or_else(|| format!("flow {} not found", sw.flow))?;
    let st = crate::engine::flows::run::FlowState::new(&file, &lib, sw.start, crate::engine::now_ms());
    let old = thread_value.flow_of(agent);
    let from = old
        .as_ref()
        .filter(|output| !output.finished || output.queued.is_some())
        .map(|output_value| output_value.title.clone());

    flow_save(agent, &st, model_state)?;
    let nodes = crate::engine::flows::resolve(&file, &lib);
    let at = nodes.get(st.node).map(|count| count.title.clone()).unwrap_or_default();

    match from {
        Some(output_2) if output_2 != file.name || old.as_ref().is_some_and(|output_3| output_3.flow != file.slug) => {
            model_state.flow_log(
                agent,
                "switch",
                format!("flow of @{agent}: {output_2} → {} (start: {at})", file.name),
            );
        }
        _ => model_state.flow_log(agent, "set", format!("flow of @{agent}: {} (start: {at})", file.name)),
    }

    flow_enter(agent, caller, None, model_state)
}

fn flow_cmd_flow_set(
    character: &Value,
    text: impl Fn(&str) -> String,
    model_state: &mut Host,
) -> Result<Value, String> {
    let agent = flow_agent(character, model_state)?;
    let sw = crate::engine::flows::run::Switch {
        flow: text("flow"),
        start: character["start"].as_u64().unwrap_or(0) as usize,
    };
    let thread_value = model_state.thread()?;
    let lib = crate::engine::flows::Library::load(Some(&thread_value.work_dir()));
    let file = lib
        .flow(&sw.flow)
        .ok_or_else(|| format!("flow {} not found", sw.flow))?;
    let sw = crate::engine::flows::run::Switch {
        flow: file.slug.clone(),
        ..sw
    };
    let name = file.name.clone();
    let cur = thread_value.flow_of(&agent);
    let running = cur.as_ref().is_some_and(|file_value| !file_value.finished);

    match (running, text("mode").as_str()) {
        (true, "after") => {
            let mut st = cur.unwrap_or_default();

            st.queued = Some(sw);
            flow_save(&agent, &st, model_state)?;
            model_state.flow_log(
                &agent,
                "queued",
                format!("queued: flow {name} after {} (@{agent})", st.title),
            );
            Ok(json!({"queued": name}))
        }
        (true, _) if model_state.sessions.get(&agent).is_some_and(|line| line.busy) => {
            let mut st = cur.unwrap_or_default();

            st.pending = Some(sw);
            flow_save(&agent, &st, model_state)?;
            model_state.flow_log(
                &agent,
                "pending",
                format!("switch @{agent} to {name}: its running turn stops at its next tool boundary"),
            );
            Ok(json!({"pending": name}))
        }
        _ => flow_apply(&agent, &sw, None, model_state),
    }
}

fn advance_agent_flow(
    character: &Value,
    text: impl Fn(&str) -> String,
    now: u64,
    model_state: &mut Host,
) -> Result<Value, String> {
    let agent = text("agent");
    let (_, mut st, _, nodes) =
        flow_ctx(&agent, model_state).map_err(|_| format!("@{agent} has 0 flow: 0 step to report"))?;

    if st.finished {
        return Err("the flow finished: 0 step left".into());
    }

    if st.pending.is_some() {
        return Err("the owner switches the flow: stop and wait".into());
    }

    let count = nodes.get(st.node).cloned().ok_or("0 current step")?;
    let pass = character["pass_gate"].as_bool().unwrap_or(false);

    if pass && roster::get(&agent).is_none_or(|agent_value| agent_value.mode != "autopilot") {
        return Err("pass_gate: only an agent on autopilot passes a gate; wait for the owner".into());
    }

    let note = text("note");
    let moved = st.step_done(&nodes, character["again"].as_bool().unwrap_or(false), pass, now);

    flow_save(&agent, &st, model_state)?;

    let tail = if note.is_empty() {
        String::new()
    } else {
        format!(" · {note}")
    };

    match moved {
        crate::engine::flows::run::Moved::Wait(width) => {
            Ok(report_owner_wait(model_state, &agent, &count, &tail, &width))
        }
        crate::engine::flows::run::Moved::Finished => {
            model_state.flow_log(
                &agent,
                "step",
                format!("✓ step {}/{} {}{tail}", nodes.len(), nodes.len(), count.title),
            );
            flow_finished(&agent, Some(&agent), model_state)
        }
        crate::engine::flows::run::Moved::To(_) => {
            if pass && count.gate {
                model_state.flow_log(
                    &agent,
                    "warn",
                    format!("gate {} passed by @{agent} (autopilot){tail}", count.title),
                );
            } else if !tail.is_empty() {
                model_state.flow_log(&agent, "step", format!("✓ {}{tail}", count.title));
            }

            flow_enter(&agent, Some(&agent), None, model_state)
        }
    }
}

fn report_owner_wait(
    model_state: &mut Host,
    agent: &str,
    count: &crate::engine::flows::Resolved,
    tail: &String,
    width: &crate::engine::flows::run::Wait,
) -> serde_json::Value {
    let what = match width {
        crate::engine::flows::run::Wait::Gate => {
            let outs: Vec<&str> = std::iter::once("approve")
                .chain(count.edges.iter().map(|(key_value, _)| key_value.as_str()))
                .collect();

            format!(
                "⏸ gate {}: waits for the owner ({}){tail}",
                count.title,
                outs.join(" · ")
            )
        }
        crate::engine::flows::run::Wait::LoopMax => format!(
            "⏸ loop of {}: max rounds reached · 1 more round, or move on{tail}",
            count.title
        ),
    };

    model_state.flow_log(agent, "wait", what);
    moved_reply(None, None, Some("owner"))
}

fn save_unsent_step(agent: &str, st: &crate::engine::flows::run::FlowState, model_state: &mut Host) -> ControlFlow<()> {
    if flow_save(agent, st, model_state).is_err() {
        return ControlFlow::Break(());
    }

    ControlFlow::Continue(())
}

fn flow_cmd_flow_suggest(text: impl Fn(&str) -> String, model_state: &mut Host) -> Result<Value, String> {
    let thread_4 = model_state.thread()?;
    let lib = crate::engine::flows::Library::load(Some(&thread_4.work_dir()));

    let file_3 = lib.flow(&text("flow")).ok_or_else(|| {
        format!(
            "flow {} not found. flows: {}",
            text("flow"),
            lib.flows
                .iter()
                .map(|file_2| file_2.slug.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )
    })?;

    model_state.flow_log(
        &text("agent"),
        "suggest",
        format!(
            "@{} suggests flow {} · {} · agents box (tab) → flow to switch",
            text("agent"),
            file_3.name,
            text("reason")
        ),
    );
    Ok(json!({}))
}

fn flow_cmd_flow_skip_flow_jump(
    op: &str,
    character: &Value,
    now: u64,
    model_state: &mut Host,
) -> Result<Value, String> {
    let agent = flow_agent(character, model_state)?;
    let (_, mut st, _, nodes) = flow_ctx(&agent, model_state)?;
    let from = nodes
        .get(st.node)
        .map(|count_2| count_2.title.clone())
        .unwrap_or_default();

    if op == "flow_skip" {
        st.skip(&nodes, now);
        flow_save(&agent, &st, model_state)?;
        model_state.flow_log(&agent, "skip", format!("owner skipped {from}"));
    } else {
        let to = character["node"].as_u64().ok_or("flow_jump: `node` required")? as usize;

        st.jump(&nodes, to, now)?;
        flow_save(&agent, &st, model_state)?;
        model_state.flow_log(&agent, "jump", format!("owner: {from} → {}", nodes[to].title));
    }

    if st.finished {
        return flow_finished(&agent, None, model_state);
    }

    flow_enter(&agent, None, None, model_state)
}

fn prepare_unsent_steps(
    thread: &store::Thread,
    to: &[String],
    text: &str,
    out: &mut Vec<(String, String)>,
    model_state: &mut Host,
) {
    for (agent, st) in thread.flow_map() {
        if let ControlFlow::Break(()) = skip_sent_step(&st) {
            continue;
        }

        let Ok((_, mut st, file, nodes)) = flow_ctx(&agent, model_state) else {
            continue;
        };

        let Some(count) = nodes.get(st.node) else { continue };
        let msg = crate::engine::flows::run::step_message(&file.name, &nodes, st.node, &st, Some(text));

        model_state.flow_log(
            &agent,
            "step",
            format!("▶ step {}/{} {} · @{agent}", st.node + 1, nodes.len(), count.title),
        );
        st.sent = true;

        if let ControlFlow::Break(()) = save_unsent_step(&agent, &st, model_state) {
            continue;
        }

        if to.contains(&agent) {
            out.push((agent, msg));
        } else {
            model_state.later.push((agent, msg));
        }
    }
}

fn finish_answered_flow(
    agent: &str,
    st: &crate::engine::flows::run::FlowState,
    model_state: &mut Host,
) -> Option<Result<Value, String>> {
    if st.finished {
        return Some(flow_finished(agent, None, model_state));
    }

    None
}

fn flow_cmd_flow_answer(
    character: &Value,
    text: impl Fn(&str) -> String,
    now: u64,
    model_state: &mut Host,
) -> Result<Value, String> {
    let agent = flow_agent(character, model_state)?;
    let (_, mut st, _, nodes) = flow_ctx(&agent, model_state)?;
    let choice = text("choice");
    let title = nodes
        .get(st.node)
        .map(|count_value| count_value.title.clone())
        .unwrap_or_default();

    st.answer(&nodes, &choice, now)?;
    flow_save(&agent, &st, model_state)?;
    model_state.flow_log(&agent, "answer", format!("owner: {choice} · {title}"));

    if let Some(value) = finish_answered_flow(&agent, &st, model_state) {
        return value;
    }

    flow_enter(&agent, None, Some(&text("note")), model_state)
}
