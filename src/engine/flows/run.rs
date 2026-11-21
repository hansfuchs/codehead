//! A thread's flow at run time: the state in `thread.json` and its moves. Pure: the host does
//! the I/O (log, delivery, interrupts).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::{Flow, Library, Resolved, resolve, roles};

/// Why the flow waits on the owner.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Wait {
    /// A gate node: approve, or another outcome.
    Gate,
    /// A loop reached its max: 1 more round, or move on.
    LoopMax,
    /// Role ROLE has 0 agent: the owner picks one to bring in.
    Role { role: String },
}

/// A switch to flow FLOW at node START.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Switch {
    pub flow: String,
    #[serde(default)]
    pub start: usize,
}

/// A node that ended: how, when.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Past {
    pub node: usize,
    pub title: String,
    /// `done`, `skipped`, `approved`, `passed by agent`, `loop`, outcome label, `left`.
    pub how: String,
    pub ms: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FlowState {
    /// Flow slug.
    pub flow: String,
    /// Its name when set (the flow file can go).
    pub title: String,
    /// Current node.
    pub node: usize,
    /// Loop rounds per node id (the node that holds the loop).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub rounds: BTreeMap<String, u32>,
    /// Role → agent.
    #[serde(default)]
    pub roles: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
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
pub enum Moved {
    /// To node N.
    To(usize),
    Wait(Wait),
    Finished,
}

impl FlowState {
    /// A new state for flow F at node START: roles mapped from the thread's agents.
    pub fn new(f: &Flow, lib: &Library, start: usize, agents: &[String], roster: &[crate::engine::roster::Agent], now: u64) -> FlowState {
        let nodes = resolve(f, lib);
        let mut st = FlowState { flow: f.slug.clone(), title: f.name.clone(), node: start.min(nodes.len().saturating_sub(1)), node_started: now, ..Default::default() };
        st.roles = auto_roles(&roles(f, &nodes), agents, roster, &BTreeMap::new());
        st
    }

    pub fn round(&self, id: &str) -> u32 {
        self.rounds.get(id).copied().unwrap_or(0)
    }

    fn leave(&mut self, nodes: &[Resolved], how: &str, now: u64) {
        if let Some(n) = nodes.get(self.node) {
            self.history.push(Past { node: self.node, title: n.title.clone(), how: how.into(), ms: now.saturating_sub(self.node_started) });
            // keep the file small: the last 200 nodes
            if self.history.len() > 200 {
                self.history.remove(0);
            }
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
    pub fn step_done(&mut self, nodes: &[Resolved], again: bool, pass: bool, now: u64) -> Moved {
        let Some(n) = nodes.get(self.node).cloned() else {
            self.finished = true;
            return Moved::Finished;
        };
        if let Some(w) = &self.wait {
            return Moved::Wait(w.clone());
        }
        if let (true, Some((to, _, max))) = (again, &n.loops) {
            let r = self.round(&n.id);
            if r + 1 >= *max {
                self.wait = Some(Wait::LoopMax);
                return Moved::Wait(Wait::LoopMax);
            }
            self.rounds.insert(n.id.clone(), r + 1);
            self.leave(nodes, "loop", now);
            return self.enter(*to, now);
        }
        if n.gate && !pass {
            self.wait = Some(Wait::Gate);
            return Moved::Wait(Wait::Gate);
        }
        self.leave(nodes, if n.gate { "passed by agent" } else { "done" }, now);
        self.next(nodes, now)
    }

    /// The owner's answer to the wait: gate `approve` or an outcome label; loop max `again` or
    /// `next`. Err: 0 such choice.
    pub fn answer(&mut self, nodes: &[Resolved], choice: &str, now: u64) -> Result<Moved, String> {
        let n = nodes.get(self.node).cloned().ok_or("0 current step")?;
        match self.wait.clone() {
            Some(Wait::Gate) => {
                if choice == "approve" {
                    self.leave(nodes, "approved", now);
                    return Ok(self.next(nodes, now));
                }
                let (label, to) = n.edges.iter().find(|(k, _)| k == choice).cloned().ok_or_else(|| format!("gate: 0 outcome {choice}"))?;
                self.leave(nodes, &label, now);
                Ok(self.enter(to, now))
            }
            Some(Wait::LoopMax) => {
                let (to, _, _) = n.loops.clone().ok_or("0 loop")?;
                match choice {
                    "again" => {
                        *self.rounds.entry(n.id.clone()).or_default() += 1;
                        self.leave(nodes, "loop", now);
                        Ok(self.enter(to, now))
                    }
                    "next" => {
                        self.leave(nodes, "done", now);
                        Ok(self.next(nodes, now))
                    }
                    c => Err(format!("loop: 0 choice {c} (again, next)")),
                }
            }
            Some(Wait::Role { .. }) => Err("the flow waits for an agent: pick one".into()),
            None => Err("the flow waits on 0 answer".into()),
        }
    }

    pub fn skip(&mut self, nodes: &[Resolved], now: u64) -> Moved {
        self.leave(nodes, "skipped", now);
        self.next(nodes, now)
    }

    pub fn jump(&mut self, nodes: &[Resolved], to: usize, now: u64) -> Result<Moved, String> {
        if to >= nodes.len() {
            return Err(format!("0 step {}", to + 1));
        }
        self.finished = false;
        self.leave(nodes, "left", now);
        Ok(self.enter(to, now))
    }

    /// The agent of node N (None: its role has 0 agent).
    pub fn agent_of(&self, n: &Resolved, lead: Option<&str>) -> Option<String> {
        self.roles.get(&n.role).cloned().or_else(|| (n.role == super::LEAD).then(|| lead.map(String::from)).flatten())
    }
}

/// Map ROLES to agents: KEEP first (still on the thread), `lead` → the thread's lead, else an agent
/// on the thread whose `roles` has the role, else 0 (an open role).
pub fn auto_roles(roles: &[String], agents: &[String], roster: &[crate::engine::roster::Agent], keep: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    let mut m = BTreeMap::new();
    for r in roles {
        if let Some(a) = keep.get(r).filter(|a| agents.contains(a)) {
            m.insert(r.clone(), a.clone());
            continue;
        }
        if r == super::LEAD {
            if let Some(l) = agents.first() {
                m.insert(r.clone(), l.clone());
            }
            continue;
        }
        if let Some(a) = agents.iter().find(|a| roster.iter().any(|x| &x.name == *a && x.roles.contains(r))) {
            m.insert(r.clone(), a.clone());
        }
    }
    m
}

/// The message an agent gets at the start of node I.
pub fn step_message(title: &str, nodes: &[Resolved], i: usize, st: &FlowState, task: Option<&str>) -> String {
    let n = &nodes[i];
    let mut s = format!("[codehead flow: {title} · step {}/{}: {}]\n\n", i + 1, nodes.len(), n.title);
    s.push_str(&crate::shortcuts::expand(&n.text));
    if !n.done.is_empty() {
        s.push_str(&format!("\n\nDone when: {}", n.done));
    }
    if let Some((to, until, max)) = &n.loops {
        let r = st.round(&n.id) + 1;
        s.push_str(&format!(
            "\n\nLoop (round {r}/{max}): when this step is done, check: {until}. Not met: `flow_step` with `again: true` (back to step {}: {}). Met: `flow_step` without it.",
            to + 1,
            nodes[*to].title
        ));
    }
    if n.gate {
        s.push_str("\n\nGate: after this step, the owner approves before the flow goes on. Call `flow_step` when the step is done; then wait for the owner.");
    }
    s.push_str("\n\nWhen the step is done: call `flow_step` (codehead MCP). Do not start the next step before it: codehead gives you the next step, or hands it to another agent.");
    // the 1st step of a flow: the whole flow, so the agent knows what comes later
    if st.history.is_empty() && i == st.node {
        s.push_str(&format!("\n\nThe flow (▶ = this step; later steps come one at a time):\n{}", outline(nodes, st)));
    }
    if let Some(t) = task.filter(|t| !t.trim().is_empty()) {
        s.push_str(&format!("\n\n---\n\n{t}"));
    }
    s
}

/// The flow outline: 1 line per node, the current one marked.
pub fn outline(nodes: &[Resolved], st: &FlowState) -> String {
    nodes
        .iter()
        .enumerate()
        .map(|(i, n)| {
            let mark = if i == st.node && !st.finished { "▶" } else if st.history.iter().any(|p| p.node == i) { "✓" } else { " " };
            let mut l = format!("{mark} {}. {} (@{})", i + 1, n.title, st.roles.get(&n.role).map(String::as_str).unwrap_or(&n.role));
            if n.gate {
                l.push_str(" [gate]");
            }
            if let Some((to, _, max)) = &n.loops {
                l.push_str(&format!(" [loop → {}, max {max}]", to + 1));
            }
            l
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Problems of the nodes for the thread: open roles, needs the role's agent lacks, node errors.
/// (node index, text). DIR: the work dir (skills of the project).
pub fn warnings(nodes: &[Resolved], st: &FlowState, lead: Option<&str>, roster: &[crate::engine::roster::Agent], dir: &std::path::Path) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let mut skills: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (i, n) in nodes.iter().enumerate() {
        for e in &n.errors {
            out.push((i, e.clone()));
        }
        let Some(agent) = st.agent_of(n, lead) else {
            out.push((i, format!("@{}: 0 agent", n.role)));
            continue;
        };
        let Some(a) = roster.iter().find(|a| a.name == agent) else { continue };
        for need in &n.needs {
            let (k, v) = need.split_once(':').unwrap_or(("skill", need));
            let ok = match k {
                "skill" => {
                    if !a.skills.is_empty() {
                        a.skills.iter().any(|s| s == v || s.ends_with(&format!(":{v}")))
                    } else {
                        let h = a.harness.split('/').next().unwrap_or_default().to_string();
                        let list = skills.entry(h.clone()).or_insert_with(|| crate::skills::load(std::slice::from_ref(&h), dir).into_iter().map(|s| s.name).collect());
                        list.iter().any(|s| s == v || s.ends_with(&format!(":{v}")))
                    }
                }
                "mcp" => a.mcp.iter().any(|m| m == v),
                "harness" => a.harness.split('/').next() == Some(v),
                _ => true,
            };
            if !ok {
                out.push((i, format!("@{} ({agent}, {}): 0 {k} {v}", n.role, a.harness)));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::flows::{Library, parse_flow, resolve};

    fn flow() -> (Flow, Vec<Resolved>) {
        let f = parse_flow(
            "t",
            r#"name = "t"
roles = ["lead", "reviewer"]
[[step]]
id = "plan"
text = "plan it"
[[step]]
text = "approve"
gate = { changes = "plan" }
[[step]]
id = "review"
text = "review"
role = "reviewer"
[[step]]
text = "address"
loop = { to = "review", until = "0 findings", max = 2 }
[[step]]
text = "report"
"#,
        )
        .unwrap();
        let n = resolve(&f, &Library::default());
        (f, n)
    }

    #[test]
    fn gates_wait_and_outcomes_pick_the_edge() {
        let (f, n) = flow();
        let mut st = FlowState::new(&f, &Library::default(), 0, &["ada".into()], &[], 0);
        assert_eq!(st.roles.get("lead").map(String::as_str), Some("ada"));
        assert!(!st.roles.contains_key("reviewer"), "0 reviewer agent: open role");
        assert_eq!(st.step_done(&n, false, false, 1), Moved::To(1));
        assert_eq!(st.step_done(&n, false, false, 2), Moved::Wait(Wait::Gate));
        // a 2nd report while waiting: still waiting
        assert_eq!(st.step_done(&n, false, false, 3), Moved::Wait(Wait::Gate));
        assert_eq!(st.answer(&n, "changes", 4).unwrap(), Moved::To(0));
        assert!(st.answer(&n, "approve", 5).is_err(), "0 wait: 0 answer");
        st.step_done(&n, false, false, 6);
        st.step_done(&n, false, false, 7);
        assert_eq!(st.answer(&n, "approve", 8).unwrap(), Moved::To(2));
        // an autopilot agent passes a gate itself
        let mut s2 = st.clone();
        s2.node = 1;
        assert_eq!(s2.step_done(&n, false, true, 9), Moved::To(2));
        assert_eq!(s2.history.last().unwrap().how, "passed by agent");
    }

    #[test]
    fn loops_go_back_until_max_then_wait() {
        let (f, n) = flow();
        let mut st = FlowState::new(&f, &Library::default(), 3, &["ada".into()], &[], 0);
        // round 1 → back to review
        assert_eq!(st.step_done(&n, true, false, 1), Moved::To(2));
        st.step_done(&n, false, false, 2);
        // round 2 = max: the owner decides
        assert_eq!(st.step_done(&n, true, false, 3), Moved::Wait(Wait::LoopMax));
        assert_eq!(st.answer(&n, "again", 4).unwrap(), Moved::To(2));
        st.step_done(&n, false, false, 5);
        assert_eq!(st.step_done(&n, false, false, 6), Moved::To(4));
        assert_eq!(st.step_done(&n, false, false, 7), Moved::Finished);
        assert!(st.finished);
        assert!(outline(&n, &st).contains("[loop → 3, max 2]"));
    }

    #[test]
    fn step_messages_say_how_to_report() {
        let (f, n) = flow();
        let st = FlowState::new(&f, &Library::default(), 3, &["ada".into()], &[], 0);
        let m = step_message("t", &n, 3, &st, Some("the task"));
        assert!(m.starts_with("[codehead flow: t · step 4/5: address]"));
        assert!(m.contains("round 1/2") && m.contains("`again: true`") && m.ends_with("the task"));
        let w = warnings(&n, &st, Some("ada"), &[], std::path::Path::new("/tmp"));
        assert_eq!(w, vec![(2, "@reviewer: 0 agent".to_string())]);
    }
}
