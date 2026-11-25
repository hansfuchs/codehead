use super::{Ev, Event, HashMap, Kind, Note, store};

type DerivedNote = (Kind, Vec<String>, u64, String);

#[derive(Default)]
struct AgentNotes {
    status: Option<(String, String, u64)>,
    asks: Vec<(String, u64, String)>,
    error: Option<(u64, String)>,
    limit: Option<(u64, u64)>,
    context: Option<(u64, u64, u64)>,
    mention: Option<(u64, String)>,
}

#[derive(Default)]
struct EventNotes {
    agents: HashMap<String, AgentNotes>,
    gate: Option<(u64, String)>,
}

#[derive(Default)]
struct NoteGroups {
    notes: Vec<DerivedNote>,
}

pub(crate) fn derive(events: &[Event], members: &[String]) -> Vec<DerivedNote> {
    let mut notes = EventNotes::default();

    for event in events {
        notes.event(event);
    }

    notes.finish(members)
}

impl EventNotes {
    fn event(&mut self, event: &Event) {
        let name = event.agent.clone().unwrap_or_default();

        match &event.kind {
            Ev::Prompt { from, .. } if from == "owner" => clear_errors_and_mentions(self),
            Ev::Prompt { from, to, text } if to.iter().any(|agent| agent == "owner") || mentions_owner(text) => {
                self.mention(from, event.ts, text);
            }
            Ev::Text { text } if mentions_owner(text) => self.mention(&name, event.ts, text),
            Ev::Ask { id, input, .. } => self.ask(name, event.ts, id, input),
            Ev::Answer { id, .. } => self.answer(id),
            Ev::TurnStart => self.turn_start(name),
            Ev::TurnEnd { error, ctx, window, .. } => {
                self.agents
                    .entry(name)
                    .or_default()
                    .turn_end(event.ts, error.as_deref(), *ctx, *window);
            }
            Ev::Error { message } if !crate::domain::chat::usage_limit(message) => {
                self.agents.entry(name).or_default().error = Some((event.ts, first_line(message)));
            }
            Ev::Resume { at } => self.agents.entry(name).or_default().limit = (*at > 0).then_some((event.ts, *at)),
            Ev::Status { state, note } => {
                self.agents.entry(name).or_default().status = Some((state.clone(), note.clone(), event.ts));
            }
            Ev::Leave => {
                self.agents.remove(&name);
            }
            Ev::Flow { op, text } => self.flow(op, text, event.ts),
            _ => {}
        }
    }

    fn mention(&mut self, name: &str, timestamp: u64, text: &str) {
        self.agents.entry(name.to_string()).or_default().mention = Some((timestamp, first_line(text)));
    }

    fn ask(&mut self, name: String, timestamp: u64, id: &str, input: &serde_json::Value) {
        let question = input["questions"][0]["question"]
            .as_str()
            .or(input["question"].as_str())
            .map_or_else(|| "permission request".into(), first_line);

        self.agents
            .entry(name)
            .or_default()
            .asks
            .push((id.to_string(), timestamp, question));
    }

    fn answer(&mut self, id: &str) {
        for agent in self.agents.values_mut() {
            agent.asks.retain(|ask| ask.0 != id);
        }
    }

    fn turn_start(&mut self, name: String) {
        let agent = self.agents.entry(name).or_default();

        agent.status = None;
        agent.limit = None;
    }

    fn flow(&mut self, operation: &str, text: &str, timestamp: u64) {
        match operation {
            "wait" => self.gate = Some((timestamp, text.to_string())),
            "pending" | "queued" | "role" => {}
            _ => self.gate = None,
        }
    }

    fn finish(self, members: &[String]) -> Vec<DerivedNote> {
        let mut groups = NoteGroups::default();
        let mut names: Vec<&String> = self.agents.keys().filter(|name| !name.is_empty()).collect();

        names.sort();

        for name in names {
            self.agents[name].collect(name, &mut groups);
        }

        if let Some((timestamp, text)) = &self.gate {
            groups.notes.push((Kind::Gate, Vec::new(), *timestamp, text.clone()));
        }

        if let Some(done) = self.done(members) {
            groups.notes.push(done);
        }

        groups.notes.sort_by_key(|note| note.0);

        groups.notes
    }

    fn done(&self, members: &[String]) -> Option<DerivedNote> {
        if members.is_empty() || !members.iter().all(|name| agent_done(name, self)) {
            return None;
        }

        let timestamp = members
            .iter()
            .filter_map(|name| self.agents[name].status.as_ref().map(|status| status.2))
            .max()
            .unwrap_or(0);

        let text = members
            .iter()
            .filter_map(|name| {
                self.agents[name]
                    .status
                    .as_ref()
                    .filter(|status| !status.1.is_empty())
                    .map(|status| first_line(&status.1))
            })
            .next_back()
            .unwrap_or_else(|| "all agents done".into());

        Some((Kind::Done, members.to_vec(), timestamp, text))
    }
}

impl AgentNotes {
    fn turn_end(&mut self, timestamp: u64, error: Option<&str>, context: u64, window: u64) {
        self.asks.clear();

        if window > 0 {
            self.context = Some((timestamp, context, window));
        }

        match error {
            Some(message) if crate::domain::chat::usage_limit(message) => {}
            Some(message) if message != crate::engine::host::INTERRUPTED && message != "session ended" => {
                self.error = Some((timestamp, first_line(message)));
            }
            _ => self.error = None,
        }
    }

    fn collect(&self, name: &str, groups: &mut NoteGroups) {
        if let Some((_, timestamp, question)) = self.asks.last() {
            groups.push(Kind::Ask, name, *timestamp, &question.clone());
        }

        if let Some((timestamp, text)) = status_note("blocked", "blocked", self) {
            groups.push(Kind::Blocked, name, timestamp, &text);
        }

        if let Some((timestamp, text)) = status_note("review", "step done", self) {
            groups.push(Kind::Step, name, timestamp, &text);
        }

        if let Some((timestamp, error)) = &self.error {
            groups.push(Kind::Error, name, *timestamp, &error.clone());
        }

        if let Some((timestamp, reset)) = self.limit {
            groups.push(
                Kind::Limit,
                name,
                timestamp,
                &format!("resumes {}", crate::domain::chat::clock(reset)),
            );
        }

        if let Some((timestamp, text)) = &self.mention {
            groups.push(Kind::Mention, name, *timestamp, &text.clone());
        }

        if let Some((timestamp, context, window)) = self.context
            && context * 10 >= window * 9
        {
            groups.push(
                Kind::Context,
                name,
                timestamp,
                &format!("context {} %", context * 100 / window),
            );
        }
    }
}

impl NoteGroups {
    fn push(&mut self, kind: Kind, name: &str, timestamp: u64, text: &str) {
        let Some(note) = self.notes.iter_mut().find(|note| note.0 == kind) else {
            self.notes
                .push((kind, vec![name.to_string()], timestamp, format!("{name}: {text}")));
            return;
        };

        note.1.push(name.to_string());

        if timestamp > note.2 {
            note.2 = timestamp;
            note.3 = format!("{name}: {text}");
        }
    }
}

pub(crate) fn mentions_owner(text: &str) -> bool {
    text.match_indices("@owner").any(|(i, _)| {
        !text[i + 6..]
            .starts_with(|character: char| character.is_alphanumeric() || character == '_' || character == '-')
    })
}
pub(crate) fn first_line(text: &str) -> String {
    text.lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or_default()
        .chars()
        .take(160)
        .collect()
}
/// The reply notes of thread T: 1 per comment with unseen agent replies (its latest).
pub(crate) fn reply_notes(thread: &store::Thread) -> Vec<Note> {
    crate::domain::changes::threads::unseen_replies(&thread.id)
        .into_iter()
        .map(|(comment, ts, by, text)| Note {
            thread: thread.id.clone(),
            title: thread.title.clone(),
            dir: thread.work_dir(),
            kind: Kind::Reply,
            agents: vec![by],
            ts,
            text,
            comment,
        })
        .collect()
}

fn clear_errors_and_mentions(notes: &mut EventNotes) {
    for agent in notes.agents.values_mut() {
        agent.error = None;
        agent.mention = None;
    }
}

fn agent_done(name: &str, notes: &EventNotes) -> bool {
    notes
        .agents
        .get(name)
        .and_then(|agent| agent.status.as_ref())
        .is_some_and(|status| status.0 == "done")
}

fn status_note(wanted: &str, fallback: &str, agent: &AgentNotes) -> Option<(u64, String)> {
    let (state, note, timestamp) = agent.status.as_ref()?;

    if state != wanted {
        return None;
    }

    let text = if note.is_empty() {
        fallback.to_string()
    } else {
        first_line(note)
    };

    Some((*timestamp, text))
}
