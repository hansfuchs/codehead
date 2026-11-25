use super::{Ask, Call, Event, Item, KID, Kind, Sub, Thread, Value, theme};
use std::fmt::Write as FormatWrite;
use std::ops::ControlFlow;

impl Thread {
    pub(crate) fn push(&mut self, event: &Event) {
        record_event(self, event);
    }

    /// AGENT works: in a turn, or its subagents run.
    pub(crate) fn working(&self, agent: &str) -> bool {
        self.busy.contains(agent) || self.subs.iter().any(|sub| sub.running() && sub.master == agent)
    }
    /// A message of AGENT: the thoughts before it go in.
    pub(super) fn text(&mut self, agent: String, text: String, ts: u64) {
        push_text(self, agent, text, ts);
    }
    /// Items of the runs still open (an agent's calls append to them in place).
    pub(crate) fn open_runs(&self) -> Vec<usize> {
        self.open_run.values().copied().collect()
    }
    /// The owner's messages, oldest first (compose history).
    pub(crate) fn sent(&self) -> Vec<String> {
        self.items
            .iter()
            .filter_map(|item| match item {
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

fn settle_ask(event: &Event, agent: String, id: &String, allow: bool, label: &str, thread: &mut Thread) {
    if let Some(i) = thread.asks.iter().position(|ask| &ask.id == id) {
        push_settled(event, agent, allow, label, thread, i);
    }
}

fn push_settled(event: &Event, agent: String, allow: bool, label: &str, thread: &mut Thread, i: usize) {
    let ask = thread.asks.remove(i);
    let qs = ask.questions();

    let question = if qs.is_empty() {
        let mut query = ask.summary();

        if let Some(cmd) = ask.input["command"].as_str() {
            write!(query, "\n$ {cmd}").unwrap();
        }

        query
    } else {
        qs.iter()
            .map(|(question, ..)| question.as_str())
            .collect::<Vec<_>>()
            .join("\n\n")
    };

    thread.items.push(Item::Settled {
        agent,
        question,
        answer: label.to_owned(),
        allow,
        permission: qs.is_empty(),
        asked: ask.ts,
        ts: event.ts,
    });
}

fn end_turn(event: &Event, agent: String, error: Option<&str>, thread: &mut Thread) {
    // thoughts at the turn's end: into its open run, else a message of their own
    if let Some(rest) = thread.pending.remove(&agent) {
        if let Some(Item::Run { calls, thoughts, .. }) =
            thread.open_run.get(&agent).and_then(|&i| thread.items.get_mut(i))
        {
            thoughts.extend(rest.into_iter().map(|thought| (calls.len(), thought)));
        } else {
            thread.pending.insert(agent.clone(), rest);
            thread.text(agent.clone(), String::new(), event.ts);
        }
    }

    thread.open_run.remove(&agent);
    thread.busy.remove(&agent);

    // a turn ends: its open asks are moot
    thread.asks.retain(|ask| ask.agent != *agent);

    match error {
        None | Some("session ended") => {}
        // the owner's esc: a note, 0 error hint
        Some(crate::engine::host::INTERRUPTED) => thread.items.push(Item::Note {
            agent,
            text: "interrupted".into(),
            color: theme::pal().subtext0,
        }),
        Some(err) => thread.items.push(Item::Note {
            agent,
            text: err.to_string(),
            color: theme::pal().error(),
        }),
    }
}

fn push_call(event: &Event, agent: String, id: &str, name: &str, input: &Value, thread: &mut Thread) {
    let call = Call {
        id: id.to_owned(),
        name: name.to_owned(),
        input: input.clone(),
        output: None,
    };
    let before = thread.pending.remove(&agent).unwrap_or_default();

    let i = if let Some(i) = thread.open_run.get(&agent).copied() {
        i
    } else {
        thread.runs += 1;
        thread.open_run.insert(agent.clone(), thread.items.len());
        thread.items.push(Item::Run {
            agent,
            key: thread.runs,
            calls: Vec::new(),
            start: event.ts,
            end: event.ts,
            thoughts: Vec::new(),
        });
        thread.items.len() - 1
    };

    if let Some(Item::Run {
        calls, end, thoughts, ..
    }) = thread.items.get_mut(i)
    {
        thoughts.extend(before.into_iter().map(|thought| (calls.len(), thought)));
        calls.push(call);
        *end = event.ts;
    }
}

fn set_resume(agent: &str, at: u64, thread: &mut Thread) {
    if at == 0 {
        thread.resumes.remove(agent);
    } else {
        thread.resumes.insert(agent.to_string(), at);
    }
}

fn push_status(agent: String, state: &String, note: &String, thread: &mut Thread) {
    thread.status.insert(agent.clone(), (state.clone(), note.clone()));

    // `working`: 0 chat row (the agent card shows a turn as working…; the note says
    // what its next messages say anyway)
    if state == "working" {
        return;
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

    thread.items.push(Item::Note { agent, text, color });
}

fn end_subagent(event: &Event, agent: String, id: &String, state: &str, result: &String, thread: &mut Thread) {
    if let Some(i) = thread.subs.iter().position(|sub| &sub.id == id && sub.running()) {
        let sub = &mut thread.subs[i];

        sub.state = Some(state.to_owned());
        sub.result.clone_from(result);
        sub.end = event.ts;

        // the master's message: its own box ends here (the next text opens a new one)
        thread.open_run.remove(&agent);
        thread.runs += 1;
        thread.items.push(Item::Sub {
            agent,
            sub: i,
            ts: event.ts,
            key: thread.runs,
        });
    }
}

fn last_of_thread(thread: &Thread, agent: &str, chars: usize) -> String {
    let mut out: Vec<&str> = Vec::new();
    let mut taken = 0;

    for item in thread.items.iter().rev() {
        if let ControlFlow::Break(()) = take_text(agent, chars, &mut out, &mut taken, item) {
            break;
        }
    }

    out.reverse();
    out.join("\n\n")
}

fn take_text<'text>(
    agent: &str,
    chars: usize,
    out: &mut Vec<&'text str>,
    taken: &mut usize,
    item: &'text Item,
) -> ControlFlow<()> {
    if let Item::Text {
        agent: author, text, ..
    } = item
        && author == agent
    {
        *taken += text.len();

        if *taken > chars && !out.is_empty() {
            return ControlFlow::Break(());
        }

        out.push(text);
    }

    ControlFlow::Continue(())
}

fn rerun_subagent(event: &Event, id: &String, thread: &mut Thread) {
    let last = thread.subs.iter().rposition(|sub| &sub.id == id);

    if let Some(old) = last.map(|i| &thread.subs[i]).filter(|sub| !sub.running()) {
        let rerun = Sub {
            start: event.ts,
            state: None,
            result: String::new(),
            end: 0,
            resumed: true,
            ..old.clone()
        };

        thread.subs.push(rerun);
    }
}

fn push_prompt(event: &Event, from: &String, to: &Vec<String>, text: &str, thread: &mut Thread) {
    thread.open_run.remove(from);

    // a message to a stopped agent: its turn, 0 auto resume
    for recipient in to {
        thread.resumes.remove(recipient);
    }

    thread.items.push(Item::Prompt {
        from: from.clone(),
        to: to.clone(),
        text: text.to_owned(),
        ts: event.ts,
    });
}

fn push_text(thread: &mut Thread, agent: String, text: String, ts: u64) {
    let thoughts = thread.pending.remove(&agent).unwrap_or_default();

    let key = if thoughts.is_empty() {
        0
    } else {
        thread.thought_keys += 1;
        thread.thought_keys
    };

    thread.items.push(Item::Text {
        agent,
        text,
        ts,
        thoughts,
        key,
    });
}

fn record_event(thread: &mut Thread, event: &Event) {
    let agent = event.agent.clone().unwrap_or_default();

    match &event.kind {
        Kind::Prompt { .. } => record_prompt(thread, event, agent),
        Kind::Text { .. } => record_text(thread, event, agent),
        Kind::Thought { .. } => record_thought(thread, event, agent),
        Kind::Tool { .. } => record_tool(thread, event, agent),
        Kind::ToolResult { .. } => record_tool_result(thread, event, &agent),
        Kind::Ask { .. } => record_ask(thread, event, agent),
        Kind::Answer { .. } => record_answer(thread, event, agent),
        Kind::TurnStart => record_turn_start(thread, event, agent),
        Kind::Resume { .. } => record_resume(thread, event, &agent),
        Kind::TurnEnd { .. } => record_turn_end(thread, event, agent),
        Kind::Session { .. } => record_session(thread, event, agent),
        Kind::Status { .. } => record_status(thread, event, agent),
        Kind::Join => record_join(thread, event, agent),
        Kind::Leave => record_leave(thread, event, agent),
        Kind::Error { .. } => record_error(thread, event, agent),
        Kind::Shell { .. } => record_shell(thread, event, agent),
        // flow lines: 0 chat rows (the agent card shows the step, a click on it the flow box)
        Kind::Flow { .. } => record_flow(thread, event, agent),
        Kind::Edits { .. } => record_edits(thread, event, agent),
        // a running sub-thread: 0 chat rows (the sub-threads row shows it); its end: a message
        // of its agent
        Kind::SubThread { .. } => record_sub_thread(thread, event, agent),
        Kind::SubStart { .. } => record_sub_start(thread, event, agent),
        Kind::SubInfo { .. } => record_sub_info(thread, event, agent),
        Kind::SubEnd { .. } => record_sub_end(thread, event, agent),
        // an ended subagent runs again: a new `Sub` (same id); the old one keeps its report
        // and row, the rerun's end gets its own row
        Kind::SubResume { .. } => record_sub_resume(thread, event, agent),
    }
}

fn push_leave(thread: &mut Thread, agent: String) {
    thread.busy.remove(&agent);
    thread.items.push(Item::Note {
        agent,
        text: "left the thread".into(),
        color: theme::pal().subtext0,
    });
}

fn set_call_output(thread: &mut Thread, event: &Event, agent: &str, id: &String, ok: bool, output: &str) {
    if let Some(i) = thread.open_run.get(agent).copied()
        && let Some(Item::Run { calls, end, .. }) = thread.items.get_mut(i)
        && let Some(call) = calls.iter_mut().find(|call| &call.id == id)
    {
        call.output = Some((ok, output.to_owned()));
        *end = event.ts;
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
