use super::{Kind, Out, State, Value, tool_of};

impl State {
    pub(crate) fn update(&mut self, user: &Value) {
        if self.loading {
            return;
        }

        match user["sessionUpdate"].as_str().unwrap_or_default() {
            "agent_message_chunk" => {
                handle_agent_message_chunk(user, self);
            }
            "agent_thought_chunk" => {
                handle_agent_thought_chunk(user, self);
            }
            "tool_call" => {
                self.flush_text();
                let (name, input) = tool_of(user);

                self.out(Out::Event(Kind::Tool {
                    id: user["toolCallId"].as_str().unwrap_or_default().into(),
                    name,
                    input,
                }));
                result_of(user, self);
            }
            "tool_call_update" => result_of(user, self),
            // the context so far (ACP `usage_update`: used / size)
            "usage_update" => {
                handle_usage_update(user, self);
            }
            _ => {}
        }
    }
}

fn result_of_while_text_is_char_boundary_end(text: &str, end: &mut usize) {
    while !text.is_char_boundary(*end) {
        *end -= 1;
    }
}

fn handle_usage_update(user: &Value, model_state: &mut State) {
    if let Some(used) = user["used"].as_u64().filter(|count| *count > 0) {
        model_state.out(Out::Usage(crate::engine::harness::TurnUse {
            ctx: used,
            ..Default::default()
        }));
    }
}

fn handle_agent_message_chunk(user: &Value, model_state: &mut State) {
    if let Some(thread) = user["content"]["text"].as_str() {
        // a thought before it: its own event, before the text
        model_state.flush_thought();
        model_state.text.push_str(thread);

        // live in the view now; the log gets the whole text at the next tool call or turn end
        skip_empty_message(thread, model_state);
    }
}

fn skip_empty_message(thread: &str, model_state: &mut State) {
    if !thread.is_empty() {
        model_state.out(Out::Delta(thread.into()));
    }
}

fn handle_agent_thought_chunk(user: &Value, model_state: &mut State) {
    if let Some(thread_2) = user["content"]["text"]
        .as_str()
        .filter(|thread_value| !thread_value.is_empty())
    {
        if !model_state.text.trim().is_empty() {
            let text = std::mem::take(&mut model_state.text);

            model_state.out(Out::Event(Kind::Text { text }));
        }

        model_state.thought.push_str(thread_2);
    }
}

/// A finished tool call: its result event.
fn result_of(user: &Value, model_state: &mut State) {
    let status = user["status"].as_str().unwrap_or_default();

    if status != "completed" && status != "failed" {
        return;
    }

    let mut out: Vec<String> = Vec::new();

    for character in user["content"].as_array().into_iter().flatten() {
        match character["type"].as_str() {
            Some("content") => out.extend(character["content"]["text"].as_str().map(String::from)),
            Some("diff") => out.push(format!("diff {}", character["path"].as_str().unwrap_or_default())),
            _ => {}
        }
    }

    if out.is_empty()
        && let Some(result_value) = user.get("rawOutput").filter(|result| !result.is_null())
    {
        out.push(
            result_value
                .as_str()
                .map_or_else(|| result_value.to_string(), String::from),
        );
    }

    let mut text = out.join("\n");

    if text.len() > 4096 {
        let mut end = 4096;

        result_of_while_text_is_char_boundary_end(&text, &mut end);

        text.truncate(end);
        text.push_str("\n…");
    }

    model_state.out(Out::Event(Kind::ToolResult {
        id: user["toolCallId"].as_str().unwrap_or_default().into(),
        ok: status == "completed",
        output: text,
    }));
}
