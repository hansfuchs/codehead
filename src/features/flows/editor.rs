use super::{Edit, Editor, FlowsTab, Focus, Gate, Node, Step, TextArea, flows, input};

impl FlowsTab {
    pub(super) fn open_step_editor(&mut self, step: &Step) {
        open_step_editor_flows_tab(self, step);
    }
    pub(super) fn open_node_editor(&mut self, i: usize) {
        open_node_editor_flows_tab(self, i);
    }

    /// The editor's inputs to disk. False: an error (the editor stays).
    pub(super) fn store_editor(&mut self) -> bool {
        store_editor_flows_tab(self)
    }

    fn store_shared_step(&mut self, field_text: impl Fn(&str) -> String, step: Step) -> Option<bool> {
        let (done, text) = (
            field_text("done when").trim().to_string(),
            field_text("text").trim_end().to_string(),
        );

        if (step.done != done || step.text != text) && !save_shared_step_text(step, done, text, self) {
            return Some(false);
        }

        None
    }

    fn store_step_editor(&mut self, gate: bool, field_text: impl Fn(&str) -> String, name: &str) -> Option<bool> {
        let Some(old) = self.lib.step(name).cloned() else {
            return Some(true);
        };

        let new = Step {
            description: field_text("description").trim().to_string(),
            done: field_text("done when").trim().to_string(),
            needs: field_text("needs")
                .split(',')
                .map(|x| x.trim().to_string())
                .filter(|x| !x.is_empty())
                .collect(),
            text: field_text("text").trim_end().to_string(),
            gate,
            ..old.clone()
        };

        if new != old && !self.save_step(&new) {
            return Some(false);
        }

        None
    }
}

fn store_inline_step(
    field_text: impl Fn(&str) -> String,
    non_empty: impl Fn(String) -> Option<String>,
    node: &mut Node,
) {
    node.done = non_empty(field_text("done when"));
    node.text = Some(field_text("text").trim_end().to_string());
}

fn shared_node_fields(node: &Node, step_name: &str, tab: &mut FlowsTab) -> Vec<(&'static str, TextArea, bool)> {
    let step = tab.lib.step(step_name).cloned().unwrap_or_default();

    vec![
        (
            "label",
            input(
                true,
                "the step's name in this flow (empty: the step name)",
                node.label.as_deref().unwrap_or_default(),
            ),
            false,
        ),
        (
            "note",
            input(
                false,
                "text for this flow only, after the step's text",
                node.note.as_deref().unwrap_or_default(),
            ),
            false,
        ),
        ("done when", input(true, "exit criteria", &step.done), true),
        ("text", input(false, "the step's instruction", &step.text), true),
    ]
}

fn save_shared_step_text(mut step: Step, done: String, text: String, state: &mut FlowsTab) -> bool {
    step.done = done;
    step.text = text;
    state.save_step(&step)
}

fn open_step_editor_flows_tab(state: &mut FlowsTab, step: &Step) {
    state.editor = Some(Editor {
        what: Edit::Step(step.name.clone()),
        fields: vec![
            (
                "description",
                input(true, "1 line: what the step does", &step.description),
                false,
            ),
            ("done when", input(true, "exit criteria", &step.done), false),
            (
                "needs",
                input(
                    true,
                    "skill:<name>, mcp:<server>, harness:<name>",
                    &step.needs.join(", "),
                ),
                false,
            ),
            (
                "text",
                input(
                    false,
                    "the instruction the agent gets · #shortcuts expand at the step's start",
                    &step.text,
                ),
                false,
            ),
        ],
        field: 4,
        gate: step.gate,
        gate0: step.gate,
    });
    state.focus = Focus::Editor;
}

fn open_node_editor_flows_tab(state: &mut FlowsTab, i: usize) {
    let Some(flow) = state.flow.clone() else { return };

    let Some(node) = flow.steps.get(i).cloned() else {
        return;
    };

    let resolved = flows::resolve(&flow, &state.lib).get(i).cloned().unwrap_or_default();
    let shared = node.uses.clone().filter(|uses| state.lib.step(uses).is_some());

    let fields = match &shared {
        Some(step_name) => shared_node_fields(&node, step_name, state),
        None => vec![
            (
                "label",
                input(true, "short name", node.label.as_deref().unwrap_or_default()),
                false,
            ),
            (
                "done when",
                input(true, "exit criteria", node.done.as_deref().unwrap_or_default()),
                false,
            ),
            (
                "text",
                input(
                    false,
                    "the instruction · #shortcuts expand at the step's start",
                    node.text.as_deref().unwrap_or_default(),
                ),
                false,
            ),
        ],
    };

    let field = fields.len() - 1;

    state.editor = Some(Editor {
        what: Edit::Node(i, shared),
        fields,
        field,
        gate: resolved.gate,
        gate0: resolved.gate,
    });
    state.focus = Focus::Editor;
}

fn store_editor_flows_tab(state: &mut FlowsTab) -> bool {
    let Some(editor) = state.editor.as_ref() else {
        return true;
    };

    let texts: Vec<(&'static str, String)> = editor
        .fields
        .iter()
        .map(|(label, area, _)| (*label, area.text()))
        .collect();
    let (what, gate) = (editor.what.clone(), editor.gate);

    let field_text = |key: &str| {
        texts
            .iter()
            .find(|(label, _)| *label == key)
            .map(|(_, text)| text.clone())
            .unwrap_or_default()
    };

    let non_empty = |text: String| Some(text.trim().to_string()).filter(|x| !x.is_empty());

    match what {
        Edit::Step(name) => {
            if let Some(value) = state.store_step_editor(gate, field_text, &name) {
                return value;
            }
        }
        Edit::Node(i, shared) => {
            if let Some(step_name) = &shared
                && let Some(step) = state.lib.step(step_name).cloned()
                && let Some(value) = state.store_shared_step(field_text, step)
            {
                return value;
            }

            let lib_gate = shared
                .as_deref()
                .and_then(|step_name| state.lib.step(step_name))
                .is_some_and(|step| step.gate);
            let Some(flow) = state.flow.as_mut() else { return true };

            let Some(node) = flow.steps.get_mut(i) else {
                return true;
            };

            node.label = non_empty(field_text("label"));

            if shared.is_some() {
                node.note = non_empty(field_text("note"));
            } else {
                store_inline_step(field_text, non_empty, node);
            }

            node.gate = match (&node.gate, gate) {
                (Some(Gate::Edges(edges)), true) => Some(Gate::Edges(edges.clone())),
                (_, flag) if flag == lib_gate => None,
                (_, flag) => Some(Gate::Flag(flag)),
            };

            state.save_flow();
        }
    }

    true
}
