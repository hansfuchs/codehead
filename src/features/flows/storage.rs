use super::{Flow, FlowsTab, Library, Step, flows};

impl FlowsTab {
    pub(super) fn fail(&mut self, error_value: impl Into<String>) {
        self.error = Some(error_value.into());
    }
    /// The open flow to disk (a built-in: a user file over it), the library read again.
    pub(super) fn save_flow(&mut self) {
        let Some(file) = self.flow.clone() else { return };

        match self.lib.save_flow(&file) {
            Ok(_) => {
                save_flow_ok(&file, self);
            }
            Err(error_value) => self.fail(format!("save: {error_value}")),
        }
    }

    pub(super) fn save_step(&mut self, text: &Step) -> bool {
        match self.lib.save_step(text) {
            Ok(_) => save_step_ok(self),
            Err(error) => {
                self.fail(format!("save: {error}"));
                false
            }
        }
    }

    /// The shared step of node I as a one-off of this flow (its text copied in).
    pub(super) fn fork(&mut self, i: usize) {
        let Some(count) = self.flow.as_ref().and_then(|file| file.steps.get(i)).cloned() else {
            return;
        };

        let Some(text) = count.uses.as_deref().and_then(|user| self.lib.step(user)).cloned() else {
            return;
        };

        if let Some(file_value) = self.flow.as_mut() {
            file_value.steps[i] = flows::inline(&count, &text);
        }

        self.save_flow();
        self.note = Some(format!("{}: a one-off of this flow now", text.name));
        self.open_node_editor(i);
    }
    pub(crate) fn paste(&mut self, text: &str) {
        if let Some((_, ta)) = self.ask.as_mut() {
            ta.insert(text);
        } else if let Some((ta, _)) = self.picker.as_mut() {
            ta.insert(text);
        } else if let Some(ed) = self.editor.as_mut() {
            ed.fields[ed.field].1.insert(text);
        }
    }
}

fn save_flow_ok(file: &Flow, view_state: &mut FlowsTab) {
    view_state.error = None;
    view_state.lib = Library::load(view_state.dir.as_deref());

    if let Some(nf) = view_state.lib.flows.iter().find(|x| x.slug == file.slug).cloned() {
        view_state.flow = Some(nf);
    }
}

fn save_step_ok(view_state: &mut FlowsTab) -> bool {
    view_state.lib = Library::load(view_state.dir.as_deref());

    if let Some(file_value) = view_state
        .flow
        .as_ref()
        .and_then(|file| view_state.lib.flows.iter().find(|x| x.slug == file.slug))
        .cloned()
    {
        view_state.flow = Some(file_value);
    }

    true
}
