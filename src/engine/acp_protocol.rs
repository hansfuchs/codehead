use super::{Kind, Mutex, Out, Pending, State, Value, error_text, json, mcp_servers, session_model, tool_of};

impl State {
    pub(crate) fn send(&mut self, value: &Value) -> Result<(), String> {
        self.stdin
            .send(value.to_string())
            .map_err(|_| "agent stdin closed".to_string())
    }
    pub(crate) fn request(&mut self, method: &str, params: &Value, why: Pending) -> Result<(), String> {
        self.next += 1;
        let id = self.next;

        self.pending.insert(id, why);
        self.send(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}))
    }
    pub(crate) fn out(&self, output: Out) {
        let _ = self.tx.send((self.name.clone(), output));
    }
    /// The buffered thought, then the buffered text, as events (empty ones: 0 events).
    pub(crate) fn flush_text(&mut self) {
        self.flush_thought();
        let thread = std::mem::take(&mut self.text);

        if !thread.trim().is_empty() {
            self.out(Out::Event(Kind::Text { text: thread }));
        }
    }
    pub(crate) fn flush_thought(&mut self) {
        let thread = std::mem::take(&mut self.thought);

        if !thread.trim().is_empty() {
            self.out(Out::Event(Kind::Thought {
                text: thread.trim().to_string(),
            }));
        }
    }
    /// The next queued prompt, when the session is up and idle.
    pub(crate) fn pump(&mut self) -> Result<(), String> {
        if self.busy || self.session.is_none() {
            return Ok(());
        }

        let Some(text) = self.queue.pop_front() else {
            return Ok(());
        };

        let text = match self.preamble.take() {
            Some(path) if !path.is_empty() => format!("{path}\n\n---\n\n{text}"),
            _ => text,
        };

        self.busy = true;
        let sid = self.session.clone().unwrap_or_default();

        self.request(
            "session/prompt",
            &json!({"sessionId": sid, "prompt": [{"type": "text", "text": text}]}),
            Pending::Prompt,
        )
    }
    pub(crate) fn reply(&mut self, why: Pending, value: &Value) -> Result<(), String> {
        let err = value.get("error").map(error_text);
        let result = &value["result"];

        match why {
            Pending::Init => {
                initialize_session(err.as_deref(), result, self)?;
            }
            Pending::New | Pending::Load => {
                establish_session(why, err.as_deref(), result, self)?;
            }
            // the model the agent's profile names: unknown to the harness → a note in the stream
            Pending::Model => {
                handle_model(err.as_deref(), self);
            }
            Pending::Prompt => {
                reply_prompt(err, result, self)?;
            }
        }

        Ok(())
    }

    pub(crate) fn incoming(&mut self, value: &Value) -> Result<(), String> {
        // a reply to 1 of our requests
        if value.get("method").is_none() {
            return receive_reply(value, self);
        }

        let method = value["method"].as_str().unwrap_or_default();
        let path = &value["params"];

        match method {
            "session/update" => self.update(&path["update"]),
            "session/request_permission" => {
                handle_session_request_permission(value, path, self);
            }
            // 0 client capabilities advertised: anything else is an error reply
            _ if value.get("id").is_some() => {
                let id = value["id"].clone();

                self.send(&json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32601, "message": format!("{method}: not supported by codehead")}}))?;
            }
            _ => {}
        }

        Ok(())
    }
}
impl State {
    /// End the session once: the stderr tail, then WHY. The host kills the process.
    pub(crate) fn exit(&mut self, stderr: &Mutex<Vec<String>>, why: &str) {
        if std::mem::replace(&mut self.dead, true) {
            return;
        }

        let mut err = stderr.lock().map(|thread| thread.join("\n")).unwrap_or_default();

        if !why.is_empty() {
            separate_stderr(&mut err);

            err.push_str(why);
        }

        self.out(Out::Exit(err));
    }
}

fn separate_stderr(err: &mut String) {
    if !err.is_empty() {
        err.push('\n');
    }
}

fn finish_session_load(why: Pending, model_state: &mut State) {
    if why == Pending::Load {
        model_state.preamble = None;
    }
}

fn handle_model(err: Option<&str>, model_state: &mut State) {
    if let Some(error_3) = err {
        let message_2 = model_state.setup.model.clone().unwrap_or_default();

        model_state.out(Out::Event(Kind::Error {
            message: format!("model {message_2}: {error_3}"),
        }));
    }
}

fn handle_session_request_permission(value: &Value, path: &Value, model_state: &mut State) {
    model_state.flush_text();
    let id = crate::engine::new_id();
    let options = path["options"].as_array().cloned().unwrap_or_default();
    let tc = &path["toolCall"];
    let (name, mut input) = tool_of(tc);

    if let Some(output) = input.as_object_mut() {
        output.insert(
            "_options".into(),
            Value::Array(
                options
                    .iter()
                    .map(|output_value| json!({"id": output_value["optionId"], "name": output_value["name"], "kind": output_value["kind"]}))
                    .collect(),
            ),
        );
        output.insert("_kind".into(), tc["kind"].clone());
    }

    model_state.asks.insert(id.clone(), (value["id"].clone(), options));
    model_state.out(Out::Control(
        id,
        json!({"subtype": "can_use_tool", "tool_name": name, "input": input}),
    ));
}

fn request_profile_model(sid: &str, options: bool, model_state: &mut State) -> Result<(), String> {
    if let Some(message_value) = model_state.setup.model.clone() {
        // ACP config options (codex-acp ≥ 0.16: 0 `session/set_model`), else the older call
        if options {
            model_state.request(
                "session/set_config_option",
                &json!({"sessionId": sid, "configId": "model", "value": message_value}),
                Pending::Model,
            )?;
        } else {
            model_state.request(
                "session/set_model",
                &json!({"sessionId": sid, "modelId": message_value}),
                Pending::Model,
            )?;
        }
    }

    Ok(())
}

fn reply_prompt(err: Option<String>, result: &Value, model_state: &mut State) -> Result<(), String> {
    model_state.flush_text();
    model_state.busy = false;
    let user = &result["usage"];
    let count = |key: &str| user[key].as_u64().unwrap_or(0);
    let error = err.or_else(|| match result["stopReason"].as_str() {
        Some("refusal") => Some("the agent refused".into()),
        Some("max_tokens") => Some("hit the token limit".into()),
        _ => None,
    });

    model_state.out(Out::Event(Kind::TurnEnd {
        cost: 0.0,
        input: count("inputTokens"),
        output: count("outputTokens"),
        cache_read: count("cachedReadTokens"),
        cache_write: count("cachedWriteTokens"),
        ctx: count("totalTokens"),
        window: 0,
        ms: 0,
        error,
    }));
    model_state.pump()?;

    Ok(())
}

fn initialize_session(err: Option<&str>, result: &Value, state: &mut State) -> Result<(), String> {
    if let Some(error_value) = err {
        return Err(format!("initialize: {error_value}"));
    }

    state.capabilities.load_session = result["agentCapabilities"]["loadSession"].as_bool().unwrap_or(false);
    let caps = &result["agentCapabilities"]["mcpCapabilities"];
    let (mcp, errors) = mcp_servers(
        &state.setup,
        caps["http"].as_bool().unwrap_or(false),
        caps["sse"].as_bool().unwrap_or(false),
    );

    crate::engine::harness::report_mcp_errors(&state.setup.harness, &errors, &state.tx, &state.name);
    state.capabilities.mcp = mcp.clone();
    let cwd = state.setup.cwd.clone();

    match state.setup.resume.clone().filter(|_| state.capabilities.load_session) {
        Some(sid) => {
            state.loading = true;
            state.session = None;
            state.request(
                "session/load",
                &json!({"sessionId": sid, "cwd": cwd, "mcpServers": mcp}),
                Pending::Load,
            )?;

            // the id stays the same: kept for the reply
            state.setup.resume = Some(sid);
        }
        None => state.request("session/new", &json!({"cwd": cwd, "mcpServers": mcp}), Pending::New)?,
    }

    Ok(())
}

fn establish_session(why: Pending, err: Option<&str>, result: &Value, state: &mut State) -> Result<(), String> {
    state.loading = false;

    let sid = if why == Pending::Load && err.is_none() {
        state.setup.resume.clone()
    } else if why == Pending::Load {
        // the old session is gone: a new one
        let cwd = state.setup.cwd.clone();
        let mcp = state.capabilities.mcp.clone();

        return state.request("session/new", &json!({"cwd": cwd, "mcpServers": mcp}), Pending::New);
    } else {
        if let Some(error_2) = err {
            return Err(format!("session/new: {error_2}"));
        }

        result["sessionId"].as_str().map(String::from)
    };

    let Some(sid) = sid else {
        return Err("session/new: 0 session id".into());
    };

    finish_session_load(why, state);

    let (model, options) = session_model(result);

    state.out(Out::Event(Kind::Session {
        session: sid.clone(),
        model,
    }));
    state.session = Some(sid.clone());

    request_profile_model(&sid, options, state)?;

    state.pump()
}

fn receive_reply(value: &Value, state: &mut State) -> Result<(), String> {
    if let Some(id) = value["id"].as_u64()
        && let Some(why) = state.pending.remove(&id)
    {
        return state.reply(why, value);
    }

    Ok(())
}
