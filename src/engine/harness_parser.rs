use super::{Kind, OUTPUT_CAP, Out, Parser, TurnUse, Value};

/// Context window of the result's `modelUsage`: the session model's entry, else the largest
/// (subagents on other models add entries). 0: unknown.
pub(crate) fn window_of(usage: &Value, model: &str) -> u64 {
    let size = |entry: &Value| entry["contextWindow"].as_u64().unwrap_or(0);

    match usage.get(model) {
        Some(entry) if size(entry) > 0 => size(entry),
        _ => usage
            .as_object()
            .into_iter()
            .flatten()
            .map(|(_, entry)| size(entry))
            .max()
            .unwrap_or(0),
    }
}
pub(crate) fn text_of(content: &Value) -> String {
    match content {
        Value::String(text) => text.clone(),
        Value::Array(blocks) => blocks
            .iter()
            .filter_map(|block| block["text"].as_str())
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
        let mut total = self.turn.iter().fold(TurnUse::default(), |sum, (_, call)| TurnUse {
            input: sum.input + call.input,
            output: sum.output + call.output,
            cache_read: sum.cache_read + call.cache_read,
            cache_write: sum.cache_write + call.cache_write,
            ctx: 0,
        });

        total.ctx = self.ctx;
        total
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
                parse_result_event(value, &mut out, self);
            }
            // a token chunk of the main agent (subagents: 0)
            "stream_event" if value["parent_tool_use_id"].is_null() => {
                parse_stream_event(value, &mut out);
            }
            "rate_limit_event" => {
                parse_rate_limit_event(value, &mut out);
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
    for block in value["message"]["content"].as_array().into_iter().flatten() {
        if block["type"] == "tool_result" {
            out.push(Out::Event(Kind::ToolResult {
                id: block["tool_use_id"].as_str().unwrap_or_default().into(),
                ok: !block["is_error"].as_bool().unwrap_or(false),
                output: cap(text_of(&block["content"])),
            }));
        }
    }
}

fn parse_stream_event(value: &Value, out: &mut Vec<Out>) {
    let event = &value["event"];

    if event["type"] == "content_block_delta"
        && event["delta"]["type"] == "text_delta"
        && let Some(text) = event["delta"]["text"].as_str().filter(|text| !text.is_empty())
    {
        out.push(Out::Delta(text.into()));
    }
}

fn parse_control_request(value: &Value, out: &mut Vec<Out>) {
    if let Some(id) = value["request_id"].as_str() {
        out.push(Out::Control(id.into(), value["request"].clone()));
    }
}

fn parse_rate_limit_event(value: &Value, out: &mut Vec<Out>) {
    let windows = &value["rate_limit_info"]["unifiedWindows"];

    let limits: Vec<(String, f64, Option<u64>)> = windows
        .as_object()
        .into_iter()
        .flatten()
        .map(|(window, limit)| {
            (
                window.clone(),
                limit["utilization"].as_f64().unwrap_or(0.0) * 100.0,
                limit["resetsAt"].as_u64(),
            )
        })
        .collect();

    if !limits.is_empty() {
        out.push(Out::Limits(limits));
    }
}

fn parse_system_event(value: &Value, out: &mut Vec<Out>, parser: &mut Parser) {
    parser.model = value["model"].as_str().unwrap_or_default().into();
    out.push(Out::Event(Kind::Session {
        session: value["session_id"].as_str().unwrap_or_default().into(),
        model: parser.model.clone(),
    }));
    let mcp = crate::engine::servers::from_init(value);

    if !mcp.is_empty() {
        out.push(Out::Mcp("claude".into(), mcp));
    }
}

fn parse_result_event(value: &Value, out: &mut Vec<Out>, parser: &mut Parser) {
    parser.turn.clear();
    let total = value["total_cost_usd"].as_f64().unwrap_or(0.0);

    // a running total; a new process (or a smaller total) starts over
    let cost = if total >= parser.cost_total {
        total - parser.cost_total
    } else {
        total
    };

    parser.cost_total = total;
    let usage = &value["usage"];
    let tokens = |key: &str| usage[key].as_u64().unwrap_or(0);

    let error = if value["is_error"].as_bool().unwrap_or(false)
        || value["subtype"]
            .as_str()
            .is_some_and(|subtype| subtype.starts_with("error"))
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
        input: tokens("input_tokens"),
        output: tokens("output_tokens"),
        cache_read: tokens("cache_read_input_tokens"),
        cache_write: tokens("cache_creation_input_tokens"),
        ctx: parser.ctx,
        window: window_of(&value["modelUsage"], &parser.model),
        ms: value["duration_ms"].as_u64().unwrap_or(0),
        error,
    }));
}

fn parse_assistant_event(value: &Value, out: &mut Vec<Out>, parser: &mut Parser) {
    let usage = &value["message"]["usage"];
    let ctx = ["input_tokens", "cache_read_input_tokens", "cache_creation_input_tokens"]
        .iter()
        .map(|key| usage[key].as_u64().unwrap_or(0))
        .sum::<u64>();

    if ctx > 0 {
        parser.ctx = ctx;
    }

    if let Some(id) = value["message"]["id"].as_str().filter(|_| usage.is_object()) {
        let tokens = |key: &str| usage[key].as_u64().unwrap_or(0);
        let call = TurnUse {
            input: tokens("input_tokens"),
            output: tokens("output_tokens"),
            cache_read: tokens("cache_read_input_tokens"),
            cache_write: tokens("cache_creation_input_tokens"),
            ctx,
        };

        match parser.turn.iter_mut().find(|(message_id, _)| message_id == id) {
            Some((_, known)) => *known = call,
            None => parser.turn.push((id.into(), call)),
        }

        out.push(Out::Usage(parser.turn_use()));
    }

    for block in value["message"]["content"].as_array().into_iter().flatten() {
        match block["type"].as_str() {
            Some("text") => {
                let text = block["text"].as_str().unwrap_or_default();

                push_text(out, text);
            }
            // a thought with a summary (most thinking blocks: empty + a signature)
            Some("thinking") => {
                push_thought(out, block);
            }
            Some("tool_use") => out.push(Out::Event(Kind::Tool {
                id: block["id"].as_str().unwrap_or_default().into(),
                name: block["name"].as_str().unwrap_or_default().into(),
                input: block["input"].clone(),
            })),
            _ => {}
        }
    }
}

fn push_thought(out: &mut Vec<Out>, block: &Value) {
    let text = block["thinking"].as_str().unwrap_or_default().trim();

    if !text.is_empty() {
        out.push(Out::Event(Kind::Thought { text: text.into() }));
    }
}

fn push_text(out: &mut Vec<Out>, text: &str) {
    if !text.trim().is_empty() {
        out.push(Out::Event(Kind::Text { text: text.into() }));
    }
}
