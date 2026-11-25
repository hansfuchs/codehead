use super::{Msg, Runtime, Value};

impl Runtime {
    pub(super) fn message(&mut self, message: Msg) {
        self.dirty = true;

        match message {
            Msg::Done(command, result) => self.command_done(command, result),
            Msg::Cost(snapshot) => self.view.cost = Some(*snapshot),
            Msg::Shell(command, output, code, directory) => {
                self.view.shell_done(command, output, code, &directory);
                self.wake = true;
            }
            Msg::Head(head) => self.view.take_head(&(head)),
            Msg::Toast(message) => self.view.toast(message),
            Msg::ShellCwd(directory) => {
                self.view.shell_state.shell_cwd = directory;
                self.view.stream.dirty = true;
            }
            Msg::Wake => self.wake = true,
            Msg::Subscribed => {
                self.view.stream.live.clear();
                self.view.stream.dirty = true;
                self.wake = true;
            }
            Msg::Live(agent, text) => {
                self.view.stream.live.retain(|(live_agent, _)| *live_agent != agent);
                self.view.stream.live.push((agent, text));
                self.view.stream.dirty = true;
            }
            Msg::Delta(agent, text) => self.delta(agent, text),
        }
    }

    fn command_done(&mut self, command: &str, result: Result<Value, String>) {
        match (command, result) {
            ("send", Err(error)) => self.send_failed(&error),
            ("answer", Err(error)) => {
                self.view.ask.sent = false;
                self.view.toast(format!("answer: {error}"));
            }
            (command, Err(error)) => self.view.toast(format!("{command}: {error}")),
            ("join" | "leave" | "replace", Ok(_)) => {
                let roster = self.view.agents.roster.clone();

                self.view.set_roster(roster);
            }
            _ => {}
        }
    }

    fn send_failed(&mut self, error: &str) {
        let error = error.lines().next().unwrap_or_default().to_string();

        if self.view.prompt.input.text().trim().is_empty()
            && let Some(index) = self.view.prompt.pending.iter().rposition(|pending| pending.2.is_none())
        {
            let (text, ..) = self.view.prompt.pending.remove(index);

            self.view.prompt.input.set_text(&text);
            self.view.prompt.input.move_end();
            self.view.save_prompt();
            self.view
                .toast(format!("not sent: {error} · the message is back in the prompt"));
        } else if let Some(pending) = self
            .view
            .prompt
            .pending
            .iter_mut()
            .rev()
            .find(|pending| pending.2.is_none())
        {
            pending.2 = Some(error);
        }
    }

    fn delta(&mut self, agent: String, text: String) {
        match self
            .view
            .stream
            .live
            .iter_mut()
            .find(|(live_agent, _)| *live_agent == agent)
        {
            Some((_, live)) => live.push_str(&text),
            None => self.view.stream.live.push((agent, text)),
        }

        self.view.stream.dirty = true;
    }
}
