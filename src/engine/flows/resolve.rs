use super::{Flow, Gate, Layer, Library, Node, Resolved, Step, free};

/// Node N (it uses step S) as a one-off with S's text and fields.
pub(crate) fn inline(count: &Node, text_value: &Step) -> Node {
    Node {
        id: count.id.clone().or_else(|| Some(text_value.name.clone())),
        uses: None,
        text: Some(text_value.text.clone()),
        label: count.label.clone().or_else(|| Some(text_value.name.clone())),
        note: count.note.clone(),
        role: None,
        done: count
            .done
            .clone()
            .or_else(|| Some(text_value.done.clone()).filter(|directory| !directory.is_empty())),
        gate: count
            .gate
            .clone()
            .or(if text_value.gate { Some(Gate::Flag(true)) } else { None }),
        loops: count.loops.clone(),
    }
}
/// One-off node N as step NAME (saved by the caller): the node then uses it.
pub(crate) fn promote(count: &Node, name: &str) -> (Step, Node) {
    let text_value = Step {
        name: name.to_string(),
        description: count.label.clone().unwrap_or_default(),
        old_role: None,
        gate: matches!(count.gate, Some(Gate::Flag(true) | Gate::Edges(_))),
        done: count.done.clone().unwrap_or_default(),
        needs: Vec::new(),
        text: count.text.clone().unwrap_or_default(),
        layer: Layer::User,
        edited: false,
    };
    let node = Node {
        id: count.id.clone(),
        uses: Some(name.to_string()),
        text: None,
        label: None,
        note: count.note.clone(),
        role: None,
        done: None,
        gate: match &count.gate {
            Some(Gate::Edges(error)) => Some(Gate::Edges(error.clone())),
            _ => None,
        },
        loops: count.loops.clone(),
    };

    (text_value, node)
}
/// Title of node N: label, else the step name, else the 1st line of the one-off text.
pub(crate) fn node_title(count: &Node) -> String {
    if let Some(line_value) = count.label.as_deref().filter(|line| !line.trim().is_empty()) {
        return line_value.trim().to_string();
    }

    if let Some(user) = &count.uses {
        return user.clone();
    }

    let thread = count
        .text
        .as_deref()
        .unwrap_or_default()
        .lines()
        .find(|line_2| !line_2.trim().is_empty())
        .unwrap_or("one-off step")
        .trim();
    let thread_value: String = thread.chars().take(48).collect();

    thread_value
}
/// Node ids of F, in order (unique).
pub(crate) fn node_ids(file: &Flow) -> Vec<String> {
    let mut ids: Vec<String> = Vec::new();

    for (i, count) in file.steps.iter().enumerate() {
        let base = count
            .id
            .clone()
            .or_else(|| count.uses.clone())
            .unwrap_or_else(|| format!("step-{}", i + 1));
        let id = free(&base, |x| ids.iter().any(|y| y == x));

        ids.push(id);
    }

    ids
}
/// F's nodes with their steps filled in.
pub(crate) fn resolve(file: &Flow, lib: &Library) -> Vec<Resolved> {
    let ids = node_ids(file);
    let at = |id: &str| ids.iter().position(|x| x == id);

    file.steps
        .iter()
        .enumerate()
        .map(|(i, count)| resolve_node(lib, &ids, &at, i, count))
        .collect()
}

fn resolve_node(
    lib: &Library,
    ids: &[String],
    at: &impl Fn(&str) -> Option<usize>,
    i: usize,
    count: &Node,
) -> Resolved {
    let step = count.uses.as_deref().and_then(|user| lib.step(user));
    let mut errors = Vec::new();

    if let (Some(user_value), None) = (&count.uses, step) {
        errors.push(format!("step {user_value} not found"));
    }

    // an old file's role: gone, the step runs on the flow's agent
    if let Some(result_value) = count
        .role
        .as_ref()
        .or_else(|| step.and_then(|text_value| text_value.old_role.as_ref()))
        .filter(|result| result.as_str() != "lead")
    {
        errors.push(format!(
            "role {result_value}: roles are gone, the step runs on the flow's agent"
        ));
    }

    let text = node_text(count, step);
    let (gate, edges) = node_gate(count, step, at, &mut errors);
    let loops = node_loop(count, at, i, &mut errors);

    Resolved {
        id: ids[i].clone(),
        title: node_title(count),
        step: count.uses.clone(),
        text,
        done: count
            .done
            .clone()
            .or_else(|| step.map(|text_4| text_4.done.clone()))
            .unwrap_or_default(),
        gate,
        edges,
        loops,
        needs: step.map(|text_5| text_5.needs.clone()).unwrap_or_default(),
        errors,
    }
}

fn node_text(count: &Node, step: Option<&Step>) -> String {
    let mut text = match (step, &count.text) {
        (Some(text_2), _) => text_2.text.clone(),
        (None, Some(thread)) => thread.clone(),
        (None, None) => String::new(),
    };

    if let Some(note) = count.note.as_deref().filter(|x| !x.trim().is_empty()) {
        separate_step_note(&mut text);

        text.push_str(note.trim());
    }

    text
}

type GateEdges = Vec<(String, usize)>;

fn node_gate(
    count: &Node,
    step: Option<&Step>,
    at: &impl Fn(&str) -> Option<usize>,
    errors: &mut Vec<String>,
) -> (bool, GateEdges) {
    match &count.gate {
        Some(Gate::Flag(bytes)) => (*bytes, Vec::new()),
        Some(Gate::Edges(message)) => (
            true,
            message
                .iter()
                .filter_map(|(key, value)| resolve_gate_edge(at, errors, key, value))
                .collect(),
        ),
        None => (step.is_some_and(|text_3| text_3.gate), Vec::new()),
    }
}

fn node_loop(
    count: &Node,
    at: &impl Fn(&str) -> Option<usize>,
    i: usize,
    errors: &mut Vec<String>,
) -> Option<(usize, String, u32)> {
    let line = count.loops.as_ref()?;

    match at(&line.to) {
        Some(j) if j <= i => Some((j, line.until.clone(), line.max.max(1))),
        Some(_) => {
            errors.push(format!("loop to {}: a loop goes to a node above", line.to));
            None
        }
        None => {
            errors.push(format!("loop to {}: node not found", line.to));
            None
        }
    }
}

fn resolve_gate_edge(
    at: &impl Fn(&str) -> Option<usize>,
    errors: &mut Vec<String>,
    key: &String,
    value: &String,
) -> Option<(String, usize)> {
    if let Some(j) = at(value) {
        Some((key.clone(), j))
    } else {
        errors.push(format!("gate outcome {key}: node {value} not found"));
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

        let width: String = text[i + 1..]
            .chars()
            .take_while(|character_value| {
                character_value.is_alphanumeric() || *character_value == '-' || *character_value == '_'
            })
            .collect();

        if width.is_empty() || width.chars().all(|character_2| character_2.is_ascii_digit()) || out.contains(&width) {
            continue;
        }

        if crate::domain::shortcuts::get(&width).is_none() {
            out.push(width);
        }
    }

    out
}
