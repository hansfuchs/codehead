use super::{AGENT_ITEMS, Ctx, KeyCode, KeyEvent, KeyModifiers, Pane, Rect, Region, View, ctx_of};

impl View {
    /// Ctrl+.: the context menu of the region in focus.
    pub(super) fn open_ctx(&mut self) {
        let Some((title, items)) = context_items(self) else {
            return;
        };

        match crate::ui::context_menu::Menu::new(title, items) {
            Some(menu) => set_context_menu(self, menu),
            None => self.toast("0 actions here"),
        }

        self.stream.dirty = true;
    }

    /// A key / click of the context menu.
    pub(super) fn ctx_out(&mut self, out: &crate::ui::context_menu::Out<Ctx>) -> bool {
        use crate::ui::context_menu::Out;
        self.stream.dirty = true;

        match out {
            Out::None => false,
            Out::Close => {
                self.ctx = None;
                false
            }
            Out::Run(action) => run_ctx_action(self, *action),
        }
    }

    /// Where the context menu opens: beside / above / below the thing in focus, never over it.
    pub(super) fn ctx_place(&mut self) -> crate::ui::context_menu::Place {
        use crate::ui::context_menu::Place;

        match self.region() {
            Region::View | Region::Panel => match self.panes.pane {
                Pane::Changes => self.panes.changes.ctx_place(),
                Pane::Files => self.panes.files.ctx_place(),
                Pane::Merge => self.merge_state.merge.ctx_place(self.layout.view_rect),
                Pane::Chat => Place::At(self.layout.input_rect),
            },
            Region::Repos => Place::At(Rect {
                height: 1,
                ..self.layout.view_rect
            }),
            Region::Agents => {
                let at = self.agents.agents_focus.as_ref().map_or(0, |focus| focus.at);
                let agent = self.agents.order.get(at);

                Place::At(
                    self.agents
                        .slots
                        .borrow()
                        .iter()
                        .find(|(name, _)| Some(name) == agent)
                        .map_or(self.layout.input_rect, |(_, rect)| *rect),
                )
            }
            _ => Place::At(self.layout.input_rect),
        }
    }
}

fn repo_context() -> (String, Vec<crate::ui::context_menu::Item<Ctx>>) {
    use crate::ui::context_menu::Item;
    (
        "repos".into(),
        vec![
            Item::new(
                "next repo",
                "→",
                Ctx::Key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE)),
            ),
            Item::new(
                "previous repo",
                "←",
                Ctx::Key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE)),
            ),
        ],
    )
}

fn set_context_menu(view: &mut View, mut menu: crate::ui::context_menu::Menu<Ctx>) {
    if view.region() == Region::Panel {
        menu.head = view.panes.changes.ctx_head();
    }

    view.ctx = Some(menu);
}

fn context_items(view: &mut View) -> Option<(String, Vec<crate::ui::context_menu::Item<Ctx>>)> {
    match view.region() {
        Region::Prompt => Some(prompt_context(view)),
        Region::Agents => agent_context(view),
        Region::Repos => Some(repo_context()),
        Region::Ask => Some((String::new(), Vec::new())),
        Region::View | Region::Panel => Some(pane_context(view)),
    }
}

fn prompt_context(view: &View) -> (String, Vec<crate::ui::context_menu::Item<Ctx>>) {
    use crate::ui::context_menu::Item;
    let mut items = Vec::new();

    if view.shell_mode() {
        items.push(Item::new("search the shell history", "", Ctx::Hist));
    }

    items.extend([
        Item::new("search the chat", "ctrl+f", Ctx::Menu(16)),
        Item::new("tool calls: open / close all", "ctrl+e", Ctx::Menu(1)),
        Item::new("thoughts: open / close all", "ctrl+t", Ctx::Menu(13)),
        Item::new("tokens: details", "ctrl+o", Ctx::Menu(18)),
        Item::new("interrupt the agents", "esc", Ctx::Menu(12)),
        Item::new("IDE on the worktree", "", Ctx::Menu(2)),
        Item::new("yazi on the worktree (its pick: into the prompt)", "", Ctx::Menu(14)),
        Item::new("paste a clipboard image", "ctrl+v", Ctx::Menu(7)),
    ]);
    ("prompt".into(), items)
}

fn agent_context(view: &mut View) -> Option<(String, Vec<crate::ui::context_menu::Item<Ctx>>)> {
    use crate::ui::context_menu::Item;
    let index = view.agents.agents_focus.as_ref().map_or(0, |focus| focus.at);
    let agent = view.agents.order.get(index)?.clone();

    view.agents.menu_agents.clone_from(&view.agents.order);
    let items = ["flow", "remove", "replace with …"]
        .iter()
        .enumerate()
        .map(|(row, label)| Item::new(*label, "", Ctx::Menu(AGENT_ITEMS + index * 3 + row)))
        .collect();

    Some((format!("@{agent}"), items))
}

fn pane_context(view: &mut View) -> (String, Vec<crate::ui::context_menu::Item<Ctx>>) {
    let (title, items) = match view.panes.pane {
        Pane::Changes => view.panes.changes.ctx_items(),
        Pane::Files => view.panes.files.ctx_items(),
        Pane::Merge => view.merge_state.merge.ctx_items(),
        Pane::Chat => (String::new(), Vec::new()),
    };

    let mut items: Vec<_> = items
        .into_iter()
        .map(|item| item.map(|action| ctx_of(&action)))
        .collect();

    if view.panes.pane == Pane::Changes {
        items.extend(changes_context(view));
    }

    (title, items)
}

fn changes_context(view: &View) -> [crate::ui::context_menu::Item<Ctx>; 2] {
    use crate::ui::context_menu::Item;

    let label = if view.panes.changes.panel_shown() {
        "source control panel: hide"
    } else {
        "source control panel: show"
    };

    [
        Item::new(
            label,
            "",
            Ctx::Key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::CONTROL)),
        ),
        Item::new("IDE on the worktree", "", Ctx::Menu(2)),
    ]
}

fn run_ctx_action(view: &mut View, action: Ctx) -> bool {
    view.ctx = None;

    match action {
        Ctx::Key(key) if key.code == KeyCode::Char('b') && key.modifiers == KeyModifiers::CONTROL => {
            toggle_context_panel(view);

            false
        }
        Ctx::Key(key) => view.key(key),
        Ctx::Step(i) => {
            let act = view.panes.changes.ctx_step(i);

            view.changes_act(act);
            false
        }
        Ctx::Cmd(command) => run_ctx_command(view, command),
        Ctx::Menu(id) => view.menu_action(crate::screens::quick_menu::Action::Command(id)),
        Ctx::Hist => {
            let cmds = view.shell_cmds();
            let text = view.prompt.input.text();

            view.prompt
                .hist
                .open(cmds, text.strip_prefix('!').unwrap_or_default().trim_start());
            false
        }
    }
}

fn toggle_context_panel(view: &mut View) {
    view.panes.changes.toggle_panel();

    if !view.panes.changes.panel_shown() && view.region() == Region::Panel {
        view.set_region(Region::View);
    }
}

fn run_ctx_command(view: &mut View, command: &'static str) -> bool {
    match view.panes.pane {
        Pane::Changes => {
            let act = view.panes.changes.ctx_cmd(command);

            view.changes_act(act);
        }
        Pane::Files => {
            let act = view.panes.files.ctx_cmd(command);

            view.files_act(act);
        }
        Pane::Merge => {
            let act = view.merge_state.merge.ctx_cmd(command);

            view.merge_act(act);
        }
        Pane::Chat => {}
    }

    false
}
