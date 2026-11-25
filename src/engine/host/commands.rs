use super::{Host, Instant, Kind, Value, json, mentions, roster, store};

impl Host {
    pub(crate) fn cmd(&mut self, request: &Value) -> Result<Value, String> {
        // a status poll (`ping`) is no activity: an open view must not keep the host alive
        if request["op"].as_str() != Some("ping") {
            self.last = Instant::now();
        }

        match request["op"].as_str().unwrap_or_default() {
            "ping" => Ok(command_ping(self)),
            "prompt" => command_prompt(request, self),
            "send" => command_send(request, self),
            "answer" => command_answer(request, self),
            // the owner stops subagent ID: a background one of Claude Code through its master
            // (TaskStop), a foreground one with its master's turn
            "sub_stop" => command_sub_stop(request, self),
            "interrupt" => command_interrupt(self),
            "join" => command_join(request, self),
            // AGENT leaves; its flow goes, or (TO: the agent that replaces it) goes to TO: the
            // current step again with TO's 1st message
            "leave" => command_leave(request, self),
            "reload" => command_reload(request, self),
            "status" => command_status(request, self),
            "stop" => Ok(command_stop(self)),
            // a sub-thread of this thread started (the view made it): its agents' messages wait
            "sub_start" => command_sub_start(request, self),
            // a sub-thread ended (its host reports, or the view found it gone): its result as a
            // message of AGENT (its agent; 0: unknown), the waiting messages go once 0 sub-thread
            // runs. An id not running: 0 effect.
            "sub_done" => command_sub_done(request, self),
            op if op.starts_with("flow_") => self.flow_cmd(op, request),
            op => Err(format!("unknown op {op:?}")),
        }
    }
}

fn set_answer_option(request: &Value, input: &mut Value) {
    if let (Some(output), Some(opt)) = (input.as_object_mut(), request["option"].as_str()) {
        output.insert("_option".into(), Value::String(opt.into()));
    }
}

fn request_text(request: &Value, key: &str) -> String {
    request[key].as_str().unwrap_or_default().to_string()
}
fn request_list(request: &Value, key: &str) -> Vec<String> {
    match &request[key] {
        Value::String(text) => text
            .split(',')
            .map(|name| name.trim().trim_start_matches('@').to_string())
            .filter(|name| !name.is_empty())
            .collect(),
        Value::Array(values) => values
            .iter()
            .filter_map(Value::as_str)
            .map(|name| name.trim_start_matches('@').to_string())
            .collect(),
        _ => Vec::new(),
    }
}

fn command_ping(model_state: &mut Host) -> serde_json::Value {
    json!({
        "live": model_state.sessions.keys().collect::<Vec<_>>(),
        // busy: in a turn, or waiting on running subagents
        "busy": model_state.sessions.iter().filter(|(name, live)| live.busy || model_state.subagents.masters().contains(*name)).map(|(name, _)| name).collect::<Vec<_>>(),
        "asks": model_state.asks.len(),
        "usage": model_state.turn_use,
    })
}

fn command_prompt(request: &Value, model_state: &mut Host) -> Result<Value, String> {
    let text = request_text(request, "text");

    if text.trim().is_empty() {
        return Err("empty message".into());
    }

    let thread_value = model_state.thread()?;
    let mut to = request_list(request, "to");

    if to.is_empty() {
        let names: Vec<String> = roster::active().into_iter().map(|agent_2| agent_2.name).collect();

        to = mentions(&text, &names);
    }

    if to.is_empty() {
        to = thread_value
            .lead()
            .map(|line_value| vec![line_value.to_string()])
            .ok_or("thread has 0 agents")?;
    }

    model_state.log(
        None,
        Kind::Prompt {
            from: "owner".into(),
            to: to.clone(),
            text: text.clone(),
        },
    );

    for agent_3 in &to {
        model_state.cancel_resume(agent_3);
    }

    if thread_value.state != "open" {
        store::update(&model_state.id, |thread_2| thread_2.state = "open".into())?;
    }

    // CONTEXT (the owner's shell commands since the last message): for the agents only
    let ctx = request_text(request, "context");

    let full = if ctx.trim().is_empty() {
        text.clone()
    } else {
        format!("{text}\n\n{ctx}")
    };

    // a flow whose current step went to 0 agent yet: the step goes with this message
    let steps = model_state.flow_unsent(&thread_value, &to, &full);

    model_state.later.extend(
        to.iter()
            .map(|agent_4| match steps.iter().find(|(sa, _)| sa == agent_4) {
                Some((_, message)) => (agent_4.clone(), message.clone()),
                None => (agent_4.clone(), full.clone()),
            }),
    );

    Ok(json!({"to": to}))
}

fn command_send(request: &Value, model_state: &mut Host) -> Result<Value, String> {
    let (from, text, to) = (
        request_text(request, "from"),
        request_text(request, "text"),
        request_list(request, "to"),
    );

    if to.is_empty() || text.trim().is_empty() {
        return Err("send: `to` and `message` required".into());
    }

    if to.contains(&from) {
        return Err("send: 0 messages to yourself".into());
    }

    for agent_5 in &to {
        roster::get(agent_5)
            .filter(|agent_6| !agent_6.archived)
            .ok_or_else(|| format!("no agent {agent_5}"))?;
    }

    model_state.log(
        Some(&from),
        Kind::Prompt {
            from: from.clone(),
            to: to.clone(),
            text: text.clone(),
        },
    );

    for agent_7 in &to {
        model_state.cancel_resume(agent_7);
    }

    for agent_8 in &to {
        model_state.deliver(agent_8, &format!("[from @{from}] {text}"))?;
    }

    Ok(json!({"to": to}))
}

fn command_answer(request: &Value, model_state: &mut Host) -> Result<Value, String> {
    let id = request_text(request, "id");
    let (agent, req) = model_state
        .asks
        .get(&id)
        .cloned()
        .ok_or_else(|| format!("no open ask {id}"))?;

    let Some(live) = model_state.sessions.get_mut(&agent) else {
        model_state.asks.remove(&id);
        return Err(format!("{agent}: session gone"));
    };

    let allow = request["allow"].as_bool().unwrap_or(false);

    // an ACP option picks itself, allow or reject
    if allow || request["option"].is_string() {
        let mut input = req["input"].clone();

        set_answer_option(request, &mut input);

        if let (Some(output_value), Some(ans)) = (
            input.as_object_mut(),
            request.get("answers").filter(|agent_9| agent_9.is_object()),
        ) {
            output_value.insert("answers".into(), ans.clone());
        }

        live.session.allow(&id, &input)?;
    } else {
        let why = request["note"]
            .as_str()
            .filter(|count| !count.is_empty())
            .unwrap_or("The owner said no.");

        live.session.deny(&id, why)?;
    }

    // answered: the ask goes only now (a failed write keeps it open for a retry)
    model_state.asks.remove(&id);
    model_state.log(
        Some(&agent),
        Kind::Answer {
            id,
            allow,
            label: request_text(request, "label"),
        },
    );

    Ok(json!({}))
}

fn command_sub_stop(request: &Value, model_state: &mut Host) -> Result<Value, String> {
    let id = request_text(request, "id");
    let result_value = model_state
        .subagents
        .stop(&id)
        .ok_or_else(|| format!("subagent {id}: 0 running"))?;
    let master = result_value.agent.clone();

    let how = if result_value.background && !result_value.task.is_empty() {
        let text = format!(
            "[codehead] The owner stopped your background subagent “{}” (task id {}). Stop it now with TaskStop (task_id: {}) and go on without its result.",
            result_value.description, result_value.task, result_value.task
        );

        model_state.deliver(&master, &text)?;
        "its master stops it"
    } else {
        // a foreground subagent ends with its master's turn: its other foreground ones too
        if let Some(line_3) = model_state.sessions.get_mut(&master).filter(|line_2| line_2.busy) {
            line_3.session.interrupt()?;
            line_3.interrupted = true;
        }

        let rest: Vec<String> = model_state
            .subagents
            .running
            .iter()
            .filter(|(_, x)| x.agent == master && !x.background)
            .map(|(i, _)| i.clone())
            .collect();

        for i in rest {
            model_state.subagents.stop(&i);
            model_state.log(
                Some(&master),
                Kind::SubEnd {
                    id: i,
                    state: "stopped".into(),
                    result: "its master's turn was interrupted".into(),
                },
            );
        }

        "its master's turn interrupted"
    };

    model_state.log(
        Some(&master),
        Kind::SubEnd {
            id,
            state: "stopped".into(),
            result: how.into(),
        },
    );

    Ok(json!({"stopped": how}))
}

fn command_interrupt(model_state: &mut Host) -> Result<Value, String> {
    let (mut stopped, mut errs) = (Vec::new(), Vec::new());

    for (name, live) in model_state.sessions.iter_mut().filter(|(_, line_4)| line_4.busy) {
        match live.session.interrupt() {
            Ok(()) => {
                live.interrupted = true;
                stopped.push(name.clone());
            }
            Err(error) => errs.push(format!("{name}: {error}")),
        }
    }

    // their open asks are moot: 0 answer comes (ACP: `cancel` answered them)
    model_state.asks.retain(|_, (agent_10, _)| !stopped.contains(agent_10));

    if !errs.is_empty() {
        return Err(errs.join("; "));
    }

    Ok(json!({"interrupted": stopped.len()}))
}

fn command_join(request: &Value, model_state: &mut Host) -> Result<Value, String> {
    let agent = request_text(request, "agent");

    model_state.ensure(&agent)?;
    let text = request_text(request, "text");

    if !text.trim().is_empty() {
        model_state.log(
            None,
            Kind::Prompt {
                from: "owner".into(),
                to: vec![agent.clone()],
                text: text.clone(),
            },
        );
        model_state.deliver(&agent, &text)?;
    }

    Ok(json!({}))
}

fn command_leave(request: &Value, model_state: &mut Host) -> Result<Value, String> {
    let (agent, to) = (request_text(request, "agent"), request_text(request, "to"));

    if let Some(mut line_5) = model_state.sessions.remove(&agent) {
        line_5.session.kill();
    }

    model_state.asks.retain(|_, (agent_11, _)| *agent_11 != agent);
    let flow = model_state.thread()?.flow_of(&agent);

    if let Some(file) = &flow {
        let line = if to.is_empty() {
            format!("flow {} of @{agent} ended: it left", file.title)
        } else {
            format!("flow {} of @{agent} → @{to}", file.title)
        };

        model_state.flow_log(&agent, "clear", line);
    }

    store::update(&model_state.id, |thread_3| {
        thread_3.set_flow(&agent, None);
        thread_3.agents.retain(|agent_12| *agent_12 != agent);

        if let Some(mut file_value) = flow.filter(|_| !to.is_empty()) {
            file_value.sent = false;
            file_value.pending = None;
            thread_3.set_flow(&to, Some(file_value));
        }
    })?;

    model_state.log(Some(&agent), Kind::Leave);

    Ok(json!({}))
}

fn command_reload(request: &Value, model_state: &mut Host) -> Result<Value, String> {
    // the agent's profile changed: its idle session ends, the next message starts
    // the new one (fresh: the harness changed, the conversation does not carry over)
    let agent = request_text(request, "agent");

    if model_state.sessions.get(&agent).is_some_and(|line_6| line_6.busy) {
        return Err(format!("{agent} is mid-turn"));
    }

    if let Some(mut line_7) = model_state.sessions.remove(&agent) {
        line_7.session.kill();
    }

    if request["fresh"].as_bool() == Some(true) {
        store::update(&model_state.id, |thread_4| {
            thread_4.sessions.remove(&agent);
        })?;
    }

    Ok(json!({}))
}

fn command_status(request: &Value, model_state: &mut Host) -> Result<Value, String> {
    let state = request_text(request, "state");

    if !["working", "blocked", "review", "done"].contains(&state.as_str()) {
        return Err("status: working, blocked, review or done".into());
    }

    model_state.log(
        Some(&request_text(request, "agent")),
        Kind::Status {
            state,
            note: request_text(request, "note"),
        },
    );
    Ok(json!({}))
}

fn command_stop(model_state: &mut Host) -> serde_json::Value {
    model_state.stopping = true;
    json!({})
}

fn command_sub_start(request: &Value, model_state: &mut Host) -> Result<Value, String> {
    let id = request_text(request, "id");

    if id.is_empty() {
        return Err("sub_start: id required".into());
    }

    let key_2 = id.clone();

    store::update(&model_state.id, move |thread_5| {
        if !thread_5.kids.contains(&key_2) {
            thread_5.kids.push(key_2);
        }
    })?;

    model_state.log(
        None,
        Kind::SubThread {
            id,
            title: request_text(request, "title"),
            state: "running".into(),
            text: request_text(request, "text"),
        },
    );

    Ok(json!({}))
}

fn command_sub_done(request: &Value, model_state: &mut Host) -> Result<Value, String> {
    let id = request_text(request, "id");
    let thread_6 = model_state.thread()?;

    if !thread_6.kids.contains(&id) {
        return Ok(json!({}));
    }

    let key_3 = id.clone();
    let thread_8 = store::update(&model_state.id, move |thread_7| thread_7.kids.retain(|x| *x != key_3))?;

    let state = if request_text(request, "state").is_empty() {
        "done".to_string()
    } else {
        request_text(request, "state")
    };

    let agent = request_text(request, "agent");

    model_state.log(
        Some(&agent).filter(|agent_13| !agent_13.is_empty()).map(String::as_str),
        Kind::SubThread {
            id,
            title: request_text(request, "title"),
            state,
            text: request_text(request, "text"),
        },
    );

    if thread_8.kids.is_empty() {
        let held = std::mem::take(&mut model_state.held);

        model_state.later.extend(held);
    }

    Ok(json!({}))
}
