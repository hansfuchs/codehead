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
fn lenient_wait<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Option<Wait>, D::Error> {
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;

    Ok(value.and_then(|value| serde_json::from_value(value).ok()))
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
    /// A new state for flow FLOW at node START.
    pub(crate) fn new(flow: &Flow, lib: &Library, start: usize, now: u64) -> FlowState {
        let nodes = resolve(flow, lib);

        FlowState {
            flow: flow.slug.clone(),
            title: flow.name.clone(),
            node: start.min(nodes.len().saturating_sub(1)),
            node_started: now,
            ..Default::default()
        }
    }

    pub(crate) fn round(&self, id: &str) -> u32 {
        self.rounds.get(id).copied().unwrap_or(0)
    }

    fn leave(&mut self, nodes: &[Resolved], how: &str, now: u64) {
        if let Some(node) = nodes.get(self.node) {
            self.history.push(Past {
                node: self.node,
                title: node.title.clone(),
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

    /// The agent reports the current node done. AGAIN: the loop's exit criteria are not met (go
    /// back). PASS: an autopilot agent passes the gate itself.
    pub(crate) fn step_done(&mut self, nodes: &[Resolved], again: bool, pass: bool, now: u64) -> Moved {
        let Some(node) = nodes.get(self.node).cloned() else {
            self.finished = true;
            return Moved::Finished;
        };

        if let Some(wait) = &self.wait {
            return Moved::Wait(wait.clone());
        }

        if let (true, Some((to, _, max))) = (again, &node.loops) {
            return repeat_node(nodes, &node, *to, *max, now, self);
        }

        if node.gate && !pass {
            self.wait = Some(Wait::Gate);
            return Moved::Wait(Wait::Gate);
        }

        self.leave(nodes, if node.gate { "passed by agent" } else { "done" }, now);

        self.next(nodes, now)
    }

    /// The owner's answer to the wait: gate `approve` or an outcome label; loop max `again` or
    /// `next`. Err: 0 such choice.
    pub(crate) fn answer(&mut self, nodes: &[Resolved], choice: &str, now: u64) -> Result<Moved, String> {
        let node = nodes.get(self.node).cloned().ok_or("0 current step")?;

        match self.wait.clone() {
            Some(Wait::Gate) => answer_gate(nodes, choice, now, &node, self),
            Some(Wait::LoopMax) => answer_loop_limit(nodes, choice, now, &node, self),
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
pub(crate) fn step_message(title: &str, nodes: &[Resolved], i: usize, state: &FlowState, task: Option<&str>) -> String {
    let node = &nodes[i];
    let mut message = format!(
        "[codehead flow: {title} · step {}/{}: {}]\n\n",
        i + 1,
        nodes.len(),
        node.title
    );

    message.push_str(&crate::domain::shortcuts::expand(&node.text));

    if !node.done.is_empty() {
        let _ = write!(message, "\n\nDone when: {}", node.done);
    }

    if let Some((to, until, max)) = &node.loops {
        let round = state.round(&node.id) + 1;

        let _ = write!(
            message,
            "\n\nLoop (round {round}/{max}): when this step is done, check: {until}. Not met: `flow_step` with `again: true` (back to step {}: {}). Met: `flow_step` without it.",
            to + 1,
            nodes[*to].title
        );
    }

    if node.gate {
        message.push_str("\n\nGate: after this step, the owner approves before the flow goes on. Call `flow_step` when the step is done; then wait for the owner.");
    }

    message.push_str("\n\nWhen the step is done: call `flow_step` (codehead MCP). Do not start the next step before it: codehead gives you the next step.");

    // the 1st step of a flow: the whole flow, so the agent knows what comes later
    if state.history.is_empty() && i == state.node {
        let _ = write!(
            message,
            "\n\nThe flow (▶ = this step; later steps come one at a time):\n{}",
            outline(nodes, state)
        );
    }

    if let Some(task) = task.filter(|task| !task.trim().is_empty()) {
        let _ = write!(message, "\n\n---\n\n{task}");
    }

    message
}

/// The flow outline: 1 line per node, the current one marked.
pub(crate) fn outline(nodes: &[Resolved], state: &FlowState) -> String {
    nodes
        .iter()
        .enumerate()
        .map(|(i, node)| {
            let mark = if i == state.node && !state.finished {
                "▶"
            } else if state.history.iter().any(|past| past.node == i) {
                "✓"
            } else {
                " "
            };

            let mut line = format!("{mark} {}. {}", i + 1, node.title);

            if node.gate {
                line.push_str(" [gate]");
            }

            if let Some((to, _, max)) = &node.loops {
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
    let found = roster.iter().find(|candidate| candidate.name == agent);

    for (i, node) in nodes.iter().enumerate() {
        for error in &node.errors {
            out.push((i, error.clone()));
        }

        let Some(roster_agent) = found else { continue };

        for need in &node.needs {
            let (key, value) = need.split_once(':').unwrap_or(("skill", need));

            let ok = match key {
                "skill" => has_skill(dir, &mut skills, roster_agent, value),
                "mcp" => roster_agent.mcp.iter().any(|server| server == value),
                "harness" => roster_agent.harness.split('/').next() == Some(value),
                _ => true,
            };

            append_warning(agent, &mut out, i, roster_agent, key, value, ok);
        }
    }

    out
}

fn append_warning(
    agent: &str,
    out: &mut Vec<(usize, String)>,
    i: usize,
    roster_agent: &crate::engine::roster::Agent,
    key: &str,
    value: &str,
    ok: bool,
) {
    if !ok {
        out.push((i, format!("@{agent} ({}): 0 {key} {value}", roster_agent.harness)));
    }
}

fn has_skill(
    dir: &std::path::Path,
    skills: &mut BTreeMap<String, Vec<String>>,
    roster_agent: &crate::engine::roster::Agent,
    value: &str,
) -> bool {
    if roster_agent.skills.is_empty() {
        let harness = roster_agent.harness.split('/').next().unwrap_or_default().to_string();

        let list = skills.entry(harness.clone()).or_insert_with(|| {
            crate::infra::skills::load(std::slice::from_ref(&harness), dir)
                .into_iter()
                .map(|skill| skill.name)
                .collect()
        });

        list.iter()
            .any(|skill| skill == value || skill.ends_with(&format!(":{value}")))
    } else {
        roster_agent
            .skills
            .iter()
            .any(|skill| skill == value || skill.ends_with(&format!(":{value}")))
    }
}

#[cfg(test)]
#[path = "run_tests.rs"]
mod tests;

/// The tool name of a flow wait shown as an ask (the question picker answers it).
pub(crate) const WAIT_TOOL: &str = "FlowWait";

/// The choice of a flow wait that sends the prompt's text as a message: the wait stays.
pub(crate) const REPLY: &str = "__reply";

fn trim_history(state: &mut FlowState) {
    if state.history.len() > 200 {
        state.history.remove(0);
    }
}

fn answer_loop_limit(
    nodes: &[Resolved],
    choice: &str,
    now: u64,
    node: &Resolved,
    state: &mut FlowState,
) -> Result<Moved, String> {
    let (to, _, _) = node.loops.clone().ok_or("0 loop")?;

    match choice {
        "again" => {
            *state.rounds.entry(node.id.clone()).or_default() += 1;
            state.leave(nodes, "loop", now);
            Ok(state.enter(to, now))
        }
        "next" => {
            state.leave(nodes, "done", now);
            Ok(state.next(nodes, now))
        }
        other => Err(format!("loop: 0 choice {other} (again, next)")),
    }
}

fn answer_gate(
    nodes: &[Resolved],
    choice: &str,
    now: u64,
    node: &Resolved,
    state: &mut FlowState,
) -> Result<Moved, String> {
    if choice == "approve" {
        state.leave(nodes, "approved", now);
        return Ok(state.next(nodes, now));
    }

    let (label, to) = node
        .edges
        .iter()
        .find(|(outcome, _)| outcome == choice)
        .cloned()
        .ok_or_else(|| format!("gate: 0 outcome {choice}"))?;

    state.leave(nodes, &label, now);
    Ok(state.enter(to, now))
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
