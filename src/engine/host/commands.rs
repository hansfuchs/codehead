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
    if let (Some(fields), Some(option)) = (input.as_object_mut(), request["option"].as_str()) {
        fields.insert("_option".into(), Value::String(option.into()));
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

fn command_ping(host: &mut Host) -> serde_json::Value {
    json!({
        "live": host.sessions.keys().collect::<Vec<_>>(),
        // busy: in a turn, or waiting on running subagents
        "busy": host.sessions.iter().filter(|(name, live)| live.busy || host.subagents.masters().contains(*name)).map(|(name, _)| name).collect::<Vec<_>>(),
        "asks": host.asks.len(),
        "usage": host.turn_use,
    })
}

fn command_prompt(request: &Value, host: &mut Host) -> Result<Value, String> {
    let text = request_text(request, "text");

    if text.trim().is_empty() {
        return Err("empty message".into());
    }

    let thread = host.thread()?;
    let mut to = request_list(request, "to");

    if to.is_empty() {
        let names: Vec<String> = roster::active().into_iter().map(|profile| profile.name).collect();

        to = mentions(&text, &names);
    }

    if to.is_empty() {
        to = thread
            .lead()
            .map(|lead| vec![lead.to_string()])
            .ok_or("thread has 0 agents")?;
    }

    host.log(
        None,
        Kind::Prompt {
            from: "owner".into(),
            to: to.clone(),
            text: text.clone(),
        },
    );

    for agent in &to {
        host.cancel_resume(agent);
    }

    if thread.state != "open" {
        store::update(&host.id, |stored| stored.state = "open".into())?;
    }

    // CONTEXT (the owner's shell commands since the last message): for the agents only
    let ctx = request_text(request, "context");

    let full = if ctx.trim().is_empty() {
        text.clone()
    } else {
        format!("{text}\n\n{ctx}")
    };

    // a flow whose current step went to 0 agent yet: the step goes with this message
    let steps = host.flow_unsent(&thread, &to, &full);

    host.later.extend(
        to.iter()
            .map(|agent| match steps.iter().find(|(step_agent, _)| step_agent == agent) {
                Some((_, message)) => (agent.clone(), message.clone()),
                None => (agent.clone(), full.clone()),
            }),
    );

    Ok(json!({"to": to}))
}

fn command_send(request: &Value, host: &mut Host) -> Result<Value, String> {
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

    for agent in &to {
        roster::get(agent)
            .filter(|profile| !profile.archived)
            .ok_or_else(|| format!("no agent {agent}"))?;
    }

    host.log(
        Some(&from),
        Kind::Prompt {
            from: from.clone(),
            to: to.clone(),
            text: text.clone(),
        },
    );

    for agent in &to {
        host.cancel_resume(agent);
    }

    for agent in &to {
        host.deliver(agent, &format!("[from @{from}] {text}"))?;
    }

    Ok(json!({"to": to}))
}

fn command_answer(request: &Value, host: &mut Host) -> Result<Value, String> {
    let id = request_text(request, "id");
    let (agent, ask) = host.asks.get(&id).cloned().ok_or_else(|| format!("no open ask {id}"))?;

    let Some(live) = host.sessions.get_mut(&agent) else {
        host.asks.remove(&id);
        return Err(format!("{agent}: session gone"));
    };

    let allow = request["allow"].as_bool().unwrap_or(false);

    // an ACP option picks itself, allow or reject
    if allow || request["option"].is_string() {
        let mut input = ask["input"].clone();

        set_answer_option(request, &mut input);

        if let (Some(fields), Some(answers)) = (
            input.as_object_mut(),
            request.get("answers").filter(|value| value.is_object()),
        ) {
            fields.insert("answers".into(), answers.clone());
        }

        live.session.allow(&id, &input)?;
    } else {
        let why = request["note"]
            .as_str()
            .filter(|note| !note.is_empty())
            .unwrap_or("The owner said no.");

        live.session.deny(&id, why)?;
    }

    // answered: the ask goes only now (a failed write keeps it open for a retry)
    host.asks.remove(&id);
    host.log(
        Some(&agent),
        Kind::Answer {
            id,
            allow,
            label: request_text(request, "label"),
        },
    );

    Ok(json!({}))
}

fn command_sub_stop(request: &Value, host: &mut Host) -> Result<Value, String> {
    let id = request_text(request, "id");
    let subagent = host
        .subagents
        .stop(&id)
        .ok_or_else(|| format!("subagent {id}: 0 running"))?;
    let master = subagent.agent.clone();

    let how = if subagent.background && !subagent.task.is_empty() {
        let text = format!(
            "[codehead] The owner stopped your background subagent “{}” (task id {}). Stop it now with TaskStop (task_id: {}) and go on without its result.",
            subagent.description, subagent.task, subagent.task
        );

        host.deliver(&master, &text)?;
        "its master stops it"
    } else {
        // a foreground subagent ends with its master's turn: its other foreground ones too
        if let Some(live) = host.sessions.get_mut(&master).filter(|live| live.busy) {
            live.session.interrupt()?;
            live.interrupted = true;
        }

        let rest: Vec<String> = host
            .subagents
            .running
            .iter()
            .filter(|(_, running)| running.agent == master && !running.background)
            .map(|(other, _)| other.clone())
            .collect();

        for other in rest {
            host.subagents.stop(&other);
            host.log(
                Some(&master),
                Kind::SubEnd {
                    id: other,
                    state: "stopped".into(),
                    result: "its master's turn was interrupted".into(),
                },
            );
        }

        "its master's turn interrupted"
    };

    host.log(
        Some(&master),
        Kind::SubEnd {
            id,
            state: "stopped".into(),
            result: how.into(),
        },
    );

    Ok(json!({"stopped": how}))
}

fn command_interrupt(host: &mut Host) -> Result<Value, String> {
    let (mut stopped, mut errs) = (Vec::new(), Vec::new());

    for (name, live) in host.sessions.iter_mut().filter(|(_, live)| live.busy) {
        match live.session.interrupt() {
            Ok(()) => {
                live.interrupted = true;
                stopped.push(name.clone());
            }
            Err(error) => errs.push(format!("{name}: {error}")),
        }
    }

    // their open asks are moot: 0 answer comes (ACP: `cancel` answered them)
    host.asks.retain(|_, (agent, _)| !stopped.contains(agent));

    if !errs.is_empty() {
        return Err(errs.join("; "));
    }

    Ok(json!({"interrupted": stopped.len()}))
}

fn command_join(request: &Value, host: &mut Host) -> Result<Value, String> {
    let agent = request_text(request, "agent");

    host.ensure(&agent)?;
    let text = request_text(request, "text");

    if !text.trim().is_empty() {
        host.log(
            None,
            Kind::Prompt {
                from: "owner".into(),
                to: vec![agent.clone()],
                text: text.clone(),
            },
        );
        host.deliver(&agent, &text)?;
    }

    Ok(json!({}))
}

fn command_leave(request: &Value, host: &mut Host) -> Result<Value, String> {
    let (agent, to) = (request_text(request, "agent"), request_text(request, "to"));

    if let Some(mut live) = host.sessions.remove(&agent) {
        live.session.kill();
    }

    host.asks.retain(|_, (asker, _)| *asker != agent);
    let flow = host.thread()?.flow_of(&agent);

    if let Some(current) = &flow {
        let line = if to.is_empty() {
            format!("flow {} of @{agent} ended: it left", current.title)
        } else {
            format!("flow {} of @{agent} → @{to}", current.title)
        };

        host.flow_log(&agent, "clear", line);
    }

    store::update(&host.id, |thread| {
        thread.set_flow(&agent, None);
        thread.agents.retain(|member| *member != agent);

        if let Some(mut moved) = flow.filter(|_| !to.is_empty()) {
            moved.sent = false;
            moved.pending = None;
            thread.set_flow(&to, Some(moved));
        }
    })?;

    host.log(Some(&agent), Kind::Leave);

    Ok(json!({}))
}

fn command_reload(request: &Value, host: &mut Host) -> Result<Value, String> {
    // the agent's profile changed: its idle session ends, the next message starts
    // the new one (fresh: the harness changed, the conversation does not carry over)
    let agent = request_text(request, "agent");

    if host.sessions.get(&agent).is_some_and(|live| live.busy) {
        return Err(format!("{agent} is mid-turn"));
    }

    if let Some(mut live) = host.sessions.remove(&agent) {
        live.session.kill();
    }

    if request["fresh"].as_bool() == Some(true) {
        store::update(&host.id, |thread| {
            thread.sessions.remove(&agent);
        })?;
    }

    Ok(json!({}))
}

fn command_status(request: &Value, host: &mut Host) -> Result<Value, String> {
    let state = request_text(request, "state");

    if !["working", "blocked", "review", "done"].contains(&state.as_str()) {
        return Err("status: working, blocked, review or done".into());
    }

    host.log(
        Some(&request_text(request, "agent")),
        Kind::Status {
            state,
            note: request_text(request, "note"),
        },
    );
    Ok(json!({}))
}

fn command_stop(host: &mut Host) -> serde_json::Value {
    host.stopping = true;
    json!({})
}

fn command_sub_start(request: &Value, host: &mut Host) -> Result<Value, String> {
    let id = request_text(request, "id");

    if id.is_empty() {
        return Err("sub_start: id required".into());
    }

    let kid = id.clone();

    store::update(&host.id, move |thread| {
        if !thread.kids.contains(&kid) {
            thread.kids.push(kid);
        }
    })?;

    host.log(
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

fn command_sub_done(request: &Value, host: &mut Host) -> Result<Value, String> {
    let id = request_text(request, "id");
    let thread = host.thread()?;

    if !thread.kids.contains(&id) {
        return Ok(json!({}));
    }

    let kid = id.clone();
    let updated = store::update(&host.id, move |stored| stored.kids.retain(|other| *other != kid))?;

    let state = if request_text(request, "state").is_empty() {
        "done".to_string()
    } else {
        request_text(request, "state")
    };

    let agent = request_text(request, "agent");

    host.log(
        Some(&agent).filter(|name| !name.is_empty()).map(String::as_str),
        Kind::SubThread {
            id,
            title: request_text(request, "title"),
            state,
            text: request_text(request, "text"),
        },
    );

    if updated.kids.is_empty() {
        let held = std::mem::take(&mut host.held);

        host.later.extend(held);
    }

    Ok(json!({}))
}
