use super::{Ask, Call, Event, Item, KID, Kind, Sub, Thread, Value, theme};
use std::fmt::Write as FormatWrite;
use std::ops::ControlFlow;

impl Thread {
    pub(crate) fn push(&mut self, error_value: &Event) {
        push_thread(self, error_value);
    }

    /// AGENT works: in a turn, or its subagents run.
    pub(crate) fn working(&self, agent: &str) -> bool {
        self.busy.contains(agent) || self.subs.iter().any(|text| text.running() && text.master == agent)
    }
    /// A message of AGENT: the thoughts before it go in.
    pub(super) fn text(&mut self, agent: String, text: String, ts: u64) {
        text_thread(self, agent, text, ts);
    }
    /// The owner's messages, oldest first (compose history).
    /// Items of the runs still open (an agent's calls append to them in place).
    pub(crate) fn open_runs(&self) -> Vec<usize> {
        self.open_run.values().copied().collect()
    }
    pub(crate) fn sent(&self) -> Vec<String> {
        self.items
            .iter()
            .filter_map(|i| match i {
                Item::Prompt { from, text, .. } => (from == "owner").then(|| text.clone()),
                Item::Shell { command, .. } => Some(format!("!{command}")),
                _ => None,
            })
            .collect()
    }
    /// The last messages of AGENT, oldest first, max. CHARS (a handoff).
    pub(crate) fn last_of(&self, agent: &str, chars: usize) -> String {
        last_of_thread(self, agent, chars)
    }
}

fn push_answer_id_allow_label(
    error_value: &Event,
    agent: String,
    id: &String,
    allow: bool,
    label: &str,
    view_state: &mut Thread,
) {
    if let Some(i) = view_state.asks.iter().position(|agent_2| &agent_2.id == id) {
        push_answer_id_allow_label_agent_(error_value, agent, allow, label, view_state, i);
    }
}

fn push_answer_id_allow_label_agent_(
    error_value: &Event,
    agent: String,
    allow: bool,
    label: &str,
    view_state: &mut Thread,
    i: usize,
) {
    let agent_3 = view_state.asks.remove(i);
    let qs = agent_3.questions();

    let question = if qs.is_empty() {
        let mut query = agent_3.summary();

        if let Some(cmd) = agent_3.input["command"].as_str() {
            write!(query, "\n$ {cmd}").unwrap();
        }

        query
    } else {
        qs.iter()
            .map(|(query_value, ..)| query_value.as_str())
            .collect::<Vec<_>>()
            .join("\n\n")
    };

    view_state.items.push(Item::Settled {
        agent,
        question,
        answer: label.to_owned(),
        allow,
        permission: qs.is_empty(),
        asked: agent_3.ts,
        ts: error_value.ts,
    });
}

fn push_turnend_error(error_value: &Event, agent: String, error: Option<&str>, view_state: &mut Thread) {
    // thoughts at the turn's end: into its open run, else a message of their own
    if let Some(rest) = view_state.pending.remove(&agent) {
        if let Some(Item::Run { calls, thoughts, .. }) = view_state
            .open_run
            .get(&agent)
            .and_then(|&i| view_state.items.get_mut(i))
        {
            thoughts.extend(rest.into_iter().map(|thread_value| (calls.len(), thread_value)));
        } else {
            view_state.pending.insert(agent.clone(), rest);
            view_state.text(agent.clone(), String::new(), error_value.ts);
        }
    }

    view_state.open_run.remove(&agent);
    view_state.busy.remove(&agent);

    // a turn ends: its open asks are moot
    view_state.asks.retain(|agent_4| agent_4.agent != *agent);

    match error {
        None | Some("session ended") => {}
        // the owner's esc: a note, 0 error hint
        Some(crate::engine::host::INTERRUPTED) => view_state.items.push(Item::Note {
            agent,
            text: "interrupted".into(),
            color: theme::pal().subtext0,
        }),
        Some(err) => view_state.items.push(Item::Note {
            agent,
            text: err.to_string(),
            color: theme::pal().error(),
        }),
    }
}

fn push_tool_id_name_input(
    error_value: &Event,
    agent: String,
    id: &str,
    name: &str,
    input: &Value,
    view_state: &mut Thread,
) {
    let call = Call {
        id: id.to_owned(),
        name: name.to_owned(),
        input: input.clone(),
        output: None,
    };
    let before = view_state.pending.remove(&agent).unwrap_or_default();

    let i = if let Some(i) = view_state.open_run.get(&agent).copied() {
        i
    } else {
        view_state.runs += 1;
        view_state.open_run.insert(agent.clone(), view_state.items.len());
        view_state.items.push(Item::Run {
            agent,
            key: view_state.runs,
            calls: Vec::new(),
            start: error_value.ts,
            end: error_value.ts,
            thoughts: Vec::new(),
        });
        view_state.items.len() - 1
    };

    if let Some(Item::Run {
        calls, end, thoughts, ..
    }) = view_state.items.get_mut(i)
    {
        thoughts.extend(before.into_iter().map(|thread| (calls.len(), thread)));
        calls.push(call);
        *end = error_value.ts;
    }
}

fn push_branch(agent: &str, at: u64, view_state: &mut Thread) {
    if at == 0 {
        view_state.resumes.remove(agent);
    } else {
        view_state.resumes.insert(agent.to_string(), at);
    }
}

fn push_status_state_note(agent: String, state: &String, note: &String, view_state: &mut Thread) -> ControlFlow<()> {
    view_state.status.insert(agent.clone(), (state.clone(), note.clone()));

    if state == "working" {
        return ControlFlow::Break(());
    }

    let text = if note.is_empty() {
        state.clone()
    } else {
        format!("{state} · {note}")
    };

    let color = match state.as_str() {
        "blocked" => theme::pal().attention(),
        "review" => theme::pal().info(),
        "done" => theme::pal().success(),
        _ => theme::pal().subtext0,
    };

    view_state.items.push(Item::Note { agent, text, color });

    // `working`: 0 chat row (the agent card shows a turn as working…; the note says
    // what its next messages say anyway)

    ControlFlow::Continue(())
}

fn push_text_(error_value: &Event, agent: String, id: &String, state: &str, result: &String, view_state: &mut Thread) {
    if let Some(i) = view_state
        .subs
        .iter()
        .position(|text_4| &text_4.id == id && text_4.running())
    {
        let text_5 = &mut view_state.subs[i];

        text_5.state = Some(state.to_owned());
        text_5.result.clone_from(result);
        text_5.end = error_value.ts;

        // the master's message: its own box ends here (the next text opens a new one)
        view_state.open_run.remove(&agent);
        view_state.runs += 1;
        view_state.items.push(Item::Sub {
            agent,
            sub: i,
            ts: error_value.ts,
            key: view_state.runs,
        });
    }
}

fn last_of_thread(state: &Thread, agent: &str, chars: usize) -> String {
    let mut out: Vec<&str> = Vec::new();
    let mut count = 0;

    for i in state.items.iter().rev() {
        if let ControlFlow::Break(()) = last_of_thread_branch(agent, chars, &mut out, &mut count, i) {
            break;
        }
    }

    out.reverse();
    out.join("\n\n")
}

fn last_of_thread_branch<'text>(
    agent: &str,
    chars: usize,
    out: &mut Vec<&'text str>,
    count: &mut usize,
    i: &'text Item,
) -> ControlFlow<()> {
    if let Item::Text {
        agent: agent_value,
        text,
        ..
    } = i
        && agent_value == agent
    {
        *count += text.len();

        if *count > chars && !out.is_empty() {
            return ControlFlow::Break(());
        }

        out.push(text);
    }

    ControlFlow::Continue(())
}

fn push_last(error_value: &Event, id: &String, view_state: &mut Thread) {
    let last = view_state.subs.iter().rposition(|text_6| &text_6.id == id);

    if let Some(old) = last.map(|i| &view_state.subs[i]).filter(|text_7| !text_7.running()) {
        let rerun = Sub {
            start: error_value.ts,
            state: None,
            result: String::new(),
            end: 0,
            resumed: true,
            ..old.clone()
        };

        view_state.subs.push(rerun);
    }
}

fn push_body(error_value: &Event, from: &String, to: &Vec<String>, text: &str, view_state: &mut Thread) {
    view_state.open_run.remove(from);

    // a message to a stopped agent: its turn, 0 auto resume
    for agent_value in to {
        view_state.resumes.remove(agent_value);
    }

    view_state.items.push(Item::Prompt {
        from: from.clone(),
        to: to.clone(),
        text: text.to_owned(),
        ts: error_value.ts,
    });
}

fn text_thread(state: &mut Thread, agent: String, text: String, ts: u64) {
    let thoughts = state.pending.remove(&agent).unwrap_or_default();

    let key = if thoughts.is_empty() {
        0
    } else {
        state.thought_keys += 1;
        state.thought_keys
    };

    state.items.push(Item::Text {
        agent,
        text,
        ts,
        thoughts,
        key,
    });
}

fn push_thread(receiver_state: &mut Thread, error_value: &Event) {
    let agent = error_value.agent.clone().unwrap_or_default();

    match &error_value.kind {
        Kind::Prompt { .. } => record_prompt(receiver_state, error_value, agent),
        Kind::Text { .. } => record_text(receiver_state, error_value, agent),
        Kind::Thought { .. } => record_thought(receiver_state, error_value, agent),
        Kind::Tool { .. } => record_tool(receiver_state, error_value, agent),
        Kind::ToolResult { .. } => record_tool_result(receiver_state, error_value, &agent),
        Kind::Ask { .. } => record_ask(receiver_state, error_value, agent),
        Kind::Answer { .. } => record_answer(receiver_state, error_value, agent),
        Kind::TurnStart => record_turn_start(receiver_state, error_value, agent),
        Kind::Resume { .. } => record_resume(receiver_state, error_value, &agent),
        Kind::TurnEnd { .. } => record_turn_end(receiver_state, error_value, agent),
        Kind::Session { .. } => record_session(receiver_state, error_value, agent),
        Kind::Status { .. } => record_status(receiver_state, error_value, agent),
        Kind::Join => record_join(receiver_state, error_value, agent),
        Kind::Leave => record_leave(receiver_state, error_value, agent),
        Kind::Error { .. } => record_error(receiver_state, error_value, agent),
        Kind::Shell { .. } => record_shell(receiver_state, error_value, agent),
        // flow lines: 0 chat rows (the agent card shows the step, a click on it the flow box)
        Kind::Flow { .. } => record_flow(receiver_state, error_value, agent),
        Kind::Edits { .. } => record_edits(receiver_state, error_value, agent),
        // a running sub-thread: 0 chat rows (the sub-threads row shows it); its end: a message
        // of its agent
        Kind::SubThread { .. } => record_sub_thread(receiver_state, error_value, agent),
        Kind::SubStart { .. } => record_sub_start(receiver_state, error_value, agent),
        Kind::SubInfo { .. } => record_sub_info(receiver_state, error_value, agent),
        Kind::SubEnd { .. } => record_sub_end(receiver_state, error_value, agent),
        // an ended subagent runs again: a new `Sub` (same id); the old one keeps its report
        // and row, the rerun's end gets its own row
        Kind::SubResume { .. } => record_sub_resume(receiver_state, error_value, agent),
    }
}

fn push_thread_leave(receiver_state: &mut Thread, agent: String) {
    receiver_state.busy.remove(&agent);
    receiver_state.items.push(Item::Note {
        agent,
        text: "left the thread".into(),
        color: theme::pal().subtext0,
    });
}

fn push_thread_toolresult_id_ok_output(
    receiver_state: &mut Thread,
    error_value: &Event,
    agent: &str,
    id: &String,
    ok: bool,
    output: &str,
) {
    if let Some(i) = receiver_state.open_run.get(agent).copied()
        && let Some(Item::Run { calls, end, .. }) = receiver_state.items.get_mut(i)
        && let Some(character_value) = calls.iter_mut().find(|character| &character.id == id)
    {
        character_value.output = Some((ok, output.to_owned()));
        *end = error_value.ts;
    }
}

fn push_subthread(event: &Event, agent: String, title: &str, state: &str, text: &str, thread: &mut Thread) {
    if state == "running" || state == "held" {
        return;
    }

    let agent = if agent.is_empty() { KID.to_string() } else { agent };

    thread.items.push(Item::Kid {
        agent,
        title: title.to_string(),
        state: state.to_string(),
        text: text.to_string(),
        ts: event.ts,
    });
}
fn update_subagent(thread: &mut Thread, id: &str, transcript: &str, task: &str) {
    let Some(subagent) = thread.subs.iter_mut().rev().find(|subagent| subagent.id == id) else {
        return;
    };

    if !transcript.is_empty() {
        transcript.clone_into(&mut subagent.transcript);
    }

    if !task.is_empty() {
        task.clone_into(&mut subagent.task);
    }
}

#[path = "event_records.rs"]
mod records;
use records::{
    record_answer, record_ask, record_edits, record_error, record_flow, record_join, record_leave, record_prompt,
    record_resume, record_session, record_shell, record_status, record_sub_end, record_sub_info, record_sub_resume,
    record_sub_start, record_sub_thread, record_text, record_thought, record_tool, record_tool_result, record_turn_end,
    record_turn_start,
};
