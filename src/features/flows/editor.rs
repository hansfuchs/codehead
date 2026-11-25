use super::{Edit, Editor, FlowsTab, Focus, Gate, Node, Step, TextArea, flows, input};

impl FlowsTab {
    pub(super) fn open_step_editor(&mut self, text_value: &Step) {
        open_step_editor_flows_tab(self, text_value);
    }
    pub(super) fn open_node_editor(&mut self, i: usize) {
        open_node_editor_flows_tab(self, i);
    }

    /// The editor's inputs to disk. False: an error (the editor stays).
    pub(super) fn store_editor(&mut self) -> bool {
        store_editor_flows_tab(self)
    }

    fn store_editor_body(&mut self, val: impl Fn(&str) -> String, text_3: Step) -> Option<bool> {
        let (done, text) = (val("done when").trim().to_string(), val("text").trim_end().to_string());

        if (text_3.done != done || text_3.text != text) && !save_shared_step_text(text_3, done, text, self) {
            return Some(false);
        }

        None
    }

    fn store_editor_edit_step_name(&mut self, ed_gate: bool, val: impl Fn(&str) -> String, name: &str) -> Option<bool> {
        let Some(text_2) = self.lib.step(name).cloned() else {
            return Some(true);
        };

        let new = Step {
            description: val("description").trim().to_string(),
            done: val("done when").trim().to_string(),
            needs: val("needs")
                .split(',')
                .map(|x| x.trim().to_string())
                .filter(|x| !x.is_empty())
                .collect(),
            text: val("text").trim_end().to_string(),
            gate: ed_gate,
            ..text_2.clone()
        };

        if new != text_2 && !self.save_step(&new) {
            return Some(false);
        }

        None
    }
}

fn store_editor_body_state(val: impl Fn(&str) -> String, opt: impl Fn(String) -> Option<String>, count: &mut Node) {
    count.done = opt(val("done when"));
    count.text = Some(val("text").trim_end().to_string());
}

fn open_node_editor_some_user_value(
    count: &Node,
    user_value: &str,
    view_state: &mut FlowsTab,
) -> Vec<(&'static str, TextArea, bool)> {
    let text_value = view_state.lib.step(user_value).cloned().unwrap_or_default();

    vec![
        (
            "label",
            input(
                true,
                "the step's name in this flow (empty: the step name)",
                count.label.as_deref().unwrap_or_default(),
            ),
            false,
        ),
        (
            "note",
            input(
                false,
                "text for this flow only, after the step's text",
                count.note.as_deref().unwrap_or_default(),
            ),
            false,
        ),
        ("done when", input(true, "exit criteria", &text_value.done), true),
        ("text", input(false, "the step's instruction", &text_value.text), true),
    ]
}

fn save_shared_step_text(mut step: Step, done: String, text: String, state: &mut FlowsTab) -> bool {
    step.done = done;
    step.text = text;
    state.save_step(&step)
}

fn open_step_editor_flows_tab(state: &mut FlowsTab, text_value: &Step) {
    state.editor = Some(Editor {
        what: Edit::Step(text_value.name.clone()),
        fields: vec![
            (
                "description",
                input(true, "1 line: what the step does", &text_value.description),
                false,
            ),
            ("done when", input(true, "exit criteria", &text_value.done), false),
            (
                "needs",
                input(
                    true,
                    "skill:<name>, mcp:<server>, harness:<name>",
                    &text_value.needs.join(", "),
                ),
                false,
            ),
            (
                "text",
                input(
                    false,
                    "the instruction the agent gets · #shortcuts expand at the step's start",
                    &text_value.text,
                ),
                false,
            ),
        ],
        field: 4,
        gate: text_value.gate,
        gate0: text_value.gate,
    });
    state.focus = Focus::Editor;
}

fn open_node_editor_flows_tab(state: &mut FlowsTab, i: usize) {
    let Some(file) = state.flow.clone() else { return };

    let Some(count) = file.steps.get(i).cloned() else {
        return;
    };

    let result = flows::resolve(&file, &state.lib).get(i).cloned().unwrap_or_default();
    let shared = count.uses.clone().filter(|user| state.lib.step(user).is_some());

    let fields = match &shared {
        Some(user_value) => open_node_editor_some_user_value(&count, user_value, state),
        None => vec![
            (
                "label",
                input(true, "short name", count.label.as_deref().unwrap_or_default()),
                false,
            ),
            (
                "done when",
                input(true, "exit criteria", count.done.as_deref().unwrap_or_default()),
                false,
            ),
            (
                "text",
                input(
                    false,
                    "the instruction · #shortcuts expand at the step's start",
                    count.text.as_deref().unwrap_or_default(),
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
        gate: result.gate,
        gate0: result.gate,
    });
    state.focus = Focus::Editor;
}

fn store_editor_flows_tab(state: &mut FlowsTab) -> bool {
    let Some(ed) = state.editor.as_ref() else { return true };
    let vals: Vec<(&'static str, String)> = ed
        .fields
        .iter()
        .map(|(line, thread, _)| (*line, thread.text()))
        .collect();
    let (what, ed_gate) = (ed.what.clone(), ed.gate);

    let val = |key: &str| {
        vals.iter()
            .find(|(line_value, _)| *line_value == key)
            .map(|(_, thread_value)| thread_value.clone())
            .unwrap_or_default()
    };

    let opt = |text_value: String| Some(text_value.trim().to_string()).filter(|x| !x.is_empty());

    match what {
        Edit::Step(name) => {
            if let Some(value) = state.store_editor_edit_step_name(ed_gate, val, &name) {
                return value;
            }
        }
        Edit::Node(i, shared) => {
            let gate = ed_gate;

            if let Some(user) = &shared
                && let Some(text_3) = state.lib.step(user).cloned()
                && let Some(value) = state.store_editor_body(val, text_3)
            {
                return value;
            }

            let lib_gate = shared
                .as_deref()
                .and_then(|user_value| state.lib.step(user_value))
                .is_some_and(|text_4| text_4.gate);
            let Some(file) = state.flow.as_mut() else { return true };

            let Some(count) = file.steps.get_mut(i) else {
                return true;
            };

            count.label = opt(val("label"));

            if shared.is_some() {
                count.note = opt(val("note"));
            } else {
                store_editor_body_state(val, opt, count);
            }

            count.gate = match (&count.gate, gate) {
                (Some(Gate::Edges(error)), true) => Some(Gate::Edges(error.clone())),
                (_, group) if group == lib_gate => None,
                (_, group_value) => Some(Gate::Flag(group_value)),
            };

            state.save_flow();
        }
    }

    true
}
