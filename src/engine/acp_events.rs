use super::{Kind, Out, State, Value, tool_of};

impl State {
    pub(crate) fn update(&mut self, update: &Value) {
        if self.loading {
            return;
        }

        match update["sessionUpdate"].as_str().unwrap_or_default() {
            "agent_message_chunk" => {
                handle_agent_message_chunk(update, self);
            }
            "agent_thought_chunk" => {
                handle_agent_thought_chunk(update, self);
            }
            "tool_call" => {
                self.flush_text();
                let (name, input) = tool_of(update);

                self.out(Out::Event(Kind::Tool {
                    id: update["toolCallId"].as_str().unwrap_or_default().into(),
                    name,
                    input,
                }));
                result_of(update, self);
            }
            "tool_call_update" => result_of(update, self),
            // the context so far (ACP `usage_update`: used / size)
            "usage_update" => {
                handle_usage_update(update, self);
            }
            _ => {}
        }
    }
}

fn floor_char_boundary(text: &str, end: &mut usize) {
    while !text.is_char_boundary(*end) {
        *end -= 1;
    }
}

fn handle_usage_update(update: &Value, state: &mut State) {
    if let Some(used) = update["used"].as_u64().filter(|count| *count > 0) {
        state.out(Out::Usage(crate::engine::harness::TurnUse {
            ctx: used,
            ..Default::default()
        }));
    }
}

fn handle_agent_message_chunk(update: &Value, state: &mut State) {
    if let Some(chunk) = update["content"]["text"].as_str() {
        // a thought before it: its own event, before the text
        state.flush_thought();
        state.text.push_str(chunk);

        // live in the view now; the log gets the whole text at the next tool call or turn end
        emit_delta(chunk, state);
    }
}

fn emit_delta(chunk: &str, state: &mut State) {
    if !chunk.is_empty() {
        state.out(Out::Delta(chunk.into()));
    }
}

fn handle_agent_thought_chunk(update: &Value, state: &mut State) {
    if let Some(thought) = update["content"]["text"].as_str().filter(|chunk| !chunk.is_empty()) {
        if !state.text.trim().is_empty() {
            let text = std::mem::take(&mut state.text);

            state.out(Out::Event(Kind::Text { text }));
        }

        state.thought.push_str(thought);
    }
}

/// A finished tool call: its result event.
fn result_of(update: &Value, state: &mut State) {
    let status = update["status"].as_str().unwrap_or_default();

    if status != "completed" && status != "failed" {
        return;
    }

    let mut out: Vec<String> = Vec::new();

    for item in update["content"].as_array().into_iter().flatten() {
        match item["type"].as_str() {
            Some("content") => out.extend(item["content"]["text"].as_str().map(String::from)),
            Some("diff") => out.push(format!("diff {}", item["path"].as_str().unwrap_or_default())),
            _ => {}
        }
    }

    if out.is_empty()
        && let Some(raw) = update.get("rawOutput").filter(|raw| !raw.is_null())
    {
        out.push(raw.as_str().map_or_else(|| raw.to_string(), String::from));
    }

    let mut text = out.join("\n");

    if text.len() > 4096 {
        let mut end = 4096;

        floor_char_boundary(&text, &mut end);

        text.truncate(end);
        text.push_str("\n…");
    }

    state.out(Out::Event(Kind::ToolResult {
        id: update["toolCallId"].as_str().unwrap_or_default().into(),
        ok: status == "completed",
        output: text,
    }));
}
