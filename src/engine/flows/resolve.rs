use super::{Flow, Gate, Layer, Library, Node, Resolved, Step, free};

/// NODE (it uses STEP) as a one-off with STEP's text and fields.
pub(crate) fn inline(node: &Node, step: &Step) -> Node {
    Node {
        id: node.id.clone().or_else(|| Some(step.name.clone())),
        uses: None,
        text: Some(step.text.clone()),
        label: node.label.clone().or_else(|| Some(step.name.clone())),
        note: node.note.clone(),
        role: None,
        done: node
            .done
            .clone()
            .or_else(|| Some(step.done.clone()).filter(|done| !done.is_empty())),
        gate: node
            .gate
            .clone()
            .or(if step.gate { Some(Gate::Flag(true)) } else { None }),
        loops: node.loops.clone(),
    }
}
/// One-off NODE as step NAME (saved by the caller): the node then uses it.
pub(crate) fn promote(node: &Node, name: &str) -> (Step, Node) {
    let step = Step {
        name: name.to_string(),
        description: node.label.clone().unwrap_or_default(),
        old_role: None,
        gate: matches!(node.gate, Some(Gate::Flag(true) | Gate::Edges(_))),
        done: node.done.clone().unwrap_or_default(),
        needs: Vec::new(),
        text: node.text.clone().unwrap_or_default(),
        layer: Layer::User,
        edited: false,
    };
    let promoted = Node {
        id: node.id.clone(),
        uses: Some(name.to_string()),
        text: None,
        label: None,
        note: node.note.clone(),
        role: None,
        done: None,
        gate: match &node.gate {
            Some(Gate::Edges(edges)) => Some(Gate::Edges(edges.clone())),
            _ => None,
        },
        loops: node.loops.clone(),
    };

    (step, promoted)
}
/// Title of NODE: label, else the step name, else the 1st line of the one-off text.
pub(crate) fn node_title(node: &Node) -> String {
    if let Some(label) = node.label.as_deref().filter(|label| !label.trim().is_empty()) {
        return label.trim().to_string();
    }

    if let Some(step) = &node.uses {
        return step.clone();
    }

    let first_line = node
        .text
        .as_deref()
        .unwrap_or_default()
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("one-off step")
        .trim();
    let title: String = first_line.chars().take(48).collect();

    title
}
/// Node ids of FLOW, in order (unique).
pub(crate) fn node_ids(flow: &Flow) -> Vec<String> {
    let mut ids: Vec<String> = Vec::new();

    for (i, node) in flow.steps.iter().enumerate() {
        let base = node
            .id
            .clone()
            .or_else(|| node.uses.clone())
            .unwrap_or_else(|| format!("step-{}", i + 1));
        let id = free(&base, |candidate| ids.iter().any(|taken| taken == candidate));

        ids.push(id);
    }

    ids
}
/// FLOW's nodes with their steps filled in.
pub(crate) fn resolve(flow: &Flow, lib: &Library) -> Vec<Resolved> {
    let ids = node_ids(flow);
    let at = |id: &str| ids.iter().position(|node_id| node_id == id);

    flow.steps
        .iter()
        .enumerate()
        .map(|(i, node)| resolve_node(lib, &ids, &at, i, node))
        .collect()
}

fn resolve_node(lib: &Library, ids: &[String], at: &impl Fn(&str) -> Option<usize>, i: usize, node: &Node) -> Resolved {
    let step = node.uses.as_deref().and_then(|uses| lib.step(uses));
    let mut errors = Vec::new();

    if let (Some(uses), None) = (&node.uses, step) {
        errors.push(format!("step {uses} not found"));
    }

    // an old file's role: gone, the step runs on the flow's agent
    if let Some(role) = node
        .role
        .as_ref()
        .or_else(|| step.and_then(|step| step.old_role.as_ref()))
        .filter(|role| role.as_str() != "lead")
    {
        errors.push(format!(
            "role {role}: roles are gone, the step runs on the flow's agent"
        ));
    }

    let text = node_text(node, step);
    let (gate, edges) = node_gate(node, step, at, &mut errors);
    let loops = node_loop(node, at, i, &mut errors);

    Resolved {
        id: ids[i].clone(),
        title: node_title(node),
        step: node.uses.clone(),
        text,
        done: node
            .done
            .clone()
            .or_else(|| step.map(|step| step.done.clone()))
            .unwrap_or_default(),
        gate,
        edges,
        loops,
        needs: step.map(|step| step.needs.clone()).unwrap_or_default(),
        errors,
    }
}

fn node_text(node: &Node, step: Option<&Step>) -> String {
    let mut text = match (step, &node.text) {
        (Some(step), _) => step.text.clone(),
        (None, Some(one_off)) => one_off.clone(),
        (None, None) => String::new(),
    };

    if let Some(note) = node.note.as_deref().filter(|note| !note.trim().is_empty()) {
        separate_step_note(&mut text);

        text.push_str(note.trim());
    }

    text
}

type GateEdges = Vec<(String, usize)>;

fn node_gate(
    node: &Node,
    step: Option<&Step>,
    at: &impl Fn(&str) -> Option<usize>,
    errors: &mut Vec<String>,
) -> (bool, GateEdges) {
    match &node.gate {
        Some(Gate::Flag(gate)) => (*gate, Vec::new()),
        Some(Gate::Edges(edges)) => (
            true,
            edges
                .iter()
                .filter_map(|(outcome, target)| resolve_gate_edge(at, errors, outcome, target))
                .collect(),
        ),
        None => (step.is_some_and(|step| step.gate), Vec::new()),
    }
}

fn node_loop(
    node: &Node,
    at: &impl Fn(&str) -> Option<usize>,
    i: usize,
    errors: &mut Vec<String>,
) -> Option<(usize, String, u32)> {
    let looped = node.loops.as_ref()?;

    match at(&looped.to) {
        Some(j) if j <= i => Some((j, looped.until.clone(), looped.max.max(1))),
        Some(_) => {
            errors.push(format!("loop to {}: a loop goes to a node above", looped.to));
            None
        }
        None => {
            errors.push(format!("loop to {}: node not found", looped.to));
            None
        }
    }
}

fn resolve_gate_edge(
    at: &impl Fn(&str) -> Option<usize>,
    errors: &mut Vec<String>,
    outcome: &String,
    target: &String,
) -> Option<(String, usize)> {
    if let Some(j) = at(target) {
        Some((outcome.clone(), j))
    } else {
        errors.push(format!("gate outcome {outcome}: node {target} not found"));
        None
    }
}

fn separate_step_note(text: &mut String) {
    if !text.is_empty() {
        text.push_str("\n\n");
    }
}
/// `#name`s in TEXT that name 0 shortcut.
pub(crate) fn unknown_shortcuts(text: &str) -> Vec<String> {
    let mut out = Vec::new();

    for (i, _) in text.match_indices('#') {
        if i > 0
            && text[..i]
                .chars()
                .next_back()
                .is_some_and(|character| character.is_alphanumeric() || character == '#')
        {
            continue;
        }

        let name: String = text[i + 1..]
            .chars()
            .take_while(|character| character.is_alphanumeric() || *character == '-' || *character == '_')
            .collect();

        if name.is_empty() || name.chars().all(|character| character.is_ascii_digit()) || out.contains(&name) {
            continue;
        }

        if crate::domain::shortcuts::get(&name).is_none() {
            out.push(name);
        }
    }

    out
}
