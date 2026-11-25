use super::{Exit, KeyEvent, View, json, store};

impl View {
    pub(super) fn key_flow(&mut self, key: KeyEvent) -> Option<bool> {
        if let Some((agent, act)) = self
            .flowbars
            .iter_mut()
            .find_map(|bar| bar.key(key).map(|act| (bar.agent.clone(), act)))
        {
            let edit = matches!(act, crate::features::flows::flow_bar::Act::Edit(_));

            self.flow_act(&agent, act);
            self.stream.dirty = true;
            return Some(edit);
        }

        None
    }
    pub(super) fn key_subthreads(&mut self, key: KeyEvent) -> Option<bool> {
        if let Some(act) = self.subthreads.key(key) {
            return Some(subthread_act(self, act));
        }

        None
    }

    pub(super) fn key_activity(&mut self, key: KeyEvent) -> Option<bool> {
        if let Some(act) = self.subagents.key(key, &self.thread.subs) {
            stop_subagent(self, act);
            return Some(false);
        }

        if self.monitors.key(key) {
            take_monitor_note(self);

            return Some(false);
        }

        None
    }
}

fn subthread_act(view: &mut View, act: crate::features::activity_rows::subthreads::Act) -> bool {
    match act {
        crate::features::activity_rows::subthreads::Act::Stop(kid) => view.stop_sub(&kid),
        crate::features::activity_rows::subthreads::Act::Open(kid) => {
            return open_subthread(view, &kid);
        }
        crate::features::activity_rows::subthreads::Act::None => {}
    }

    false
}

fn open_subthread(view: &mut View, kid: &str) -> bool {
    let Some(thread) = store::load(kid) else {
        return false;
    };

    view.exit = Exit::Open {
        id: thread.id.clone(),
        dir: thread.work_dir(),
        title: thread.title.clone(),
        note: "sub-thread · esc esc: back to its thread".into(),
    };
    true
}

fn take_monitor_note(view: &mut View) {
    if let Some(note) = view.monitors.take_note() {
        view.toast(note);
    }
}

fn stop_subagent(view: &mut View, act: crate::features::activity_rows::subagents::Act) {
    if let crate::features::activity_rows::subagents::Act::Stop(id) = act {
        let what = view
            .thread
            .subs
            .iter()
            .find(|sub| sub.id == id)
            .map(|sub| sub.description.clone())
            .unwrap_or_default();

        view.host("sub_stop", json!({"op": "sub_stop", "id": id}));
        view.toast(format!("stopping subagent “{what}”…"));
    }
}
