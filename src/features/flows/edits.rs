use super::{Ask, Flow, FlowsTab, Focus, Gate, Layer, Loop, Out, Step, Sub, flows, input};
use std::collections::BTreeMap;

impl FlowsTab {
    pub(super) fn out(&mut self) -> Out {
        match self.note.take() {
            Some(count) => Out::Toast(count),
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
    pub(super) fn move_node(&mut self, directory: isize) {
        let Some(fl) = self.flow.as_mut() else { return };
        let to = self.node as isize + directory;

        if to < 0 || to as usize >= fl.steps.len() {
            return;
        }

        // keep the ids of the moved nodes (loops and outcomes point to ids)
        let ids = flows::node_ids(fl);

        for (i, nd) in fl.steps.iter_mut().enumerate() {
            move_node_branch(&ids, i, nd);
        }

        fl.steps.swap(self.node, to as usize);
        self.node = to as usize;
        self.save_flow();
    }
    pub(super) fn apply_ask(&mut self, what: &Ask, value: &str) -> Result<(), String> {
        apply_ask_flows_tab(self, what, value)
    }
}

fn move_node_branch(ids: &[String], i: usize, nd: &mut flows::Node) {
    if nd.id.is_none() && nd.uses.is_none() {
        nd.id = Some(ids[i].clone());
    }
}

fn apply_ask_message_value(value: &str, to_id: String, nd: &mut flows::Node) {
    let mut message_value = match nd.gate.take() {
        Some(Gate::Edges(message)) => message,
        _ => BTreeMap::default(),
    };

    message_value.insert(value.to_string(), to_id);
    nd.gate = Some(Gate::Edges(message_value));
}

/// An id for node I (set when it has none): loops and outcomes point to it.
fn id_of(i: usize, view_state: &mut FlowsTab) -> String {
    let Some(fl) = view_state.flow.as_mut() else {
        return String::new();
    };

    let ids = flows::node_ids(fl);

    if fl.steps[i].id.is_none() && fl.steps[i].uses.as_deref() != Some(ids[i].as_str()) {
        fl.steps[i].id = Some(ids[i].clone());
    }

    ids[i].clone()
}

fn apply_ask_ask_renameflow(value: &str, view_state: &mut FlowsTab) {
    if let Some(file_value) = view_state.flow.as_mut() {
        file_value.name = value.to_string();
    }

    view_state.save_flow();
}

fn apply_ask_ask_loopuntil_from_to(value: &str, from: usize, to: usize, view_state: &mut FlowsTab) {
    let to_id = id_of(to, view_state);

    if let Some(nd) = view_state.flow.as_mut().and_then(|file_4| file_4.steps.get_mut(from)) {
        nd.loops = Some(Loop {
            to: to_id,
            until: value.to_string(),
            max: 3,
        });
    }

    view_state.save_flow();
}

fn delete_ok_count(text: &Step, count: usize, view_state: &mut FlowsTab) {
    view_state.note = Some(format!(
        "step {} deleted · {count} flows keep its text as a one-off",
        text.name
    ));
}

fn delete_flows_tab(state: &mut FlowsTab) {
    match state.sub {
        Sub::Flows => {
            let Some(file) = state.flow.clone() else { return };

            if file.layer == Layer::Builtin {
                return state.fail("a built-in flow stays: edit it (a user copy), or duplicate it");
            }

            if let Err(error_value) = state.lib.remove_flow_file(&file.slug, file.layer) {
                return state.fail(error_value);
            }

            state.note = Some(if file.edited {
                format!("{}: back to the built-in", file.name)
            } else {
                format!("flow {} deleted", file.name)
            });
        }
        Sub::Steps => {
            let Some(text) = state.cur_step().cloned() else { return };

            if text.edited {
                return state.fail("an edited built-in: ctrl+d resets it");
            }

            match state.lib.delete_step(&text.name) {
                Ok(count) => {
                    delete_ok_count(&text, count, state);
                }
                Err(error_2) => return state.fail(error_2),
            }
        }
    }

    state.error = None;
    state.reload();
}

fn reset_flows_tab(state: &mut FlowsTab) {
    let (name, layer, edited) = match state.sub {
        Sub::Flows => match &state.flow {
            Some(file) => (file.slug.clone(), file.layer, file.edited),
            None => return,
        },
        Sub::Steps => match state.cur_step() {
            Some(text) => (text.name.clone(), text.layer, text.edited),
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
    let Some(mut file) = state.flow.clone() else { return };

    if state.lib.repo.is_none() {
        return state.fail("0 repo here: start codehead in a repo for repo flows");
    }

    let old = file.layer;

    file.layer = if old == Layer::Repo { Layer::User } else { Layer::Repo };

    if let Err(error) = state.lib.save_flow(&file) {
        return state.fail(error);
    }

    if old != Layer::Builtin {
        let _ = state.lib.remove_flow_file(&file.slug, old);
    }

    state.note = Some(format!("{} → {} flows", file.name, file.layer.label()));
    state.flow = Some(file);
    state.reload();
}

fn apply_ask_flows_tab(state: &mut FlowsTab, what: &Ask, value: &str) -> Result<(), String> {
    match what {
        Ask::NewFlow | Ask::DuplicateFlow => create_flow(state, what, value)?,
        Ask::RenameFlow => {
            apply_ask_ask_renameflow(value, state);
        }
        Ask::NewStep | Ask::DuplicateStep => create_step(state, what, value)?,
        Ask::RenameStep => {
            let Some(text_2) = state.cur_step().cloned() else {
                return Ok(());
            };

            let new = flows::slug(value);

            state.lib.rename_step(&text_2.name, &new)?;
            state.reload();
            state.at = state
                .list()
                .iter()
                .position(|&i| state.lib.steps[i].name == new)
                .unwrap_or(0);
            state.note = Some(format!("step {} → {new}: every flow follows", text_2.name));
        }
        Ask::Promote(i) => {
            let name = flows::slug(value);

            if state.lib.step(&name).is_some() {
                return Err(format!("step {name} exists already"));
            }

            let Some(nd) = state.flow.as_ref().and_then(|file_2| file_2.steps.get(*i)).cloned() else {
                return Ok(());
            };

            let (text_3, node) = flows::promote(&nd, &name);

            state.lib.save_step(&text_3)?;

            if let Some(file_3) = state.flow.as_mut() {
                file_3.steps[*i] = node;
            }

            state.save_flow();
            state.note = Some(format!("step {name} saved: a for it in any flow"));
        }
        Ask::LoopUntil(from, to) => {
            apply_ask_ask_loopuntil_from_to(value, *from, *to, state);
        }
        Ask::Outcome(from, to) => {
            if !flows::valid_name(value) {
                return Err("label: letters, digits, - and _".into());
            }

            let to_id = id_of(*to, state);

            if let Some(nd) = state.flow.as_mut().and_then(|file_5| file_5.steps.get_mut(*from)) {
                apply_ask_message_value(value, to_id, nd);
            }

            state.save_flow();
        }
    }

    Ok(())
}

fn create_flow(state: &mut FlowsTab, what: &Ask, value: &str) -> Result<(), String> {
    let slug = state.lib.free_flow_slug(&flows::slug(value));

    let mut file = match what {
        Ask::DuplicateFlow => state.flow.clone().unwrap_or_default(),
        _ => Flow::default(),
    };

    file.slug.clone_from(&slug);
    file.name = value.to_string();

    file.layer = if file.layer == Layer::Repo {
        Layer::Repo
    } else {
        Layer::User
    };

    file.edited = false;
    state.lib.save_flow(&file)?;
    state.filter.clear();
    state.reload();
    state.at = state
        .list()
        .iter()
        .position(|&i| state.lib.flows[i].slug == slug)
        .unwrap_or(0);
    state.load_flow();
    state.focus = Focus::Rail;

    if file.steps.is_empty() {
        state.picker = Some((input(true, "step name, or text of a one-off step", ""), 0));
    }

    Ok(())
}

fn create_step(state: &mut FlowsTab, what: &Ask, value: &str) -> Result<(), String> {
    let name = flows::slug(value);

    if state.lib.step(&name).is_some() {
        return Err(format!("step {name} exists already"));
    }

    let mut text = match what {
        Ask::DuplicateStep => state.cur_step().cloned().unwrap_or_default(),
        _ => Step::default(),
    };

    text.name.clone_from(&name);
    text.layer = Layer::User;
    text.edited = false;
    state.lib.save_step(&text)?;
    state.filter.clear();
    state.reload();
    state.at = state
        .list()
        .iter()
        .position(|&i| state.lib.steps[i].name == name)
        .unwrap_or(0);

    if let Some(text_value) = state.lib.step(&name).cloned() {
        state.open_step_editor(&text_value);
    }

    Ok(())
}
