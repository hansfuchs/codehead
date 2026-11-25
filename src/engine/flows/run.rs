//! An agent's flow at run time: the state in `thread.json` (1 per agent of the thread) and its
//! moves. Pure: the host does the I/O (log, delivery, interrupts).

use std::collections::BTreeMap;
use std::fmt::Write as _;

use serde::{Deserialize, Serialize};

use super::{Flow, Library, Resolved, resolve};

/// Why the flow waits on the owner.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum Wait {
    /// A gate node: approve, or another outcome.
    Gate,
    /// A loop reached its max: 1 more round, or move on.
    LoopMax,
}

/// A wait of `thread.json`; an old kind (`role`: roles are gone) reads as 0 wait.
fn lenient_wait<'de, D: serde::Deserializer<'de>>(directory: D) -> Result<Option<Wait>, D::Error> {
    let value = Option::<serde_json::Value>::deserialize(directory)?;

    Ok(value.and_then(|value_value| serde_json::from_value(value_value).ok()))
}

/// A switch to flow FLOW at node START.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Switch {
    pub flow: String,
    #[serde(default)]
    pub start: usize,
}

/// A node that ended: how, when.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Past {
    pub node: usize,
    pub title: String,
    /// `done`, `skipped`, `approved`, `passed by agent`, `loop`, outcome label, `left`.
    pub how: String,
    pub ms: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct FlowState {
    /// Flow slug.
    pub flow: String,
    /// Its name when set (the flow file can go).
    pub title: String,
    /// Current node.
    pub node: usize,
    /// Loop rounds per node id (the node that holds the loop).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub rounds: BTreeMap<String, u32>,
    #[serde(default, deserialize_with = "lenient_wait", skip_serializing_if = "Option::is_none")]
    pub wait: Option<Wait>,
    /// `after`: the switch once this flow ends.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queued: Option<Switch>,
    /// `now`: the switch at the next tool boundary of a busy agent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending: Option<Switch>,
    /// The current node's text went to its agent.
    #[serde(default)]
    pub sent: bool,
    #[serde(default)]
    pub node_started: u64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub history: Vec<Past>,
    /// The last node ended.
    #[serde(default)]
    pub finished: bool,
}

/// What a step-done report did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Moved {
    /// To node N.
    To(usize),
    Wait(Wait),
    Finished,
}

impl FlowState {
    /// A new state for flow F at node START.
    pub(crate) fn new(file: &Flow, lib: &Library, start: usize, now: u64) -> FlowState {
        let nodes = resolve(file, lib);

        FlowState {
            flow: file.slug.clone(),
            title: file.name.clone(),
            node: start.min(nodes.len().saturating_sub(1)),
            node_started: now,
            ..Default::default()
        }
    }

    pub(crate) fn round(&self, id: &str) -> u32 {
        self.rounds.get(id).copied().unwrap_or(0)
    }

    fn leave(&mut self, nodes: &[Resolved], how: &str, now: u64) {
        if let Some(count) = nodes.get(self.node) {
            self.history.push(Past {
                node: self.node,
                title: count.title.clone(),
                how: how.into(),
                ms: now.saturating_sub(self.node_started),
            });

            // keep the file small: the last 200 nodes
            trim_history(self);
        }
    }

    fn enter(&mut self, to: usize, now: u64) -> Moved {
        self.node = to;
        self.node_started = now;
        self.sent = false;
        self.wait = None;
        Moved::To(to)
    }

    fn next(&mut self, nodes: &[Resolved], now: u64) -> Moved {
        if self.node + 1 >= nodes.len() {
            self.finished = true;
            self.wait = None;
            return Moved::Finished;
        }

        self.enter(self.node + 1, now)
    }

    /// The agent reports the current node done. LOOP: the loop's exit criteria are not met (go
    /// back). PASS: an autopilot agent passes the gate itself.
    pub(crate) fn step_done(&mut self, nodes: &[Resolved], again: bool, pass: bool, now: u64) -> Moved {
        let Some(count) = nodes.get(self.node).cloned() else {
            self.finished = true;
            return Moved::Finished;
        };

        if let Some(width) = &self.wait {
            return Moved::Wait(width.clone());
        }

        if let (true, Some((to, _, max))) = (again, &count.loops) {
            return repeat_node(nodes, &count, *to, *max, now, self);
        }

        if count.gate && !pass {
            self.wait = Some(Wait::Gate);
            return Moved::Wait(Wait::Gate);
        }

        self.leave(nodes, if count.gate { "passed by agent" } else { "done" }, now);

        self.next(nodes, now)
    }

    /// The owner's answer to the wait: gate `approve` or an outcome label; loop max `again` or
    /// `next`. Err: 0 such choice.
    pub(crate) fn answer(&mut self, nodes: &[Resolved], choice: &str, now: u64) -> Result<Moved, String> {
        let count = nodes.get(self.node).cloned().ok_or("0 current step")?;

        match self.wait.clone() {
            Some(Wait::Gate) => answer_gate(nodes, choice, now, &count, self),
            Some(Wait::LoopMax) => answer_loop_limit(nodes, choice, now, &count, self),
            None => Err("the flow waits on 0 answer".into()),
        }
    }

    pub(crate) fn skip(&mut self, nodes: &[Resolved], now: u64) -> Moved {
        self.leave(nodes, "skipped", now);
        self.next(nodes, now)
    }

    pub(crate) fn jump(&mut self, nodes: &[Resolved], to: usize, now: u64) -> Result<Moved, String> {
        if to >= nodes.len() {
            return Err(format!("0 step {}", to + 1));
        }

        self.finished = false;
        self.leave(nodes, "left", now);
        Ok(self.enter(to, now))
    }
}

/// The message an agent gets at the start of node I.
pub(crate) fn step_message(title: &str, nodes: &[Resolved], i: usize, st: &FlowState, task: Option<&str>) -> String {
    let count = &nodes[i];
    let mut text_value = format!(
        "[codehead flow: {title} · step {}/{}: {}]\n\n",
        i + 1,
        nodes.len(),
        count.title
    );

    text_value.push_str(&crate::domain::shortcuts::expand(&count.text));

    if !count.done.is_empty() {
        let _ = write!(text_value, "\n\nDone when: {}", count.done);
    }

    if let Some((to, until, max)) = &count.loops {
        let result = st.round(&count.id) + 1;

        let _ = write!(
            text_value,
            "\n\nLoop (round {result}/{max}): when this step is done, check: {until}. Not met: `flow_step` with `again: true` (back to step {}: {}). Met: `flow_step` without it.",
            to + 1,
            nodes[*to].title
        );
    }

    if count.gate {
        text_value.push_str("\n\nGate: after this step, the owner approves before the flow goes on. Call `flow_step` when the step is done; then wait for the owner.");
    }

    text_value.push_str("\n\nWhen the step is done: call `flow_step` (codehead MCP). Do not start the next step before it: codehead gives you the next step.");

    // the 1st step of a flow: the whole flow, so the agent knows what comes later
    if st.history.is_empty() && i == st.node {
        let _ = write!(
            text_value,
            "\n\nThe flow (▶ = this step; later steps come one at a time):\n{}",
            outline(nodes, st)
        );
    }

    if let Some(thread_value) = task.filter(|thread| !thread.trim().is_empty()) {
        let _ = write!(text_value, "\n\n---\n\n{thread_value}");
    }

    text_value
}

/// The flow outline: 1 line per node, the current one marked.
pub(crate) fn outline(nodes: &[Resolved], st: &FlowState) -> String {
    nodes
        .iter()
        .enumerate()
        .map(|(i, count)| {
            let mark = if i == st.node && !st.finished {
                "▶"
            } else if st.history.iter().any(|path| path.node == i) {
                "✓"
            } else {
                " "
            };

            let mut line = format!("{mark} {}. {}", i + 1, count.title);

            if count.gate {
                line.push_str(" [gate]");
            }

            if let Some((to, _, max)) = &count.loops {
                let _ = write!(line, " [loop → {}, max {max}]", to + 1);
            }

            line
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Problems of the nodes for AGENT (the flow's agent): needs it lacks, node errors. (node index,
/// text). DIR: the work dir (skills of the project).
pub(crate) fn warnings(
    nodes: &[Resolved],
    agent: &str,
    roster: &[crate::engine::roster::Agent],
    dir: &std::path::Path,
) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let mut skills: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let agent_2 = roster.iter().find(|agent_value| agent_value.name == agent);

    for (i, count) in nodes.iter().enumerate() {
        for error in &count.errors {
            out.push((i, error.clone()));
        }

        let Some(agent_3) = agent_2 else { continue };

        for need in &count.needs {
            let (key, value) = need.split_once(':').unwrap_or(("skill", need));

            let ok = match key {
                "skill" => handle_skill(dir, &mut skills, agent_3, value),
                "mcp" => agent_3.mcp.iter().any(|message| message == value),
                "harness" => agent_3.harness.split('/').next() == Some(value),
                _ => true,
            };

            append_warning(agent, &mut out, i, agent_3, key, value, ok);
        }
    }

    out
}

fn append_warning(
    agent: &str,
    out: &mut Vec<(usize, String)>,
    i: usize,
    agent_3: &crate::engine::roster::Agent,
    key: &str,
    value: &str,
    ok: bool,
) {
    if !ok {
        out.push((i, format!("@{agent} ({}): 0 {key} {value}", agent_3.harness)));
    }
}

fn handle_skill(
    dir: &std::path::Path,
    skills: &mut BTreeMap<String, Vec<String>>,
    agent_3: &crate::engine::roster::Agent,
    value: &str,
) -> bool {
    if agent_3.skills.is_empty() {
        let height = agent_3.harness.split('/').next().unwrap_or_default().to_string();

        let list = skills.entry(height.clone()).or_insert_with(|| {
            crate::infra::skills::load(std::slice::from_ref(&height), dir)
                .into_iter()
                .map(|text| text.name)
                .collect()
        });

        list.iter()
            .any(|text_value| text_value == value || text_value.ends_with(&format!(":{value}")))
    } else {
        agent_3
            .skills
            .iter()
            .any(|text_2| text_2 == value || text_2.ends_with(&format!(":{value}")))
    }
}

#[cfg(test)]
#[path = "run_tests.rs"]
mod tests;

/// The tool name of a flow wait shown as an ask (the question picker answers it).
pub(crate) const WAIT_TOOL: &str = "FlowWait";

/// The choice of a flow wait that sends the prompt's text as a message: the wait stays.
pub(crate) const REPLY: &str = "__reply";

fn trim_history(model_state: &mut FlowState) {
    if model_state.history.len() > 200 {
        model_state.history.remove(0);
    }
}

fn answer_loop_limit(
    nodes: &[Resolved],
    choice: &str,
    now: u64,
    count: &Resolved,
    model_state: &mut FlowState,
) -> Result<Moved, String> {
    let (to, _, _) = count.loops.clone().ok_or("0 loop")?;

    match choice {
        "again" => {
            *model_state.rounds.entry(count.id.clone()).or_default() += 1;
            model_state.leave(nodes, "loop", now);
            Ok(model_state.enter(to, now))
        }
        "next" => {
            model_state.leave(nodes, "done", now);
            Ok(model_state.next(nodes, now))
        }
        character => Err(format!("loop: 0 choice {character} (again, next)")),
    }
}

fn answer_gate(
    nodes: &[Resolved],
    choice: &str,
    now: u64,
    count: &Resolved,
    model_state: &mut FlowState,
) -> Result<Moved, String> {
    if choice == "approve" {
        model_state.leave(nodes, "approved", now);
        return Ok(model_state.next(nodes, now));
    }

    let (label, to) = count
        .edges
        .iter()
        .find(|(key, _)| key == choice)
        .cloned()
        .ok_or_else(|| format!("gate: 0 outcome {choice}"))?;

    model_state.leave(nodes, &label, now);
    Ok(model_state.enter(to, now))
}

fn repeat_node(
    nodes: &[Resolved],
    node: &Resolved,
    target: usize,
    maximum: u32,
    now: u64,
    state: &mut FlowState,
) -> Moved {
    let round = state.round(&node.id);

    if round + 1 >= maximum {
        state.wait = Some(Wait::LoopMax);
        return Moved::Wait(Wait::LoopMax);
    }

    state.rounds.insert(node.id.clone(), round + 1);
    state.leave(nodes, "loop", now);
    state.enter(target, now)
}
