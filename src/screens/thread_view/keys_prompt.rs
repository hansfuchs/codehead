use super::{KeyCode, KeyEvent, KeyModifiers, Pane, View};

impl View {
    pub(super) fn key_ask(&mut self, key: KeyEvent) -> Option<bool> {
        let tab = matches!(key.code, KeyCode::Tab | KeyCode::BackTab)
            && !key.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT);
        let complete = tab && !self.ask.focus && !self.thread.asks.is_empty() && self.prompt_dropdown();
        let ask = self.thread.asks.first().cloned()?;

        if (tab && self.panes.pane != Pane::Chat && self.panes.prompt_focus) || complete {
            return None;
        }

        let action = self.ask.key(&ask, key);

        self.ask_act(action).then_some(false)
    }

    pub(super) fn key_prompt_dropdown(&mut self, key: KeyEvent) -> Option<bool> {
        if self.shell_state.cwd_edit.is_some() {
            self.cwd_key(key);
            return Some(false);
        }

        if self.prompt.hist.key(key) {
            take_history_pick(self);
            return Some(false);
        }

        if self
            .prompt
            .mention
            .key(key, &mut self.prompt.input, &self.agents.roster)
        {
            take_mention_pick(self);
            return Some(false);
        }

        if self.prompt.command.key(key, &mut self.prompt.input) {
            return Some(prompt_command(self));
        }

        if self.prompt.shortcuts.key(key, &mut self.prompt.input) || self.prompt.paths.key(key, &mut self.prompt.input)
        {
            return Some(false);
        }

        if shell_completion_key(self, key) {
            return Some(false);
        }

        None
    }
}

fn take_history_pick(view: &mut View) {
    if let Some(text) = view.prompt.hist.picked.take() {
        view.prompt.input.set_text(&text);
    }
}

fn take_mention_pick(view: &mut View) {
    let Some((new, Some(old))) = view.prompt.mention.picked.take() else {
        return;
    };

    view.toast(format!("on send: {new} replaces {old}"));
    view.prompt.replace = Some((new, old));
}

fn prompt_command(view: &mut View) -> bool {
    match view.prompt.command.action.take() {
        Some(crate::features::prompt_input::command::Action::Branch) => return view.branch(),
        Some(crate::features::prompt_input::command::Action::Handoff) => view.handoff(),
        Some(crate::features::prompt_input::command::Action::Reload) => view.reload_sessions(),
        Some(crate::features::prompt_input::command::Action::Flow) => prompt_flow(view),
        None => {}
    }

    false
}

fn prompt_flow(view: &mut View) {
    let agent = view.default_to().into_iter().next().unwrap_or_default();

    view.open_flow(&agent, true);
}

fn shell_completion_key(view: &mut View, key: KeyEvent) -> bool {
    if !view.comp_on() {
        return false;
    }

    let commands = view.shell_cmds();

    view.prompt
        .comp
        .key(key, &mut view.prompt.input, &view.shell_state.shell_cwd, &commands)
        || view.prompt.comp.accept(key, &mut view.prompt.input, &commands)
}
