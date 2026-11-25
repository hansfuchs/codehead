use super::{Ask, Kind, Mutex, Out, Pending, State, TurnUse, Value, error_text, json};

impl State {
    pub(crate) fn send(&mut self, value: &Value) -> Result<(), String> {
        self.stdin
            .send(value.to_string())
            .map_err(|_| "codex stdin closed".to_string())
    }
    pub(crate) fn request(&mut self, method: &str, params: &Value, why: Pending) -> Result<(), String> {
        self.next += 1;
        let id = self.next;

        self.pending.insert(id, why);
        self.send(&json!({"id": id, "method": method, "params": params}))
    }
    pub(crate) fn out(&self, output: Out) {
        let _ = self.tx.send((self.name.clone(), output));
    }
    /// The running turn's tokens: the thread total since the turn's start.
    pub(crate) fn turn_use(&self) -> TurnUse {
        TurnUse {
            input: self.total.input.saturating_sub(self.base.input),
            output: self.total.output.saturating_sub(self.base.output),
            cache_read: self.total.cache_read.saturating_sub(self.base.cache_read),
            cache_write: self.total.cache_write.saturating_sub(self.base.cache_write),
            ctx: self.ctx,
        }
    }
    /// The next queued prompt, when the thread is up and idle.
    pub(crate) fn pump(&mut self) -> Result<(), String> {
        if self.busy {
            return Ok(());
        }

        let Some(tid) = self.thread.clone() else { return Ok(()) };

        let Some(text) = self.queue.pop_front() else {
            return Ok(());
        };

        self.busy = true;
        self.base = self.total;

        let mut path = json!({"threadId": tid, "input": [{"type": "text", "text": text, "text_elements": []}]});

        if let Some(error) = &self.setup.effort {
            path["effort"] = json!(error);
        }

        self.request("turn/start", &path, Pending::Turn)
    }
    pub(crate) fn reply(&mut self, why: Pending, value: &Value) -> Result<(), String> {
        let err = value.get("error").map(error_text);
        let result = &value["result"];

        match why {
            Pending::Init => {
                initialize_thread(err.as_deref(), self)?;
            }
            Pending::Start | Pending::Resume => {
                establish_thread(why, err.as_deref(), result, self)?;
            }
            Pending::Turn => {
                handle_turn(err.as_deref(), result, self)?;
            }
            Pending::Interrupt => {
                report_interrupt_error(err, self);
            }
        }

        Ok(())
    }

    pub(crate) fn incoming(&mut self, value: &Value) -> Result<(), String> {
        let method = value["method"].as_str();

        let Some(method) = method else {
            // a reply to 1 of our requests
            return receive_reply(value, self);
        };

        if let Some(rpc) = value.get("id") {
            self.ask(method, rpc.clone(), &value["params"])
        } else {
            self.notify(method, &value["params"]);
            Ok(())
        }
    }
    /// The answer to ask ID: ALLOW, with INPUT (a question's `answers`: text → answer).
    pub(crate) fn answer(&mut self, id: &str, allow: bool, input: &Value) -> Result<(), String> {
        let (rpc, kind) = self.asks.remove(id).ok_or_else(|| format!("no open ask {id}"))?;

        let result = match kind {
            Ask::Command | Ask::FileChange => json!({"decision": if allow { "accept" } else { "decline" }}),
            Ask::Elicitation if allow => json!({"action": "accept", "content": {}}),
            Ask::Elicitation => json!({"action": "decline"}),
            Ask::Questions(ids) => answer_ask_questions_ids(allow, input, ids),
        };

        self.send(&json!({"id": rpc, "result": result}))
    }
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

fn answer_ask_questions_ids(allow: bool, input: &Value, ids: Vec<(String, String)>) -> Value {
    let mut answers = serde_json::Map::new();

    for (text, qid) in ids {
        if let Some(agent) = input["answers"][&text].as_str().filter(|_| allow) {
            answers.insert(qid, json!({"answers": [agent]}));
        }
    }

    json!({"answers": answers})
}

fn separate_stderr(err: &mut String) {
    if !err.is_empty() {
        err.push('\n');
    }
}

/// Params of `thread/start`, `thread/resume`, `thread/fork`.
fn thread_params(model_state: &State) -> Value {
    let mut path = json!({
        "cwd": model_state.setup.cwd,
        "approvalPolicy": "untrusted",
        "sandbox": model_state.setup.sandbox,
        "developerInstructions": model_state.setup.instructions,
        "config": model_state.setup.config,
    });

    if let Some(message) = &model_state.setup.model {
        path["model"] = json!(message);
    }

    path
}

fn report_interrupt_error(err: Option<String>, model_state: &mut State) {
    if let Some(error_4) = err {
        model_state.out(Out::Event(Kind::Error {
            message: format!("interrupt: {error_4}"),
        }));
    }
}

fn handle_turn(err: Option<&str>, result: &Value, model_state: &mut State) -> Result<(), String> {
    if let Some(error_3) = err {
        // 0 turn: the turn end comes from here
        model_state.busy = false;
        model_state.turn = None;
        model_state.out(Out::Event(Kind::TurnEnd {
            cost: 0.0,
            input: 0,
            output: 0,
            cache_read: 0,
            cache_write: 0,
            ctx: model_state.ctx,
            window: model_state.window,
            ms: 0,
            error: Some(error_3.to_string()),
        }));

        model_state.pump()?;
    } else if let Some(id) = result["turn"]["id"].as_str() {
        model_state.turn = Some(id.to_string());
    }

    Ok(())
}

fn request_thread_resume(mut path: Value, tid: &str, model_state: &mut State) -> Result<(), String> {
    path["threadId"] = json!(tid);

    let method = if model_state.setup.fork {
        "thread/fork"
    } else {
        "thread/resume"
    };

    model_state.request(method, &path, Pending::Resume)?;
    Ok(())
}

fn initialize_thread(err: Option<&str>, state: &mut State) -> Result<(), String> {
    if let Some(error_value) = err {
        return Err(format!("initialize: {error_value}"));
    }

    state.send(&json!({"method": "initialized", "params": {}}))?;
    let path = thread_params(state);

    match state.setup.resume.clone() {
        Some(tid) => {
            request_thread_resume(path, &tid, state)?;
        }
        None => state.request("thread/start", &path, Pending::Start)?,
    }

    Ok(())
}

fn establish_thread(why: Pending, err: Option<&str>, result: &Value, state: &mut State) -> Result<(), String> {
    if let Some(error_2) = err {
        // the old thread is gone: a new one
        if why == Pending::Resume {
            state.setup.resume = None;
            let path_value = thread_params(state);

            return state.request("thread/start", &path_value, Pending::Start);
        }

        return Err(format!("thread/start: {error_2}"));
    }

    let Some(tid) = result["thread"]["id"].as_str().map(String::from) else {
        return Err("thread/start: 0 thread id".into());
    };

    let model = result["model"]
        .as_str()
        .or(result["thread"]["model"].as_str())
        .unwrap_or_default()
        .to_string();

    state.out(Out::Event(Kind::Session {
        session: tid.clone(),
        model,
    }));
    state.thread = Some(tid);

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
