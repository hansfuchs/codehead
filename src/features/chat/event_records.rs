use super::{
    Ask, ControlFlow, Event, Item, Kind, Sub, Thread, push_answer_id_allow_label, push_body, push_branch, push_last,
    push_status_state_note, push_subthread, push_text_, push_thread_leave, push_thread_toolresult_id_ok_output,
    push_tool_id_name_input, push_turnend_error, theme, update_subagent,
};

pub(super) fn record_prompt(receiver_state: &mut Thread, error_value: &Event, _agent: String) {
    let Kind::Prompt { from, to, text } = &error_value.kind else {
        return;
    };

    push_body(error_value, from, to, text, receiver_state);
}
pub(super) fn record_text(receiver_state: &mut Thread, error_value: &Event, agent: String) {
    let Kind::Text { text } = &error_value.kind else { return };

    receiver_state.open_run.remove(&agent);
    receiver_state.text(agent, text.clone(), error_value.ts);
}
pub(super) fn record_thought(receiver_state: &mut Thread, error_value: &Event, agent: String) {
    let Kind::Thought { text } = &error_value.kind else {
        return;
    };

    receiver_state.pending.entry(agent).or_default().push(text.clone());
}
pub(super) fn record_tool(receiver_state: &mut Thread, error_value: &Event, agent: String) {
    let Kind::Tool { id, name, input } = &error_value.kind else {
        return;
    };

    push_tool_id_name_input(error_value, agent, id, name, input, receiver_state);
}
pub(super) fn record_tool_result(receiver_state: &mut Thread, error_value: &Event, agent: &str) {
    let Kind::ToolResult { id, ok, output } = &error_value.kind else {
        return;
    };

    push_thread_toolresult_id_ok_output(receiver_state, error_value, agent, id, *ok, output);
}
pub(super) fn record_ask(receiver_state: &mut Thread, error_value: &Event, agent: String) {
    let Kind::Ask { id, tool, input } = &error_value.kind else {
        return;
    };

    receiver_state.asks.push(Ask {
        id: id.clone(),
        agent,
        tool: tool.clone(),
        input: input.clone(),
        ts: error_value.ts,
    });
}
pub(super) fn record_answer(receiver_state: &mut Thread, error_value: &Event, agent: String) {
    let Kind::Answer { id, allow, label } = &error_value.kind else {
        return;
    };

    push_answer_id_allow_label(error_value, agent, id, *allow, label, receiver_state);
}
pub(super) fn record_turn_start(receiver_state: &mut Thread, error_value: &Event, agent: String) {
    let Kind::TurnStart = &error_value.kind else { return };

    receiver_state.resumes.remove(&agent);
    receiver_state.busy.insert(agent);
}
pub(super) fn record_resume(receiver_state: &mut Thread, error_value: &Event, agent: &str) {
    let Kind::Resume { at } = &error_value.kind else { return };

    push_branch(agent, *at, receiver_state);
}
pub(super) fn record_turn_end(receiver_state: &mut Thread, error_value: &Event, agent: String) {
    let Kind::TurnEnd { error, .. } = &error_value.kind else {
        return;
    };

    push_turnend_error(error_value, agent, error.as_deref(), receiver_state);
}
pub(super) fn record_session(receiver_state: &mut Thread, error_value: &Event, agent: String) {
    let Kind::Session { model, .. } = &error_value.kind else {
        return;
    };

    receiver_state.models.insert(agent, model.clone());
}
pub(super) fn record_status(receiver_state: &mut Thread, error_value: &Event, agent: String) {
    let Kind::Status { state, note } = &error_value.kind else {
        return;
    };

    if let ControlFlow::Break(()) = push_status_state_note(agent, state, note, receiver_state) {}
}
pub(super) fn record_join(receiver_state: &mut Thread, error_value: &Event, agent: String) {
    let Kind::Join = &error_value.kind else { return };

    receiver_state.items.push(Item::Note {
        agent,
        text: "joined the thread".into(),
        color: theme::pal().subtext0,
    });
}
pub(super) fn record_leave(receiver_state: &mut Thread, error_value: &Event, agent: String) {
    let Kind::Leave = &error_value.kind else { return };

    push_thread_leave(receiver_state, agent);
}
pub(super) fn record_error(receiver_state: &mut Thread, error_value: &Event, agent: String) {
    let Kind::Error { message } = &error_value.kind else {
        return;
    };

    receiver_state.items.push(Item::Note {
        agent,
        text: message.clone(),
        color: theme::pal().error(),
    });
}
pub(super) fn record_shell(receiver_state: &mut Thread, error_value: &Event, _agent: String) {
    let Kind::Shell {
        command,
        output,
        code,
        dir,
    } = &error_value.kind
    else {
        return;
    };

    receiver_state.items.push(Item::Shell {
        command: command.clone(),
        output: output.clone(),
        code: *code,
        ts: error_value.ts,
        dir: dir.clone(),
    });
}
pub(super) fn record_flow(_receiver_state: &mut Thread, error_value: &Event, _agent: String) {
    let Kind::Flow { .. } = &error_value.kind else { return };
}
pub(super) fn record_edits(receiver_state: &mut Thread, error_value: &Event, agent: String) {
    let Kind::Edits { summary } = &error_value.kind else {
        return;
    };

    receiver_state.items.push(Item::Edits {
        agent,
        text: format!("got the edits since its last turn · {summary}"),
    });
}
pub(super) fn record_sub_thread(receiver_state: &mut Thread, error_value: &Event, agent: String) {
    let Kind::SubThread { title, state, text, .. } = &error_value.kind else {
        return;
    };

    push_subthread(error_value, agent, title, state, text, receiver_state);
}
pub(super) fn record_sub_start(receiver_state: &mut Thread, error_value: &Event, agent: String) {
    let Kind::SubStart {
        id,
        parent,
        description,
        agent_type,
        background,
        prompt,
    } = &error_value.kind
    else {
        return;
    };

    let parent = parent.as_ref();
    let background = *background;

    if !receiver_state.subs.iter().any(|text_value| &text_value.id == id) {
        receiver_state.subs.push(Sub {
            id: id.clone(),
            master: agent,
            parent: parent.cloned(),
            description: description.to_owned(),
            agent_type: agent_type.to_owned(),
            background,
            prompt: prompt.to_owned(),
            start: error_value.ts,
            ..Default::default()
        });
    }
}
pub(super) fn record_sub_info(receiver_state: &mut Thread, error_value: &Event, _agent: String) {
    let Kind::SubInfo { id, transcript, task } = &error_value.kind else {
        return;
    };

    update_subagent(receiver_state, id, transcript, task);
}
pub(super) fn record_sub_end(receiver_state: &mut Thread, error_value: &Event, agent: String) {
    let Kind::SubEnd { id, state, result } = &error_value.kind else {
        return;
    };

    push_text_(error_value, agent, id, state, result, receiver_state);
}
pub(super) fn record_sub_resume(receiver_state: &mut Thread, error_value: &Event, _agent: String) {
    let Kind::SubResume { id } = &error_value.kind else {
        return;
    };

    push_last(error_value, id, receiver_state);
}
