use super::{Kind, Mutex, Out, Pending, State, TurnUse, Value, error_text, json, mcp_servers, session_model, tool_of};

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
        let text = std::mem::take(&mut self.text);

        if !text.trim().is_empty() {
            self.out(Out::Event(Kind::Text { text }));
        }
    }
    pub(crate) fn flush_thought(&mut self) {
        let thought = std::mem::take(&mut self.thought);

        if !thought.trim().is_empty() {
            self.out(Out::Event(Kind::Thought {
                text: thought.trim().to_string(),
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
            Some(preamble) if !preamble.is_empty() => format!("{preamble}\n\n---\n\n{text}"),
            _ => text,
        };

        self.busy = true;
        self.usage.turn_started();
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
                report_model_error(err.as_deref(), self);
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
        let params = &value["params"];

        match method {
            "session/update" => self.update(&params["update"]),
            "session/request_permission" => {
                handle_session_request_permission(value, params, self);
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

        let mut err = stderr.lock().map(|lines| lines.join("\n")).unwrap_or_default();

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

fn skip_preamble_on_load(why: Pending, state: &mut State) {
    if why == Pending::Load {
        state.preamble = None;
    }
}

fn report_model_error(err: Option<&str>, state: &mut State) {
    if let Some(error) = err {
        let model = state.setup.model.clone().unwrap_or_default();

        state.out(Out::Event(Kind::Error {
            message: format!("model {model}: {error}"),
        }));
    }
}

fn handle_session_request_permission(request: &Value, params: &Value, state: &mut State) {
    state.flush_text();
    let id = crate::engine::new_id();
    let options = params["options"].as_array().cloned().unwrap_or_default();
    let tc = &params["toolCall"];
    let (name, mut input) = tool_of(tc);

    if let Some(fields) = input.as_object_mut() {
        fields.insert(
            "_options".into(),
            Value::Array(
                options
                    .iter()
                    .map(|option| json!({"id": option["optionId"], "name": option["name"], "kind": option["kind"]}))
                    .collect(),
            ),
        );
        fields.insert("_kind".into(), tc["kind"].clone());
    }

    state.asks.insert(id.clone(), (request["id"].clone(), options));
    state.out(Out::Control(
        id,
        json!({"subtype": "can_use_tool", "tool_name": name, "input": input}),
    ));
}

fn request_profile_model(sid: &str, options: bool, state: &mut State) -> Result<(), String> {
    if let Some(model) = state.setup.model.clone() {
        // ACP config options (codex-acp ≥ 0.16: 0 `session/set_model`), else the older call
        if options {
            state.request(
                "session/set_config_option",
                &json!({"sessionId": sid, "configId": "model", "value": model}),
                Pending::Model,
            )?;
        } else {
            state.request(
                "session/set_model",
                &json!({"sessionId": sid, "modelId": model}),
                Pending::Model,
            )?;
        }
    }

    Ok(())
}

fn reply_prompt(err: Option<String>, result: &Value, state: &mut State) -> Result<(), String> {
    state.flush_text();
    state.busy = false;
    let usage = &result["usage"];
    let count = |key: &str| usage[key].as_u64().unwrap_or(0);
    let error = err.or_else(|| match result["stopReason"].as_str() {
        Some("refusal") => Some("the agent refused".into()),
        Some("max_tokens") => Some("hit the token limit".into()),
        _ => None,
    });

    let tokens = TurnUse {
        input: count("inputTokens"),
        output: count("outputTokens"),
        cache_read: count("cachedReadTokens"),
        cache_write: count("cachedWriteTokens"),
        ctx: state.usage.used,
    };

    // `totalTokens` is the turn's sum, not the context: the context is the last `usage_update`
    state.out(Out::Event(Kind::TurnEnd {
        cost: state.usage.turn_usd(tokens),
        input: tokens.input,
        output: tokens.output,
        cache_read: tokens.cache_read,
        cache_write: tokens.cache_write,
        ctx: tokens.ctx,
        window: state.usage.size,
        ms: 0,
        error,
    }));
    state.pump()?;

    Ok(())
}

fn initialize_session(err: Option<&str>, result: &Value, state: &mut State) -> Result<(), String> {
    if let Some(error) = err {
        return Err(format!("initialize: {error}"));
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
        if let Some(error) = err {
            return Err(format!("session/new: {error}"));
        }

        result["sessionId"].as_str().map(String::from)
    };

    let Some(sid) = sid else {
        return Err("session/new: 0 session id".into());
    };

    skip_preamble_on_load(why, state);

    let (model, options) = session_model(result);

    state.usage.session_started(why == Pending::New, &model);
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
