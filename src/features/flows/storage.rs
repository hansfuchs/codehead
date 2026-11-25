use super::{Flow, FlowsTab, Library, Step, flows};

impl FlowsTab {
    pub(super) fn fail(&mut self, error: impl Into<String>) {
        self.error = Some(error.into());
    }
    /// The open flow to disk (a built-in: a user file over it), the library read again.
    pub(super) fn save_flow(&mut self) {
        let Some(flow) = self.flow.clone() else { return };

        match self.lib.save_flow(&flow) {
            Ok(_) => {
                reload_after_flow_save(&flow, self);
            }
            Err(error) => self.fail(format!("save: {error}")),
        }
    }

    pub(super) fn save_step(&mut self, step: &Step) -> bool {
        match self.lib.save_step(step) {
            Ok(_) => reload_after_step_save(self),
            Err(error) => {
                self.fail(format!("save: {error}"));
                false
            }
        }
    }

    /// The shared step of node I as a one-off of this flow (its text copied in).
    pub(super) fn fork(&mut self, i: usize) {
        let Some(node) = self.flow.as_ref().and_then(|flow| flow.steps.get(i)).cloned() else {
            return;
        };

        let Some(step) = node
            .uses
            .as_deref()
            .and_then(|step_name| self.lib.step(step_name))
            .cloned()
        else {
            return;
        };

        if let Some(flow) = self.flow.as_mut() {
            flow.steps[i] = flows::inline(&node, &step);
        }

        self.save_flow();
        self.note = Some(format!("{}: a one-off of this flow now", step.name));
        self.open_node_editor(i);
    }
    pub(crate) fn paste(&mut self, text: &str) {
        if let Some((_, answer)) = self.ask.as_mut() {
            answer.insert(text);
        } else if let Some((query, _)) = self.picker.as_mut() {
            query.insert(text);
        } else if let Some(editor) = self.editor.as_mut() {
            editor.fields[editor.field].1.insert(text);
        }
    }
}

fn reload_after_flow_save(flow: &Flow, tab: &mut FlowsTab) {
    tab.error = None;
    tab.lib = Library::load(tab.dir.as_deref());

    if let Some(saved) = tab.lib.flows.iter().find(|x| x.slug == flow.slug).cloned() {
        tab.flow = Some(saved);
    }
}

fn reload_after_step_save(tab: &mut FlowsTab) -> bool {
    tab.lib = Library::load(tab.dir.as_deref());

    if let Some(reloaded) = tab
        .flow
        .as_ref()
        .and_then(|flow| tab.lib.flows.iter().find(|x| x.slug == flow.slug))
        .cloned()
    {
        tab.flow = Some(reloaded);
    }

    true
}
