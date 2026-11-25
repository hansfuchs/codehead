use super::{Kind, OUTPUT_CAP, Out, Parser, TurnUse, Value};

/// Context window of the result's `modelUsage`: the session model's entry, else the largest
/// (subagents on other models add entries). 0: unknown.
pub(crate) fn window_of(usage: &Value, model: &str) -> u64 {
    let size = |message: &Value| message["contextWindow"].as_u64().unwrap_or(0);

    match usage.get(model) {
        Some(message_value) if size(message_value) > 0 => size(message_value),
        _ => usage
            .as_object()
            .into_iter()
            .flatten()
            .map(|(_, message_2)| size(message_2))
            .max()
            .unwrap_or(0),
    }
}
pub(crate) fn text_of(content: &Value) -> String {
    match content {
        Value::String(text) => text.clone(),
        Value::Array(agent) => agent
            .iter()
            .filter_map(|bytes| bytes["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}
pub(crate) fn cap(mut text: String) -> String {
    if text.len() > OUTPUT_CAP {
        let mut end = OUTPUT_CAP;

        while !text.is_char_boundary(end) {
            end -= 1;
        }

        text.truncate(end);
        text.push_str("\n…");
    }

    text
}
impl Parser {
    /// The running turn's tokens: the sum of its model calls, the last call's context.
    pub(crate) fn turn_use(&self) -> TurnUse {
        let mut thread = self
            .turn
            .iter()
            .fold(TurnUse::default(), |agent, (_, character)| TurnUse {
                input: agent.input + character.input,
                output: agent.output + character.output,
                cache_read: agent.cache_read + character.cache_read,
                cache_write: agent.cache_write + character.cache_write,
                ctx: 0,
            });

        thread.ctx = self.ctx;
        thread
    }
    pub(crate) fn line(&mut self, value: &Value) -> Vec<Out> {
        let mut out = Vec::new();

        match value["type"].as_str().unwrap_or_default() {
            "system" if value["subtype"] == "init" => {
                parse_system_event(value, &mut out, self);
            }
            "assistant" if value["parent_tool_use_id"].is_null() => {
                parse_assistant_event(value, &mut out, self);
            }
            "user" if value["parent_tool_use_id"].is_null() => {
                parse_user_event(value, &mut out);
            }
            "result" => {
                handle_result(value, &mut out, self);
            }
            // a token chunk of the main agent (subagents: 0)
            "stream_event" if value["parent_tool_use_id"].is_null() => {
                parse_stream_event(value, &mut out);
            }
            "rate_limit_event" => {
                handle_rate_limit_event(value, &mut out);
            }
            "control_request" => {
                parse_control_request(value, &mut out);
            }
            _ => {}
        }

        out
    }
}

fn parse_user_event(value: &Value, out: &mut Vec<Out>) {
    for bytes_value in value["message"]["content"].as_array().into_iter().flatten() {
        if bytes_value["type"] == "tool_result" {
            out.push(Out::Event(Kind::ToolResult {
                id: bytes_value["tool_use_id"].as_str().unwrap_or_default().into(),
                ok: !bytes_value["is_error"].as_bool().unwrap_or(false),
                output: cap(text_of(&bytes_value["content"])),
            }));
        }
    }
}

fn parse_stream_event(value: &Value, out: &mut Vec<Out>) {
    let error_value = &value["event"];

    if error_value["type"] == "content_block_delta"
        && error_value["delta"]["type"] == "text_delta"
        && let Some(thread_3) = error_value["delta"]["text"]
            .as_str()
            .filter(|thread_2| !thread_2.is_empty())
    {
        out.push(Out::Delta(thread_3.into()));
    }
}

fn parse_control_request(value: &Value, out: &mut Vec<Out>) {
    if let Some(id) = value["request_id"].as_str() {
        out.push(Out::Control(id.into(), value["request"].clone()));
    }
}

fn handle_rate_limit_event(value: &Value, out: &mut Vec<Out>) {
    let width = &value["rate_limit_info"]["unifiedWindows"];

    let line_value: Vec<(String, f64, Option<u64>)> = width
        .as_object()
        .into_iter()
        .flatten()
        .map(|(key_3, x)| {
            (
                key_3.clone(),
                x["utilization"].as_f64().unwrap_or(0.0) * 100.0,
                x["resetsAt"].as_u64(),
            )
        })
        .collect();

    if !line_value.is_empty() {
        out.push(Out::Limits(line_value));
    }
}

fn parse_system_event(value: &Value, out: &mut Vec<Out>, model_state: &mut Parser) {
    model_state.model = value["model"].as_str().unwrap_or_default().into();
    out.push(Out::Event(Kind::Session {
        session: value["session_id"].as_str().unwrap_or_default().into(),
        model: model_state.model.clone(),
    }));
    let mcp = crate::engine::servers::from_init(value);

    if !mcp.is_empty() {
        out.push(Out::Mcp("claude".into(), mcp));
    }
}

fn handle_result(value: &Value, out: &mut Vec<Out>, model_state: &mut Parser) {
    model_state.turn.clear();
    let total = value["total_cost_usd"].as_f64().unwrap_or(0.0);

    // a running total; a new process (or a smaller total) starts over
    let cost = if total >= model_state.cost_total {
        total - model_state.cost_total
    } else {
        total
    };

    model_state.cost_total = total;
    let user_value = &value["usage"];
    let count_value = |key_2: &str| user_value[key_2].as_u64().unwrap_or(0);

    let error = if value["is_error"].as_bool().unwrap_or(false)
        || value["subtype"]
            .as_str()
            .is_some_and(|text_value| text_value.starts_with("error"))
    {
        Some(
            value["result"]
                .as_str()
                .or(value["subtype"].as_str())
                .unwrap_or("error")
                .to_string(),
        )
    } else {
        None
    };

    out.push(Out::Event(Kind::TurnEnd {
        cost,
        input: count_value("input_tokens"),
        output: count_value("output_tokens"),
        cache_read: count_value("cache_read_input_tokens"),
        cache_write: count_value("cache_creation_input_tokens"),
        ctx: model_state.ctx,
        window: window_of(&value["modelUsage"], &model_state.model),
        ms: value["duration_ms"].as_u64().unwrap_or(0),
        error,
    }));
}

fn parse_assistant_event(value: &Value, out: &mut Vec<Out>, model_state: &mut Parser) {
    let user = &value["message"]["usage"];
    let ctx = ["input_tokens", "cache_read_input_tokens", "cache_creation_input_tokens"]
        .iter()
        .map(|key| user[key].as_u64().unwrap_or(0))
        .sum::<u64>();

    if ctx > 0 {
        model_state.ctx = ctx;
    }

    if let Some(id) = value["message"]["id"].as_str().filter(|_| user.is_object()) {
        let count = |key_value: &str| user[key_value].as_u64().unwrap_or(0);
        let call = TurnUse {
            input: count("input_tokens"),
            output: count("output_tokens"),
            cache_read: count("cache_read_input_tokens"),
            cache_write: count("cache_creation_input_tokens"),
            ctx,
        };

        match model_state.turn.iter_mut().find(|(i, _)| i == id) {
            Some((_, character)) => *character = call,
            None => model_state.turn.push((id.into(), call)),
        }

        out.push(Out::Usage(model_state.turn_use()));
    }

    for bytes in value["message"]["content"].as_array().into_iter().flatten() {
        match bytes["type"].as_str() {
            Some("text") => {
                let thread = bytes["text"].as_str().unwrap_or_default();

                skip_empty_assistant_text(out, thread);
            }
            // a thought with a summary (most thinking blocks: empty + a signature)
            Some("thinking") => {
                append_thought(out, bytes);
            }
            Some("tool_use") => out.push(Out::Event(Kind::Tool {
                id: bytes["id"].as_str().unwrap_or_default().into(),
                name: bytes["name"].as_str().unwrap_or_default().into(),
                input: bytes["input"].clone(),
            })),
            _ => {}
        }
    }
}

fn append_thought(out: &mut Vec<Out>, bytes: &Value) {
    let thread_value = bytes["thinking"].as_str().unwrap_or_default().trim();

    if !thread_value.is_empty() {
        out.push(Out::Event(Kind::Thought {
            text: thread_value.into(),
        }));
    }
}

fn skip_empty_assistant_text(out: &mut Vec<Out>, thread: &str) {
    if !thread.trim().is_empty() {
        out.push(Out::Event(Kind::Text { text: thread.into() }));
    }
}
