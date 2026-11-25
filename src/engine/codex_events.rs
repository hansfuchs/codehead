use super::{Ask, Kind, Out, State, Value, json, result_of, tool_of, usage_of};

impl State {
    pub(crate) fn notify(&mut self, method: &str, path: &Value) {
        match method {
            "turn/started" => {
                set_turn_identifier(path, self);
            }
            "item/agentMessage/delta" => {
                set_thread_identifier(path, self);
            }
            "item/started" => {
                notify_item_started(path, self);
            }
            "item/completed" => completed(&path["item"], self),
            "thread/tokenUsage/updated" => {
                notify_thread_tokenusage_updated(path, self);
            }
            "turn/completed" => {
                handle_turn_completed(path, self);
            }
            "mcpServer/startupStatus/updated" => {
                handle_mcpserver_startupstatus_updated(path, self);
            }
            _ => {}
        }
    }

    /// A request of the app server: an ask, else an error reply.
    pub(crate) fn ask(&mut self, method: &str, rpc: Value, path: &Value) -> Result<(), String> {
        let id = crate::engine::new_id();
        let item = path["itemId"].as_str().and_then(|i| self.items.get(i)).cloned();

        let (kind, name, mut input) = match method {
            "item/commandExecution/requestApproval" => {
                let input = json!({"command": path["command"], "cwd": path["cwd"], "description": path["reason"], "_kind": "execute"});

                (Ask::Command, "Bash".to_string(), input)
            }
            "item/fileChange/requestApproval" => {
                let (_, mut input) = item.unwrap_or_else(|| ("Edit".into(), json!({"_kind": "edit"})));

                set_permission_reason(path, &mut input);

                (Ask::FileChange, "Edit".to_string(), input)
            }
            "mcpServer/elicitation/request" => {
                let server = path["serverName"].as_str().unwrap_or_default();

                (
                    Ask::Elicitation,
                    format!("mcp__{server}"),
                    json!({"description": path["message"], "_kind": "other"}),
                )
            }
            "item/tool/requestUserInput" => request_user_input(path),
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

fn set_permission_reason(path: &Value, input: &mut Value) {
    if let Some(result) = path["reason"].as_str() {
        input["description"] = json!(result);
    }
}

fn request_user_input(path: &Value) -> (Ask, String, Value) {
    let qs = path["questions"].as_array().cloned().unwrap_or_default();

    let ids = qs
        .iter()
        .map(|query| {
            (
                query["question"].as_str().unwrap_or_default().to_string(),
                query["id"].as_str().unwrap_or_default().to_string(),
            )
        })
        .collect();

    let questions: Vec<Value> = qs
        .iter()
        .map(|query_value| {
            let options: Vec<Value> = query_value["options"].as_array().into_iter().flatten().map(|output| json!({"label": output["label"], "description": output["description"]})).collect();

            json!({"question": query_value["question"], "header": query_value["header"], "options": options, "multiSelect": false})
        })
        .collect();

    (
        Ask::Questions(ids),
        "AskUserQuestion".to_string(),
        json!({"questions": questions}),
    )
}

fn set_thread_identifier(path: &Value, model_state: &mut State) {
    if let Some(thread_value) = path["delta"].as_str().filter(|thread| !thread.is_empty()) {
        model_state.out(Out::Delta(thread_value.into()));
    }
}

fn notify_thread_tokenusage_updated(path: &Value, model_state: &mut State) {
    let user = &path["tokenUsage"];

    model_state.total = usage_of(&user["total"]);
    let last = usage_of(&user["last"]);

    if last.ctx > 0 {
        model_state.ctx = last.ctx;
    }

    if let Some(width) = user["modelContextWindow"].as_u64() {
        model_state.window = width;
    }

    if model_state.busy {
        model_state.out(Out::Usage(model_state.turn_use()));
    }
}

fn set_turn_identifier(path: &Value, model_state: &mut State) {
    if let Some(id) = path["turn"]["id"].as_str() {
        model_state.turn = Some(id.to_string());
    }
}

fn handle_turn_completed(path: &Value, model_state: &mut State) {
    let thread_2 = &path["turn"];

    model_state.busy = false;
    model_state.turn = None;
    model_state.items.clear();

    let error = match thread_2["status"].as_str().unwrap_or_default() {
        "failed" => Some(thread_2["error"]["message"].as_str().unwrap_or("failed").to_string()),
        _ => None,
    };

    let user_value = model_state.turn_use();

    model_state.out(Out::Event(Kind::TurnEnd {
        cost: 0.0,
        input: user_value.input,
        output: user_value.output,
        cache_read: user_value.cache_read,
        cache_write: user_value.cache_write,
        ctx: model_state.ctx,
        window: model_state.window,
        ms: thread_2["durationMs"].as_u64().unwrap_or(0),
        error,
    }));

    if let Err(error_value) = model_state.pump() {
        model_state.out(Out::Event(Kind::Error {
            message: format!("codex: {error_value}"),
        }));
    }
}

fn handle_mcpserver_startupstatus_updated(path: &Value, model_state: &mut State) {
    let state = match path["status"].as_str().unwrap_or_default() {
        "ready" => "connected",
        "starting" => return,
        text => text,
    };

    let error = path["error"]
        .as_str()
        .or(path["failureReason"].as_str())
        .unwrap_or_default()
        .to_string();
    let name = path["name"].as_str().unwrap_or_default().to_string();

    if name == "codex_apps" {
        return;
    }

    let source = if model_state.setup.config["mcp_servers"].get(&name).is_some() {
        "codehead"
    } else {
        "user"
    };

    let row = crate::engine::servers::Status {
        state: state.into(),
        error,
        source: source.into(),
        at: crate::engine::now_ms(),
        ..Default::default()
    };

    model_state.out(Out::Mcp("codex".into(), vec![(name, row)]));

    // codex_apps: the app server's own
}

/// A finished item: the agent's text, a thought, a tool result.
fn completed(item: &Value, model_state: &mut State) {
    match item["type"].as_str().unwrap_or_default() {
        "agentMessage" => {
            let thread = item["text"].as_str().unwrap_or_default();

            if !thread.trim().is_empty() {
                model_state.out(Out::Event(Kind::Text { text: thread.into() }));
            }
        }
        "reasoning" => {
            let thread_value = item["summary"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|text_value| text_value.as_str())
                .collect::<Vec<_>>()
                .join("\n\n");

            if !thread_value.trim().is_empty() {
                model_state.out(Out::Event(Kind::Thought {
                    text: thread_value.trim().to_string(),
                }));
            }
        }
        _ => {
            let id = item["id"].as_str().unwrap_or_default().to_string();

            if model_state.items.remove(&id).is_some() {
                let (ok, output) = result_of(item);

                model_state.out(Out::Event(Kind::ToolResult { id, ok, output }));
            }
        }
    }
}

fn notify_item_started(path: &Value, model_state: &mut State) {
    let item = &path["item"];

    if let Some((name, input)) = tool_of(item) {
        let id = item["id"].as_str().unwrap_or_default().to_string();

        model_state.items.insert(id.clone(), (name.clone(), input.clone()));
        model_state.out(Out::Event(Kind::Tool { id, name, input }));
    }
}
