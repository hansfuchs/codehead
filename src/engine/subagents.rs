//! Subagents of the agents on a thread, host side. A subagent starts with its master's task
//! tool call (Claude Code `Agent` / `Task`, an ACP agent's task tool). Claude Code keeps a file per
//! subagent next to the session (`<session>/subagents/agent-<id>.jsonl` + `.meta.json`): its
//! steps, nested subagents, the end of a background subagent and its report come from there.
//! The tracker turns all of it into `sub_start` / `sub_info` / `sub_end` events of the log.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::store::{Event, Kind};

/// A session's `subagents` dir: found again after this (a resume in another dir moves it).
const REDIR: std::time::Duration = std::time::Duration::from_secs(60);

/// A task tool: its call starts a subagent.
pub(crate) fn is_sub_tool(name: &str) -> bool {
    matches!(name, "Agent" | "Task")
        || matches!(
            name.to_ascii_lowercase().as_str(),
            "task" | "subagent" | "spawn_agent" | "delegate_task" | "dispatch_agent"
        )
}

/// A running subagent.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Run {
    /// Its master: the codehead agent.
    pub agent: String,
    pub description: String,
    pub background: bool,
    /// Started by another subagent (it ends with its meta file, not a tool result of the master).
    pub nested: bool,
    /// Claude Code: its transcript, its agent id (the task id of `TaskStop`).
    pub transcript: Option<PathBuf>,
    pub task: String,
    /// Resumed at (unix ms; 0: never): a `finished` meta file older than this is its last end.
    pub resumed: u64,
}

/// An ended subagent whose files changed this long (ms) after its end: resumed.
const RESUME_GAP: u64 = 2_000;

/// A resumed subagent: its meta file stays `finished` and its transcript unchanged this long
/// (ms): the resume never ran (it ends again).
const RESUME_WAIT: u64 = 30_000;

/// Ended subagents this old (ms) count as done for good: 0 scans for a resume.
const RESUMABLE: u64 = 24 * 60 * 60 * 1000;

/// Events to log: (master, kind).
pub(crate) type Logs = Vec<(String, Kind)>;

#[derive(Default)]
pub(crate) struct Tracker {
    /// By id (the tool call that started it).
    pub running: HashMap<String, Run>,
    /// Ended ids: a later scan never starts them again.
    ended: HashSet<String>,
    /// Ended ones, by id: (the run, its end in unix ms). A message of the master resumes them.
    done: HashMap<String, (Run, u64)>,
    /// Claude Code session id → its `subagents` dir, when found (found again after `REDIR`).
    dirs: HashMap<String, (PathBuf, std::time::Instant)>,
    /// Meta files older than this (ms) belong to earlier processes.
    since: u64,
}

fn cut(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_string();
    }

    let mut end = max;

    while !text.is_char_boundary(end) {
        end -= 1;
    }

    format!("{}\n…", &text[..end])
}

/// What a subagent does, from its tool input: the description, else the title, else the 1st line of the prompt.
fn description_of(input: &Value) -> String {
    let path = input["prompt"].as_str().unwrap_or_default();
    let directory = input["description"]
        .as_str()
        .or_else(|| input["title"].as_str())
        .filter(|text| !text.trim().is_empty())
        .unwrap_or_else(|| path.lines().next().unwrap_or("subagent"));

    directory
        .chars()
        .map(|character| if character.is_control() { ' ' } else { character })
        .take(120)
        .collect()
}

/// `agentId: <id>` in a tool result (Claude Code's background launch).
fn agent_id_in(output: &str) -> Option<String> {
    let at = output.find("agentId:")? + "agentId:".len();
    let id: String = output[at..]
        .trim_start()
        .chars()
        .take_while(|character| character.is_ascii_alphanumeric() || *character == '-' || *character == '_')
        .collect();

    (!id.is_empty()).then_some(id)
}

/// OUTPUT is the launch of a background subagent (Claude Code: `Async agent launched
/// successfully … agentId: …`), not its report.
fn async_launch(output: &str) -> bool {
    let head: String = output.chars().take(400).collect();

    head.contains("Async agent launched") || (agent_id_in(&head).is_some() && head.contains("in the background"))
}

impl Tracker {
    /// The tracker of a host that starts now, from the thread's log: subagents the log has open
    /// ran in a process that is gone: they end as lost.
    pub(crate) fn from_log(events: &[Event]) -> (Tracker, Logs) {
        let mut thread = Tracker {
            since: super::now_ms(),
            ..Default::default()
        };
        let mut open: Vec<(String, String)> = Vec::new();
        let mut runs: HashMap<String, Run> = HashMap::new();

        replay_subagent_events(events, &mut thread, &mut open, &mut runs);

        let logs = open
            .into_iter()
            .map(|(agent_value, id)| {
                thread.ended.insert(id.clone());

                restore_ended_subagent(&mut thread, &mut runs, &id);

                (
                    agent_value,
                    Kind::SubEnd {
                        id,
                        state: "lost".into(),
                        result: "codehead's thread host restarted: the harness process that ran it is gone".into(),
                    },
                )
            })
            .collect();

        (thread, logs)
    }
    /// Subagents run.
    pub(crate) fn any(&self) -> bool {
        !self.running.is_empty()
    }
    /// Masters with running subagents.
    pub(crate) fn masters(&self) -> HashSet<String> {
        self.running.values().map(|result| result.agent.clone()).collect()
    }
    /// Ended subagents with files (Claude Code), ended in the last RESUMABLE: a message of their
    /// master can resume them.
    pub(crate) fn resumable(&self) -> bool {
        let since = super::now_ms().saturating_sub(RESUMABLE);

        self.done
            .values()
            .any(|(result, end)| result.transcript.is_some() && *end >= since)
    }
    /// ID ends: out of the running ones, kept with its end time for a resume.
    fn end(&mut self, id: &str) -> Option<Run> {
        let result = self.running.remove(id)?;

        self.ended.insert(id.to_string());
        self.done.insert(id.to_string(), (result.clone(), super::now_ms()));
        Some(result)
    }
    /// Ended subagent ID runs again. It ends with its meta file (0 tool result of its master).
    fn resume(&mut self, id: &str) -> Logs {
        let Some((mut result, _)) = self.done.remove(id) else {
            return Vec::new();
        };

        self.ended.remove(id);
        result.resumed = super::now_ms();
        result.background = true;
        let agent = result.agent.clone();

        self.running.insert(id.to_string(), result);

        vec![(agent, Kind::SubResume { id: id.to_string() })]
    }
    /// An event of AGENT: a task tool call starts a subagent; its result ends it (a background
    /// one: its result is the launch, the meta file says when it ends).
    pub(crate) fn event(&mut self, agent: &str, kind: &Kind) -> Logs {
        match kind {
            // a task tool, or any tool call with a subagent type (ACP agents name their tools freely)
            Kind::Tool { id, name, input }
                if (is_sub_tool(name) || input["subagent_type"].is_string())
                    && !self.running.contains_key(id)
                    && !self.ended.contains(id) =>
            {
                let description = description_of(input);
                let background = input["run_in_background"] == true;

                self.running.insert(
                    id.clone(),
                    Run {
                        agent: agent.into(),
                        description: description.clone(),
                        background,
                        ..Default::default()
                    },
                );
                let prompt = cut(input["prompt"].as_str().unwrap_or_default(), 2048);
                let agent_type = input["subagent_type"].as_str().unwrap_or_default().to_string();

                vec![(
                    agent.into(),
                    Kind::SubStart {
                        id: id.clone(),
                        parent: None,
                        description,
                        agent_type,
                        background,
                        prompt,
                    },
                )]
            }
            Kind::ToolResult { id, ok, output }
                if self
                    .running
                    .get(id)
                    .is_some_and(|result_value| result_value.agent == agent) =>
            {
                record_task_result(agent, id, *ok, output, self)
            }
            // a message to an ended subagent (Claude Code `SendMessage`, `to`: its agent id) resumes it
            Kind::Tool { name, input, .. } if name == "SendMessage" => {
                let to = input["to"].as_str().unwrap_or_default();
                let id = self
                    .done
                    .iter()
                    .find(|(_, (result_4, _))| result_4.agent == agent && !to.is_empty() && result_4.task == to)
                    .map(|(id, _)| id.clone());

                id.map(|id| self.resume(&id)).unwrap_or_default()
            }
            _ => Vec::new(),
        }
    }

    /// The harness process of AGENT ended (WHY): its running subagents are lost.
    pub(crate) fn exit(&mut self, agent: &str, why: &str) -> Logs {
        let ids: Vec<String> = self
            .running
            .iter()
            .filter(|(_, result_value)| result_value.agent == agent)
            .map(|(id, _)| id.clone())
            .collect();

        ids.into_iter()
            .map(|id| {
                self.end(&id);
                (
                    agent.to_string(),
                    Kind::SubEnd {
                        id,
                        state: "lost".into(),
                        result: why.to_string(),
                    },
                )
            })
            .collect()
    }
    /// The owner stops ID: it leaves the tracker (its end is logged by the caller).
    pub(crate) fn stop(&mut self, id: &str) -> Option<Run> {
        self.end(id)
    }
}

fn replay_subagent_events(
    events: &[Event],
    thread: &mut Tracker,
    open: &mut Vec<(String, String)>,
    runs: &mut HashMap<String, Run>,
) {
    for error in events {
        let agent = error.agent.clone().unwrap_or_default();

        match &error.kind {
            Kind::SubStart {
                id,
                parent,
                description,
                background,
                ..
            } => {
                from_log_substart(open, runs, &agent, id, parent.as_deref(), description, *background);
            }
            Kind::SubInfo { id, transcript, task } => {
                record_transcript(runs, id, transcript, task);
            }
            Kind::SubEnd { id, .. } => {
                from_log_subend_id(thread, open, &*runs, error, id);
            }
            Kind::SubResume { id } => {
                restore_resumed_subagent(thread, open, agent, id);
            }
            _ => {}
        }
    }
}

fn restore_resumed_subagent(thread: &mut Tracker, open: &mut Vec<(String, String)>, agent: String, id: &String) {
    thread.ended.remove(id);
    thread.done.remove(id);

    if !open.iter().any(|(_, x)| x == id) {
        open.push((agent, id.clone()));
    }
}

fn from_log_substart(
    open: &mut Vec<(String, String)>,
    runs: &mut HashMap<String, Run>,
    agent: &str,
    id: &str,
    parent: Option<&str>,
    description: &str,
    background: bool,
) {
    open.push((agent.to_owned(), id.to_owned()));
    runs.insert(
        id.to_owned(),
        Run {
            agent: agent.to_owned(),
            description: description.to_owned(),
            background,
            nested: parent.is_some(),
            ..Default::default()
        },
    );
}

fn restore_ended_subagent(thread: &mut Tracker, runs: &mut HashMap<String, Run>, id: &String) {
    if let Some(result_3) = runs.remove(id) {
        thread.done.insert(id.clone(), (result_3, thread.since));
    }
}

fn from_log_subend_id(
    thread: &mut Tracker,
    open: &mut Vec<(String, String)>,
    runs: &HashMap<String, Run>,
    error: &Event,
    id: &String,
) {
    thread.ended.insert(id.clone());
    open.retain(|(_, x)| x != id);

    if let Some(result_2) = runs.get(id) {
        thread.done.insert(id.clone(), (result_2.clone(), error.ts));
    }
}

fn record_transcript(runs: &mut HashMap<String, Run>, id: &String, transcript: &String, task: &String) {
    if let Some(result_value) = runs.get_mut(id) {
        if !transcript.is_empty() {
            result_value.transcript = Some(PathBuf::from(transcript));
        }

        if !task.is_empty() {
            result_value.task.clone_from(task);
        }
    }
}

#[cfg(test)]
#[path = "subagents_tests.rs"]
mod tests;

#[path = "subagent_transcripts.rs"]
mod subagent_transcripts;
pub(crate) use subagent_transcripts::*;

#[path = "subagent_scan.rs"]
mod subagent_scan;

fn record_task_result(
    agent: &str,
    id: &String,
    ok: bool,
    output: &str,
    model_state: &mut Tracker,
) -> Vec<(String, Kind)> {
    let result_2 = model_state.running.get_mut(id).expect("checked");

    // Claude Code starts subagents in the background by default (0 `run_in_background`
    // in the input): the result is the launch, not the end
    if ok && !result_2.background && async_launch(output) {
        result_2.background = true;
    }

    if result_2.background && ok {
        if let Some(task) = agent_id_in(output).filter(|thread| *thread != result_2.task) {
            result_2.task.clone_from(&task);
            let transcript = result_2
                .transcript
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_default();

            return vec![(
                agent.into(),
                Kind::SubInfo {
                    id: id.clone(),
                    transcript,
                    task,
                },
            )];
        }

        return Vec::new();
    }

    let result_3 = model_state.end(id).expect("checked");
    let result = result_3
        .transcript
        .as_deref()
        .and_then(result_of)
        .filter(|_| ok)
        .unwrap_or_else(|| output.to_owned());

    vec![(
        agent.into(),
        Kind::SubEnd {
            id: id.clone(),
            state: if ok { "done" } else { "failed" }.into(),
            result,
        },
    )]
}
