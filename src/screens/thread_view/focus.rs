use super::{AgentsFocus, Pane, Region, View};

impl View {
    /// ctrl+m: the keys to the prompt; every box, filter and dropdown that holds them lets go.
    pub(super) fn focus_prompt(&mut self) {
        self.menu = None;
        self.notes_panel = None;
        self.ide.ide_pick = None;
        self.agents.agents_focus = None;
        self.find.focus = false;
        self.flowbars
            .iter_mut()
            .for_each(crate::features::flows::flow_bar::FlowBar::close);
        self.ask.focus = false;
        self.panes.prompt_focus = self.panes.pane != Pane::Chat;
        self.stream.dirty = true;
    }

    /// The agents box has the keys, AGENT highlighted (None: the one a message goes to now).
    pub(super) fn focus_agents(&mut self, agent: Option<&str>) {
        if self.agents.order.is_empty() {
            self.toast("0 agents on the thread · @name in the prompt brings one in");
            return;
        }

        let want = agent.map(String::from).or_else(|| self.default_to().into_iter().next());
        let at = want
            .and_then(|width| self.agents.order.iter().position(|agent_value| *agent_value == width))
            .unwrap_or(0);

        self.agents.agents_focus = Some(AgentsFocus {
            at,
            ..Default::default()
        });
        self.find.focus = false;
        self.flowbars
            .iter_mut()
            .for_each(crate::features::flows::flow_bar::FlowBar::close);

        // the box shows in the chat (and next to a wide changes view)
        if self.panes.pane != Pane::Chat && !self.layout.wide_split {
            self.set_pane(Pane::Chat);
        }

        self.stream.dirty = true;
    }

    /// The regions tab walks, in order: the prompt, the view (changes, merge, files), the panel,
    /// the repo tabs, an open question, the agents box (the chat; a wide changes view too).
    pub(super) fn regions(&self) -> Vec<Region> {
        let mut result = vec![Region::Prompt];

        if self.panes.pane != Pane::Chat {
            regions_body(self, &mut result);
        }

        if !self.thread.asks.is_empty() {
            result.push(Region::Ask);
        }

        if !self.agents.order.is_empty() && (self.panes.pane == Pane::Chat || self.layout.wide_split) {
            result.push(Region::Agents);
        }

        result
    }

    /// The region with the keys.
    pub(super) fn region(&self) -> Region {
        if self.agents.agents_focus.is_some() {
            Region::Agents
        } else if self.ask.focus && !self.thread.asks.is_empty() {
            Region::Ask
        } else if self.panes.pane != Pane::Chat && !self.panes.prompt_focus {
            region_branch(self)
        } else {
            Region::Prompt
        }
    }

    /// The keys to region R; every box, bar and dropdown that holds them lets go.
    pub(super) fn set_region(&mut self, result: Region) {
        self.agents.agents_focus = None;
        self.ask.focus = false;
        self.checkout_state.repo_focus = false;
        self.find.focus = false;
        self.flowbars
            .iter_mut()
            .for_each(crate::features::flows::flow_bar::FlowBar::close);
        let side = self.panes.pane != Pane::Chat;

        match result {
            Region::Prompt => self.panes.prompt_focus = side,
            Region::View | Region::Panel => {
                focus_view_region(self, result);
            }
            Region::Repos => {
                self.panes.prompt_focus = false;
                self.checkout_state.repo_focus = true;
            }
            Region::Ask => {
                self.panes.prompt_focus = side;
                self.ask.focus = true;
            }
            Region::Agents => {
                self.panes.prompt_focus = side;
                self.focus_agents(None);
            }
        }

        self.stream.dirty = true;
    }

    pub(super) fn cycle(&mut self, directory: isize) {
        let rs = self.regions();
        let at = rs.iter().position(|x| *x == self.region()).unwrap_or(0) as isize;
        let to = rs[(at + directory).rem_euclid(rs.len() as isize) as usize];

        self.set_region(to);
    }

    /// Tab stays inside: a dropdown of the prompt, shell mode (completion), the shell dir input,
    /// the history search, a text box of a view (a comment, the commit message, a filter, a PR
    /// title / body).
    pub(super) fn tab_is_local(&mut self) -> bool {
        match self.region() {
            Region::Prompt => {
                self.prompt_dropdown()
                    || self.shell_mode()
                    || self.shell_state.cwd_edit.is_some()
                    || self.prompt.hist.is_open()
            }
            Region::View | Region::Panel => match self.panes.pane {
                Pane::Changes => self.panes.changes.typing(),
                Pane::Merge => self.merge_state.merge.typing(),
                Pane::Files => self.panes.files.typing(),
                Pane::Chat => false,
            },
            _ => false,
        }
    }
}

fn regions_body(view: &View, result: &mut Vec<Region>) {
    result.push(Region::View);

    if view.panes.pane == Pane::Changes && view.panes.changes.panel_shown() {
        result.push(Region::Panel);
    }

    if view.panes.pane == Pane::Changes && view.checkout_state.checkouts.len() > 1 {
        result.push(Region::Repos);
    }
}

fn region_branch(view: &View) -> Region {
    if view.panes.pane == Pane::Changes && view.checkout_state.repo_focus {
        Region::Repos
    } else if view.panes.pane == Pane::Changes && view.panes.changes.panel_focus() {
        Region::Panel
    } else {
        Region::View
    }
}

/// Tab (D 1) / shift+tab (D -1): the next / previous region.
fn focus_view_region(view: &mut View, result: Region) {
    view.panes.prompt_focus = false;

    if view.panes.pane == Pane::Changes {
        view.panes.changes.focus_panel(result == Region::Panel);
    }
}
