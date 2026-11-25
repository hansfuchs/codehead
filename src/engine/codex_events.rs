use super::{Ask, Kind, Out, State, Value, json, result_of, tool_of, usage_of};

impl State {
    pub(crate) fn notify(&mut self, method: &str, params: &Value) {
        match method {
            "turn/started" => {
                set_turn_id(params, self);
            }
            "item/agentMessage/delta" => {
                emit_text_delta(params, self);
            }
            "item/started" => {
                start_tool_item(params, self);
            }
            "item/completed" => completed(&params["item"], self),
            "thread/tokenUsage/updated" => {
                update_token_usage(params, self);
            }
            "turn/completed" => {
                handle_turn_completed(params, self);
            }
            "mcpServer/startupStatus/updated" => {
                report_mcp_status(params, self);
            }
            _ => {}
        }
    }

    /// A request of the app server: an ask, else an error reply.
    pub(crate) fn ask(&mut self, method: &str, rpc: Value, params: &Value) -> Result<(), String> {
        let id = crate::engine::new_id();
        let item = params["itemId"]
            .as_str()
            .and_then(|item_id| self.items.get(item_id))
            .cloned();

        let (kind, name, mut input) = match method {
            "item/commandExecution/requestApproval" => {
                let input = json!({"command": params["command"], "cwd": params["cwd"], "description": params["reason"], "_kind": "execute"});

                (Ask::Command, "Bash".to_string(), input)
            }
            "item/fileChange/requestApproval" => {
                let (_, mut input) = item.unwrap_or_else(|| ("Edit".into(), json!({"_kind": "edit"})));

                set_permission_reason(params, &mut input);

                (Ask::FileChange, "Edit".to_string(), input)
            }
            "mcpServer/elicitation/request" => {
                let server = params["serverName"].as_str().unwrap_or_default();

                (
                    Ask::Elicitation,
                    format!("mcp__{server}"),
                    json!({"description": params["message"], "_kind": "other"}),
                )
            }
            "item/tool/requestUserInput" => request_user_input(params),
            _ => {
                return self.send(&json!({"id": rpc, "error": {"code": -32601, "message": format!("{method}: not supported by codehead")}}));
            }
        };

        if !input.is_object() {
            input = json!({});
        }

        self.asks.insert(id.clone(), (rpc, kind));
        self.out(Out::Control(
            id,
            json!({"subtype": "can_use_tool", "tool_name": name, "input": input}),
        ));

        Ok(())
    }
}

fn set_permission_reason(params: &Value, input: &mut Value) {
    if let Some(reason) = params["reason"].as_str() {
        input["description"] = json!(reason);
    }
}

fn request_user_input(params: &Value) -> (Ask, String, Value) {
    let qs = params["questions"].as_array().cloned().unwrap_or_default();

    let ids = qs
        .iter()
        .map(|question| {
            (
                question["question"].as_str().unwrap_or_default().to_string(),
                question["id"].as_str().unwrap_or_default().to_string(),
            )
        })
        .collect();

    let questions: Vec<Value> = qs
        .iter()
        .map(|question| {
            let options: Vec<Value> = question["options"].as_array().into_iter().flatten().map(|option| json!({"label": option["label"], "description": option["description"]})).collect();

            json!({"question": question["question"], "header": question["header"], "options": options, "multiSelect": false})
        })
        .collect();

    (
        Ask::Questions(ids),
        "AskUserQuestion".to_string(),
        json!({"questions": questions}),
    )
}

fn emit_text_delta(params: &Value, state: &mut State) {
    if let Some(delta) = params["delta"].as_str().filter(|delta| !delta.is_empty()) {
        state.out(Out::Delta(delta.into()));
    }
}

fn update_token_usage(params: &Value, state: &mut State) {
    let usage = &params["tokenUsage"];

    state.total = usage_of(&usage["total"]);
    let last = usage_of(&usage["last"]);

    if last.ctx > 0 {
        state.ctx = last.ctx;
    }

    if let Some(window) = usage["modelContextWindow"].as_u64() {
        state.window = window;
    }

    if state.busy {
        state.out(Out::Usage(state.turn_use()));
    }
}

fn set_turn_id(params: &Value, state: &mut State) {
    if let Some(id) = params["turn"]["id"].as_str() {
        state.turn = Some(id.to_string());
    }
}

fn handle_turn_completed(params: &Value, state: &mut State) {
    let turn = &params["turn"];

    state.busy = false;
    state.turn = None;
    state.items.clear();

    let error = match turn["status"].as_str().unwrap_or_default() {
        "failed" => Some(turn["error"]["message"].as_str().unwrap_or("failed").to_string()),
        _ => None,
    };

    let usage = state.turn_use();

    state.out(Out::Event(Kind::TurnEnd {
        cost: 0.0,
        input: usage.input,
        output: usage.output,
        cache_read: usage.cache_read,
        cache_write: usage.cache_write,
        ctx: state.ctx,
        window: state.window,
        ms: turn["durationMs"].as_u64().unwrap_or(0),
        error,
    }));

    if let Err(error) = state.pump() {
        state.out(Out::Event(Kind::Error {
            message: format!("codex: {error}"),
        }));
    }
}

fn report_mcp_status(params: &Value, state: &mut State) {
    let status = match params["status"].as_str().unwrap_or_default() {
        "ready" => "connected",
        "starting" => return,
        text => text,
    };

    let error = params["error"]
        .as_str()
        .or(params["failureReason"].as_str())
        .unwrap_or_default()
        .to_string();
    let name = params["name"].as_str().unwrap_or_default().to_string();

    // codex_apps: the app server's own
    if name == "codex_apps" {
        return;
    }

    let source = if state.setup.config["mcp_servers"].get(&name).is_some() {
        "codehead"
    } else {
        "user"
    };

    let row = crate::engine::servers::Status {
        state: status.into(),
        error,
        source: source.into(),
        at: crate::engine::now_ms(),
        ..Default::default()
    };

    state.out(Out::Mcp("codex".into(), vec![(name, row)]));
}

/// A finished item: the agent's text, a thought, a tool result.
fn completed(item: &Value, state: &mut State) {
    match item["type"].as_str().unwrap_or_default() {
        "agentMessage" => {
            let text = item["text"].as_str().unwrap_or_default();

            if !text.trim().is_empty() {
                state.out(Out::Event(Kind::Text { text: text.into() }));
            }
        }
        "reasoning" => {
            let summary = item["summary"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|part| part.as_str())
                .collect::<Vec<_>>()
                .join("\n\n");

            if !summary.trim().is_empty() {
                state.out(Out::Event(Kind::Thought {
                    text: summary.trim().to_string(),
                }));
            }
        }
        _ => {
            let id = item["id"].as_str().unwrap_or_default().to_string();

            if state.items.remove(&id).is_some() {
                let (ok, output) = result_of(item);

                state.out(Out::Event(Kind::ToolResult { id, ok, output }));
            }
        }
    }
}

fn start_tool_item(params: &Value, state: &mut State) {
    let item = &params["item"];

    if let Some((name, input)) = tool_of(item) {
        let id = item["id"].as_str().unwrap_or_default().to_string();

        state.items.insert(id.clone(), (name.clone(), input.clone()));
        state.out(Out::Event(Kind::Tool { id, name, input }));
    }
}
