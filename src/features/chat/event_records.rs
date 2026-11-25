use super::{
    Ask, Event, Item, Kind, Sub, Thread, end_subagent, end_turn, push_call, push_leave, push_prompt, push_status,
    push_subthread, rerun_subagent, set_call_output, set_resume, settle_ask, theme, update_subagent,
};

pub(super) fn record_prompt(thread: &mut Thread, event: &Event, _agent: String) {
    let Kind::Prompt { from, to, text } = &event.kind else {
        return;
    };

    push_prompt(event, from, to, text, thread);
}
pub(super) fn record_text(thread: &mut Thread, event: &Event, agent: String) {
    let Kind::Text { text } = &event.kind else { return };

    thread.open_run.remove(&agent);
    thread.text(agent, text.clone(), event.ts);
}
pub(super) fn record_thought(thread: &mut Thread, event: &Event, agent: String) {
    let Kind::Thought { text } = &event.kind else {
        return;
    };

    thread.pending.entry(agent).or_default().push(text.clone());
}
pub(super) fn record_tool(thread: &mut Thread, event: &Event, agent: String) {
    let Kind::Tool { id, name, input } = &event.kind else {
        return;
    };

    push_call(event, agent, id, name, input, thread);
}
pub(super) fn record_tool_result(thread: &mut Thread, event: &Event, agent: &str) {
    let Kind::ToolResult { id, ok, output } = &event.kind else {
        return;
    };

    set_call_output(thread, event, agent, id, *ok, output);
}
pub(super) fn record_ask(thread: &mut Thread, event: &Event, agent: String) {
    let Kind::Ask { id, tool, input } = &event.kind else {
        return;
    };

    thread.asks.push(Ask {
        id: id.clone(),
        agent,
        tool: tool.clone(),
        input: input.clone(),
        ts: event.ts,
    });
}
pub(super) fn record_answer(thread: &mut Thread, event: &Event, agent: String) {
    let Kind::Answer { id, allow, label } = &event.kind else {
        return;
    };

    settle_ask(event, agent, id, *allow, label, thread);
}
pub(super) fn record_turn_start(thread: &mut Thread, event: &Event, agent: String) {
    let Kind::TurnStart = &event.kind else { return };

    thread.resumes.remove(&agent);
    thread.busy.insert(agent);
}
pub(super) fn record_resume(thread: &mut Thread, event: &Event, agent: &str) {
    let Kind::Resume { at } = &event.kind else { return };

    set_resume(agent, *at, thread);
}
pub(super) fn record_turn_end(thread: &mut Thread, event: &Event, agent: String) {
    let Kind::TurnEnd { error, .. } = &event.kind else {
        return;
    };

    end_turn(event, agent, error.as_deref(), thread);
}
pub(super) fn record_session(thread: &mut Thread, event: &Event, agent: String) {
    let Kind::Session { model, .. } = &event.kind else {
        return;
    };

    thread.models.insert(agent, model.clone());
}
pub(super) fn record_status(thread: &mut Thread, event: &Event, agent: String) {
    let Kind::Status { state, note } = &event.kind else {
        return;
    };

    push_status(agent, state, note, thread);
}
pub(super) fn record_join(thread: &mut Thread, event: &Event, agent: String) {
    let Kind::Join = &event.kind else { return };

    thread.items.push(Item::Note {
        agent,
        text: "joined the thread".into(),
        color: theme::pal().subtext0,
    });
}
pub(super) fn record_leave(thread: &mut Thread, event: &Event, agent: String) {
    let Kind::Leave = &event.kind else { return };

    push_leave(thread, agent);
}
pub(super) fn record_error(thread: &mut Thread, event: &Event, agent: String) {
    let Kind::Error { message } = &event.kind else {
        return;
    };

    thread.items.push(Item::Note {
        agent,
        text: message.clone(),
        color: theme::pal().error(),
    });
}
pub(super) fn record_shell(thread: &mut Thread, event: &Event, _agent: String) {
    let Kind::Shell {
        command,
        output,
        code,
        dir,
    } = &event.kind
    else {
        return;
    };

    thread.items.push(Item::Shell {
        command: command.clone(),
        output: output.clone(),
        code: *code,
        ts: event.ts,
        dir: dir.clone(),
    });
}
pub(super) fn record_flow(_thread: &mut Thread, event: &Event, _agent: String) {
    let Kind::Flow { .. } = &event.kind else { return };
}
pub(super) fn record_edits(thread: &mut Thread, event: &Event, agent: String) {
    let Kind::Edits { summary } = &event.kind else {
        return;
    };

    thread.items.push(Item::Edits {
        agent,
        text: format!("got the edits since its last turn · {summary}"),
    });
}
pub(super) fn record_sub_thread(thread: &mut Thread, event: &Event, agent: String) {
    let Kind::SubThread { title, state, text, .. } = &event.kind else {
        return;
    };

    push_subthread(event, agent, title, state, text, thread);
}
pub(super) fn record_sub_start(thread: &mut Thread, event: &Event, agent: String) {
    let Kind::SubStart {
        id,
        parent,
        description,
        agent_type,
        background,
        prompt,
    } = &event.kind
    else {
        return;
    };

    let parent = parent.as_ref();
    let background = *background;

    if !thread.subs.iter().any(|sub| &sub.id == id) {
        thread.subs.push(Sub {
            id: id.clone(),
            master: agent,
            parent: parent.cloned(),
            description: description.to_owned(),
            agent_type: agent_type.to_owned(),
            background,
            prompt: prompt.to_owned(),
            start: event.ts,
            ..Default::default()
        });
    }
}
pub(super) fn record_sub_info(thread: &mut Thread, event: &Event, _agent: String) {
    let Kind::SubInfo { id, transcript, task } = &event.kind else {
        return;
    };

    update_subagent(thread, id, transcript, task);
}
pub(super) fn record_sub_end(thread: &mut Thread, event: &Event, agent: String) {
    let Kind::SubEnd { id, state, result } = &event.kind else {
        return;
    };

    end_subagent(event, agent, id, state, result, thread);
}
pub(super) fn record_sub_resume(thread: &mut Thread, event: &Event, _agent: String) {
    let Kind::SubResume { id } = &event.kind else {
        return;
    };

    rerun_subagent(event, id, thread);
}
