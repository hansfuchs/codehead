use super::{Duration, Exit, MouseEvent, View, chat, json};

impl View {
    /// A picker result: redraw, or the answer to the host. True: the picker took the key.
    pub(super) fn ask_act(&mut self, act: crate::features::chat::ask::Act) -> bool {
        match act {
            crate::features::chat::ask::Act::Pass => false,
            crate::features::chat::ask::Act::Changed => {
                self.stream.dirty = true;
                true
            }
            // the flow's wait: its step (flow_answer with the prompt's note, flow_role), 0 host ask
            crate::features::chat::ask::Act::Answer(character)
                if character["id"].as_str().is_some_and(|id| id.starts_with("flow:")) =>
            {
                ask_act_option(self, &(character))
            }
            crate::features::chat::ask::Act::Answer(character) => {
                self.ask.sent = true;
                self.stream.dirty = true;
                self.host("answer", character);
                true
            }
        }
    }

    /// `> branch`: a copy of the thread, its view next.
    /// A key of the flow box asks for ACT.
    pub(super) fn flow_act(&mut self, agent: &str, act: crate::features::flows::flow_bar::Act) {
        match act {
            crate::features::flows::flow_bar::Act::None => {}
            crate::features::flows::flow_bar::Act::Host(op, mut character) => {
                character["agent"] = json!(agent);
                self.host(op, character);
            }
            crate::features::flows::flow_bar::Act::Answer(choice) => {
                // the prompt's text: the owner's note to the agent
                answer_flow(self, agent, &choice);
            }
            crate::features::flows::flow_bar::Act::Edit(slug) => {
                self.exit = Exit::Home(crate::screens::quick_menu::Home::Flows(Some(slug)));
            }
        }
    }

    pub(super) fn mention_open(&self) -> bool {
        self.prompt.mention.is_open(&self.prompt.input, &self.agents.roster)
    }

    /// A dropdown of the prompt shows: @, commands, # shortcuts, paths.
    pub(super) fn prompt_dropdown(&mut self) -> bool {
        self.mention_open()
            || self.prompt.command.is_open(&self.prompt.input)
            || self.prompt.shortcuts.is_open(&self.prompt.input)
            || self.prompt.paths.is_open(&self.prompt.input)
    }

    /// A new ask may take the focus: an empty prompt and 0 keys for 1 s.
    pub(super) fn ask_may_focus(&self) -> bool {
        self.prompt.input.text().trim().is_empty()
            && self
                .last_key
                .is_none_or(|thread| thread.elapsed() >= Duration::from_secs(1))
    }

    /// Bind the picker to the 1st open ask.
    pub(super) fn sync_ask(&mut self) {
        // a flow's wait never takes the keys: the owner may want to write first (tab: the choices)
        let focus = self.ask_may_focus()
            && self
                .thread
                .asks
                .first()
                .is_none_or(|agent| agent.tool != crate::engine::flows::run::WAIT_TOOL);

        self.ask.sync_with(self.thread.asks.first(), focus);
    }

    /// The flows' waits (gate, loop max), 1 per agent, as the last open asks: the agents' asks
    /// first, the question picker answers them. True: the asks changed.
    pub(super) fn place_flow_ask(&mut self) -> bool {
        let want: Vec<chat::Ask> = self
            .flowbars
            .iter()
            .filter_map(crate::features::flows::flow_bar::FlowBar::wait_ask)
            .collect();
        let asks = &mut self.thread.asks;
        let count = asks.len();
        let have: Vec<usize> = (0..count)
            .filter(|&i| asks[i].tool == crate::engine::flows::run::WAIT_TOOL)
            .collect();
        let at_end = have.iter().enumerate().all(|(key, &i)| i == count - have.len() + key);

        if at_end && have.len() == want.len() && have.iter().zip(&want).all(|(&i, width)| asks[i].id == width.id) {
            return false;
        }

        asks.retain(|agent| agent.tool != crate::engine::flows::run::WAIT_TOOL);
        asks.extend(want);

        true
    }

    /// A flow row per member, in the members' order (a new member: its row, a gone one: 0 row).
    pub(super) fn sync_flowbars(&mut self) {
        let mut bars = std::mem::take(&mut self.flowbars);

        for member in &self.agents.members {
            self.flowbars.push(member_flow(&mut bars, member, &self.id));
        }
    }

    /// A mouse event on the flow rows: a click that opens a box closes the others. True: used.
    pub(super) fn flow_mouse(&mut self, event: MouseEvent) -> bool {
        let Some(i) = self.flowbars.iter_mut().position(|bytes| bytes.mouse(event)) else {
            return false;
        };

        if self.flowbars[i].is_open() {
            close_other_flows(self, i);
        }

        true
    }

    /// The flow box of AGENT open (PICK: on the flows to switch to); every other box closed.
    pub(super) fn open_flow(&mut self, agent: &str, pick: bool) {
        if !self.flowbars.iter().any(|bytes| bytes.agent == agent) {
            self.sync_flowbars();
        }

        for bytes in &mut self.flowbars {
            open_flow_branch(agent, pick, bytes);
        }

        self.stream.dirty = true;
    }

    /// `place_flow_ask`, then the picker on the 1st ask.
    pub(super) fn sync_flow_ask(&mut self) {
        let first = self.thread.asks.first().map(|agent| agent.id.clone());

        if !self.place_flow_ask() {
            return;
        }

        if self.thread.asks.first().map(|ask| ask.id.clone()) != first {
            self.sync_ask();
        }

        self.stream.dirty = true;
    }
}

fn open_flow_branch(agent: &str, pick: bool, bytes: &mut crate::features::flows::flow_bar::FlowBar) {
    if bytes.agent != agent {
        bytes.close();
        return;
    }

    if pick {
        bytes.pick();
    } else {
        bytes.open();
    }
}

fn member_flow(
    bars: &mut Vec<crate::features::flows::flow_bar::FlowBar>,
    member: &str,
    id: &str,
) -> crate::features::flows::flow_bar::FlowBar {
    if let Some(index) = bars.iter().position(|flow| flow.agent == member) {
        return bars.remove(index);
    }

    let mut flow = crate::features::flows::flow_bar::FlowBar::new(member);

    flow.tick(id, true);
    flow
}

fn ask_act_option(view: &mut View, character: &serde_json::Value) -> bool {
    let option = character["option"].as_str().unwrap_or_default().to_string();
    let empty = view.prompt.input.text().trim().is_empty();
    let id = character["id"].as_str().unwrap_or_default().to_string();

    let Some(bytes) = view
        .flowbars
        .iter()
        .position(|bytes| bytes.wait_ask().is_some_and(|agent| agent.id == id))
    else {
        return true;
    };

    // reply: the prompt's text as a message to the waiting flow's agent, the wait stays
    if option == crate::features::flows::flow_bar::REPLY {
        reply_to_flow(view, bytes, empty);

        view.stream.dirty = true;
        return true;
    }

    // a gate's outcome back to a step: 0 send without the owner's words what to change
    if empty && option != "approve" && view.flowbars[bytes].waits_gate() {
        view.ask.focus = false;
        view.toast(format!(
            "{option}: type what to change in the prompt first, then pick it again"
        ));
        view.stream.dirty = true;
        return true;
    }

    if let Some(act) = view.flowbars[bytes].wait_answer(&option) {
        let agent = view.flowbars[bytes].agent.clone();

        view.flow_act(&agent, act);
    }

    view.sync_flow_ask();
    view.stream.dirty = true;

    true
}

fn close_other_flows(view: &mut View, index: usize) {
    view.flowbars
        .iter_mut()
        .enumerate()
        .filter(|(other, _)| *other != index)
        .for_each(|(_, flow)| flow.close());
}

fn reply_to_flow(view: &mut View, bytes: usize, empty: bool) {
    view.ask.focus = false;

    if empty {
        view.toast("reply: type the message in the prompt first (ctrl+enter sends it too)");
    } else {
        view.agents.reply_to = Some(view.flowbars[bytes].agent.clone());
        view.submit();
        view.agents.reply_to = None;
    }
}

fn answer_flow(view: &mut View, agent: &str, choice: &str) {
    let note = view.prompt.input.text().trim().to_string();

    if !note.is_empty() {
        view.prompt.input.load_text("");
    }

    view.host(
        "flow_answer",
        json!({"op": "flow_answer", "agent": agent, "choice": choice, "note": note}),
    );
    view.toast(format!("flow: {choice}"));
}
