use super::{Ask, Flow, FlowsTab, Focus, Gate, Layer, Loop, Out, Step, Sub, flows, input};
use std::collections::BTreeMap;

impl FlowsTab {
    pub(super) fn out(&mut self) -> Out {
        match self.note.take() {
            Some(note) => Out::Toast(note),
            None => Out::None,
        }
    }
    pub(super) fn delete(&mut self) {
        delete_flows_tab(self);
    }

    /// An edited built-in back to the built-in.
    pub(super) fn reset(&mut self) {
        reset_flows_tab(self);
    }
    /// The flow from the user layer to the repo (`.codehead/flows`) and back.
    pub(super) fn move_layer(&mut self) {
        move_layer_flows_tab(self);
    }
    pub(super) fn move_node(&mut self, delta: isize) {
        let Some(flow) = self.flow.as_mut() else { return };
        let to = self.node as isize + delta;

        if to < 0 || to as usize >= flow.steps.len() {
            return;
        }

        // keep the ids of the moved nodes (loops and outcomes point to ids)
        let ids = flows::node_ids(flow);

        for (i, node) in flow.steps.iter_mut().enumerate() {
            pin_node_id(&ids, i, node);
        }

        flow.steps.swap(self.node, to as usize);
        self.node = to as usize;
        self.save_flow();
    }
    pub(super) fn apply_ask(&mut self, what: &Ask, value: &str) -> Result<(), String> {
        apply_ask_flows_tab(self, what, value)
    }
}

fn pin_node_id(ids: &[String], i: usize, node: &mut flows::Node) {
    if node.id.is_none() && node.uses.is_none() {
        node.id = Some(ids[i].clone());
    }
}

fn add_outcome(value: &str, to_id: String, node: &mut flows::Node) {
    let mut edges = match node.gate.take() {
        Some(Gate::Edges(edges)) => edges,
        _ => BTreeMap::default(),
    };

    edges.insert(value.to_string(), to_id);
    node.gate = Some(Gate::Edges(edges));
}

/// An id for node I (set when it has none): loops and outcomes point to it.
fn id_of(i: usize, tab: &mut FlowsTab) -> String {
    let Some(flow) = tab.flow.as_mut() else {
        return String::new();
    };

    let ids = flows::node_ids(flow);

    if flow.steps[i].id.is_none() && flow.steps[i].uses.as_deref() != Some(ids[i].as_str()) {
        flow.steps[i].id = Some(ids[i].clone());
    }

    ids[i].clone()
}

fn rename_flow(value: &str, tab: &mut FlowsTab) {
    if let Some(flow) = tab.flow.as_mut() {
        flow.name = value.to_string();
    }

    tab.save_flow();
}

fn set_loop_until(value: &str, from: usize, to: usize, tab: &mut FlowsTab) {
    let to_id = id_of(to, tab);

    if let Some(node) = tab.flow.as_mut().and_then(|flow| flow.steps.get_mut(from)) {
        node.loops = Some(Loop {
            to: to_id,
            until: value.to_string(),
            max: 3,
        });
    }

    tab.save_flow();
}

fn note_step_deleted(step: &Step, count: usize, tab: &mut FlowsTab) {
    tab.note = Some(format!(
        "step {} deleted · {count} flows keep its text as a one-off",
        step.name
    ));
}

fn delete_flows_tab(state: &mut FlowsTab) {
    match state.sub {
        Sub::Flows => {
            let Some(flow) = state.flow.clone() else { return };

            if flow.layer == Layer::Builtin {
                return state.fail("a built-in flow stays: edit it (a user copy), or duplicate it");
            }

            if let Err(error) = state.lib.remove_flow_file(&flow.slug, flow.layer) {
                return state.fail(error);
            }

            state.note = Some(if flow.edited {
                format!("{}: back to the built-in", flow.name)
            } else {
                format!("flow {} deleted", flow.name)
            });
        }
        Sub::Steps => {
            let Some(step) = state.cur_step().cloned() else { return };

            if step.edited {
                return state.fail("an edited built-in: ctrl+d resets it");
            }

            match state.lib.delete_step(&step.name) {
                Ok(count) => {
                    note_step_deleted(&step, count, state);
                }
                Err(error) => return state.fail(error),
            }
        }
    }

    state.error = None;
    state.reload();
}

fn reset_flows_tab(state: &mut FlowsTab) {
    let (name, layer, edited) = match state.sub {
        Sub::Flows => match &state.flow {
            Some(flow) => (flow.slug.clone(), flow.layer, flow.edited),
            None => return,
        },
        Sub::Steps => match state.cur_step() {
            Some(step) => (step.name.clone(), step.layer, step.edited),
            None => return,
        },
    };

    if !edited {
        return state.fail("0 edits to reset: only an edited built-in resets");
    }

    let result = if state.sub == Sub::Flows {
        state.lib.remove_flow_file(&name, layer)
    } else {
        state.lib.remove_step_file(&name, layer)
    };

    match result {
        Ok(()) => state.note = Some(format!("{name}: back to the built-in")),
        Err(error) => return state.fail(error),
    }

    state.flow = None;
    state.reload();
}

fn move_layer_flows_tab(state: &mut FlowsTab) {
    let Some(mut flow) = state.flow.clone() else { return };

    if state.lib.repo.is_none() {
        return state.fail("0 repo here: start codehead in a repo for repo flows");
    }

    let old = flow.layer;

    flow.layer = if old == Layer::Repo { Layer::User } else { Layer::Repo };

    if let Err(error) = state.lib.save_flow(&flow) {
        return state.fail(error);
    }

    if old != Layer::Builtin {
        let _ = state.lib.remove_flow_file(&flow.slug, old);
    }

    state.note = Some(format!("{} → {} flows", flow.name, flow.layer.label()));
    state.flow = Some(flow);
    state.reload();
}

fn apply_ask_flows_tab(state: &mut FlowsTab, what: &Ask, value: &str) -> Result<(), String> {
    match what {
        Ask::NewFlow | Ask::DuplicateFlow => create_flow(state, what, value)?,
        Ask::RenameFlow => {
            rename_flow(value, state);
        }
        Ask::NewStep | Ask::DuplicateStep => create_step(state, what, value)?,
        Ask::RenameStep => {
            let Some(step) = state.cur_step().cloned() else {
                return Ok(());
            };

            let new = flows::slug(value);

            state.lib.rename_step(&step.name, &new)?;
            state.reload();
            state.at = state
                .list()
                .iter()
                .position(|&i| state.lib.steps[i].name == new)
                .unwrap_or(0);
            state.note = Some(format!("step {} → {new}: every flow follows", step.name));
        }
        Ask::Promote(i) => {
            let name = flows::slug(value);

            if state.lib.step(&name).is_some() {
                return Err(format!("step {name} exists already"));
            }

            let Some(node) = state.flow.as_ref().and_then(|flow| flow.steps.get(*i)).cloned() else {
                return Ok(());
            };

            let (step, promoted) = flows::promote(&node, &name);

            state.lib.save_step(&step)?;

            if let Some(flow) = state.flow.as_mut() {
                flow.steps[*i] = promoted;
            }

            state.save_flow();
            state.note = Some(format!("step {name} saved: a for it in any flow"));
        }
        Ask::LoopUntil(from, to) => {
            set_loop_until(value, *from, *to, state);
        }
        Ask::Outcome(from, to) => {
            if !flows::valid_name(value) {
                return Err("label: letters, digits, - and _".into());
            }

            let to_id = id_of(*to, state);

            if let Some(node) = state.flow.as_mut().and_then(|flow| flow.steps.get_mut(*from)) {
                add_outcome(value, to_id, node);
            }

            state.save_flow();
        }
    }

    Ok(())
}

fn create_flow(state: &mut FlowsTab, what: &Ask, value: &str) -> Result<(), String> {
    let slug = state.lib.free_flow_slug(&flows::slug(value));

    let mut flow = match what {
        Ask::DuplicateFlow => state.flow.clone().unwrap_or_default(),
        _ => Flow::default(),
    };

    flow.slug.clone_from(&slug);
    flow.name = value.to_string();

    flow.layer = if flow.layer == Layer::Repo {
        Layer::Repo
    } else {
        Layer::User
    };

    flow.edited = false;
    state.lib.save_flow(&flow)?;
    state.filter.clear();
    state.reload();
    state.at = state
        .list()
        .iter()
        .position(|&i| state.lib.flows[i].slug == slug)
        .unwrap_or(0);
    state.load_flow();
    state.focus = Focus::Rail;

    if flow.steps.is_empty() {
        state.picker = Some((input(true, "step name, or text of a one-off step", ""), 0));
    }

    Ok(())
}

fn create_step(state: &mut FlowsTab, what: &Ask, value: &str) -> Result<(), String> {
    let name = flows::slug(value);

    if state.lib.step(&name).is_some() {
        return Err(format!("step {name} exists already"));
    }

    let mut step = match what {
        Ask::DuplicateStep => state.cur_step().cloned().unwrap_or_default(),
        _ => Step::default(),
    };

    step.name.clone_from(&name);
    step.layer = Layer::User;
    step.edited = false;
    state.lib.save_step(&step)?;
    state.filter.clear();
    state.reload();
    state.at = state
        .list()
        .iter()
        .position(|&i| state.lib.steps[i].name == name)
        .unwrap_or(0);

    if let Some(step) = state.lib.step(&name).cloned() {
        state.open_step_editor(&step);
    }

    Ok(())
}
