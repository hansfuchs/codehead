use super::{Act, App, Cmd, Field, KEYS_ITEM, Screen, StartCtx, Tab};

impl App {
    pub(crate) fn set_prompt(&mut self, text: &str) {
        self.compose.prompt.set_text(text);
        self.backup();
    }

    pub(crate) fn prompt_text(&self) -> String {
        self.compose.prompt.text()
    }

    pub(super) fn commands(&self) -> (Vec<Cmd>, Vec<(&'static str, &'static str)>) {
        match (self.compose.screen, self.tab) {
            (Screen::Compose, _) => commands_compose(),
            (Screen::Pick, Tab::Notes) => commands_notes(),
            (Screen::Pick, Tab::Threads) => commands_threads(),
            (Screen::Pick, Tab::Agents | Tab::New) => commands_agents(),
            (Screen::Pick, Tab::Merge) => commands_merge(),
            (Screen::Pick, Tab::Flows) => commands_flows(),
            (Screen::Pick, Tab::Projects) => commands_projects(),
            (Screen::Pick, Tab::Settings) => commands_settings(),
        }
    }

    /// Ctrl+.: the commands of this screen; the agents field: the card's flow first.
    pub(super) fn open_ctx(&mut self) {
        use crate::ui::context_menu::Item;
        let mut items = Vec::new();

        let title = match self.cur_tab() {
            Tab::New => "task",
            Tab::Notes => "notes",
            Tab::Threads => "threads",
            Tab::Projects => "projects",
            Tab::Merge => "merge",
            Tab::Agents => "agents",
            Tab::Flows => "flows",
            Tab::Settings => "settings",
        };

        if self.compose.screen == Screen::Compose
            && !self.tabs_focus
            && self.compose.field == Field::Agents
            && self.agent_list.agents.get(self.compose.pick_cursor).is_some()
        {
            items.push(Item::new(
                "flow of this agent (a flow picks the agent)",
                "",
                StartCtx::Flow,
            ));
        }

        let (cmds, _) = self.commands();

        items.extend(cmds.into_iter().filter(|cmd| cmd.act != Act::Quit).map(ctx_item));

        if let Some(menu) = crate::ui::context_menu::Menu::new(title, items) {
            self.overlays.ctx = Some(menu);
        }
    }

    pub(super) fn ctx_out(&mut self, out: crate::ui::context_menu::Out<StartCtx>) {
        use crate::ui::context_menu::Out;

        match out {
            Out::None => {}
            Out::Close => self.overlays.ctx = None,
            Out::Run(action) => {
                self.overlays.ctx = None;

                match action {
                    StartCtx::Flow => self.open_flow_menu(),
                    StartCtx::Run(act) => self.run(act),
                }
            }
        }
    }

    pub(super) fn open_menu(&mut self) {
        let (cmds, help) = self.commands();
        let mut items: Vec<crate::screens::quick_menu::Item> = cmds
            .iter()
            .enumerate()
            .map(|(i, cmd)| crate::screens::quick_menu::Item {
                label: cmd.label.to_string(),
                key: cmd.key.to_string(),
                id: i,
                section: "this screen",
            })
            .collect();

        items.push(crate::screens::quick_menu::Item {
            label: "keys of this screen".into(),
            key: String::new(),
            id: KEYS_ITEM,
            section: "help",
        });
        self.overlays.menu = Some(crate::screens::quick_menu::Menu::new(
            items,
            None,
            self.agent_list.agents.clone(),
            Vec::new(),
        ));
        self.overlays.menu_cmds = cmds;
        self.overlays.menu_help = help;
    }
}

fn ctx_item(cmd: Cmd) -> crate::ui::context_menu::Item<StartCtx> {
    crate::ui::context_menu::Item::new(
        cmd.label,
        if cmd.key == "menu" { "" } else { cmd.key },
        StartCtx::Run(cmd.act),
    )
}

fn commands_compose() -> (Vec<Cmd>, Vec<(&'static str, &'static str)>) {
    (
        vec![
            Cmd::new("ctrl+enter", "start the thread (also cmd+enter, alt+enter)", Act::Start),
            // no hotkey: ⌘→ arrives as ctrl+e in macOS terminals (it opened the editor)
            Cmd::new("menu", "edit the prompt in $EDITOR", Act::EditPrompt),
            Cmd::new("enter on notes", "pick notes", Act::OpenNotes),
            Cmd::new(
                "ctrl+s",
                "save the task as a draft for later, start over",
                Act::SaveDraft,
            ),
            Cmd::new(
                "menu",
                "reset: where from the where row, 0 notes, 0 Claude Code context (asks first)",
                Act::Reset,
            ),
            Cmd::new("esc esc", "quit", Act::Quit),
        ],
        vec![
            ("↑↓", "field (↑ on the prompt's 1st row leaves it)"),
            ("tab / shift+tab", "tabs row ↔ the task"),
            ("ctrl+.", "actions of the field: agents card → its flow"),
            ("F1-F7 · alt+1-7", "main tabs"),
            ("enter", "title: next field · notes: pick · prompt: new line"),
            ("x on notes", "drop notes + Claude Code context"),
            (
                "enter",
                "where: dropdown · current branch · new branch · new worktree · recent branches · type: filter",
            ),
            ("↑↓ space", "below where: checkout · git stash boxes, 1 per row"),
            ("@", "prompt: agent names (↑↓ tab / enter, esc)"),
            ("ctrl+v", "prompt: clipboard image → [Image #n] (dropped path too)"),
            ("←→ space", "agents: move · pick / unpick"),
            ("shift+←→↑↓", "select text"),
            ("mouse", "click: cursor · drag: select + copy"),
            ("dbl / triple", "select word / line"),
            ("bksp", "remove char or selection"),
            ("ctrl+shift+a c x v", "all · copy · cut · paste"),
            ("ctrl+a e · ⌘←→", "row start · row end"),
            ("ctrl+u k · ⌘⌫", "delete to row start · row end"),
            ("ctrl+z · ⌘z", "undo"),
            ("ctrl+shift+z · ⌘⇧z · ctrl+y", "redo"),
            ("ctrl+←→", "word left / right"),
        ],
    )
}

fn commands_notes() -> (Vec<Cmd>, Vec<(&'static str, &'static str)>) {
    (
        vec![
            Cmd::new("ctrl+f", "filter by name + content", Act::Filter),
            Cmd::new("d", "delete the note / folder under the cursor", Act::DeleteNote),
            Cmd::new("e", "edit the note in $EDITOR", Act::EditNote),
            Cmd::new("y", "copy the picked note paths", Act::CopyPaths),
            Cmd::new("x", "clear the selection", Act::ClearSelection),
            Cmd::new("c", "collapse all folders", Act::CollapseAll),
            Cmd::new("o", "expand all folders", Act::ExpandAll),
            Cmd::new("r", "reload from disk", Act::Reload),
            Cmd::new("menu", "quit codehead", Act::Quit),
        ],
        vec![
            ("↑↓", "move"),
            ("←→", "fold / unfold folder"),
            ("space", "select note / folder"),
            ("enter", "confirm selection → new (0 notes allowed)"),
            ("pgup pgdn", "scroll preview"),
            ("esc", "clear filter · back to new"),
            ("esc esc", "back to new"),
        ],
    )
}

fn commands_threads() -> (Vec<Cmd>, Vec<(&'static str, &'static str)>) {
    (
        vec![
            Cmd::new("ctrl+f", "filter thread names (fuzzy)", Act::ThreadFilter),
            Cmd::new(
                "a",
                "archive: worktree goes, branch stays (the selection too)",
                Act::Thread(crate::features::thread_list::Act::Archive),
            ),
            Cmd::new(
                "u",
                "unarchive: the host makes the worktree again",
                Act::Thread(crate::features::thread_list::Act::Unarchive),
            ),
            Cmd::new(
                "e",
                "edit: title, repo, linked repos, agents",
                Act::Thread(crate::features::thread_list::Act::Edit),
            ),
            Cmd::new(
                "d",
                "delete thread + worktree (+ branch, if pushed)",
                Act::DeleteThreads,
            ),
            Cmd::new("r", "reload", Act::Reload),
            Cmd::new("menu", "quit codehead", Act::Quit),
        ],
        vec![
            ("↑↓", "move · ↑ on the top card: view tabs, repo tabs, tabs"),
            ("←→", "previous / next repo (on the view tabs: the view)"),
            ("space", "select thread"),
            (
                "enter",
                "open thread (worktree: new task there; claude: new thread, last response as context)",
            ),
            ("ctrl+.", "actions of the thread / the selection"),
            (
                "c",
                "thread / claude session: last response in / out of the task context",
            ),
            ("F1-F7 · alt+1-7", "switch tab"),
            ("pgup pgdn", "scroll preview"),
            ("esc", "clear filter · clear selection"),
        ],
    )
}

fn commands_agents() -> (Vec<Cmd>, Vec<(&'static str, &'static str)>) {
    (
        vec![
            Cmd::new(
                "ctrl+f",
                "filter agents (fuzzy: name, harness, model)",
                Act::AgentFilter,
            ),
            Cmd::new("n", "new agent", Act::NewAgent),
            Cmd::new("e", "edit agent (also enter)", Act::EditAgent),
            Cmd::new(
                "d",
                "delete agent: its threads move to the default agent",
                Act::DeleteAgent,
            ),
            Cmd::new("r", "reload", Act::Reload),
            Cmd::new("menu", "quit codehead", Act::Quit),
        ],
        vec![
            ("↑↓←→", "move in the card grid · ↑ on top row: tabs"),
            ("enter", "edit agent"),
            ("ctrl+f", "filter: enter / esc back to the cards"),
            ("F1-F7 · alt+1-7", "switch tab"),
            ("pgup pgdn", "scroll preview"),
            ("esc", "clear the filter"),
        ],
    )
}

fn commands_merge() -> (Vec<Cmd>, Vec<(&'static str, &'static str)>) {
    (
        vec![
            Cmd::new("r", "read the pull requests again", Act::Reload),
            Cmd::new("menu", "quit codehead", Act::Quit),
        ],
        vec![
            ("↑↓ / j k", "move: PRs; in a PR: the things to act on"),
            ("enter", "open the PR · act (edit, dropdown, merge, update)"),
            ("o / ctrl+click", "open on GitHub"),
            ("←→", "state tabs: open, merged, closed, all"),
            ("[ ] · < >", "previous / next PR · overview / changes"),
            (
                "↑ on top · ←→",
                "PR / views row: switch PR / overview, changes (the diff too)",
            ),
            ("tab", "the PR's prompt (a new thread with this task); tab / esc: back"),
            ("esc", "back to the list"),
        ],
    )
}

fn commands_flows() -> (Vec<Cmd>, Vec<(&'static str, &'static str)>) {
    (
        vec![
            Cmd::new("r", "read the flow and step files again", Act::Reload),
            Cmd::new("menu", "quit codehead", Act::Quit),
        ],
        vec![
            ("←→", "flows / steps"),
            ("enter", "flow: its rail · step: edit"),
            ("n D R", "new · duplicate · rename"),
            (
                "d d · ctrl+d ctrl+d · r",
                "delete · reset an edited built-in · read again",
            ),
            ("m", "flow: user ↔ repo (.codehead/flows)"),
            ("rail: a enter J K", "add · edit · move a step"),
            ("rail: g o L + -", "gate · gate outcome · loop · loop max"),
            ("rail: s", "save a one-off as a shared step"),
            ("editor: ctrl+g ctrl+f", "gate · fork a shared step into a one-off"),
        ],
    )
}

fn commands_settings() -> (Vec<Cmd>, Vec<(&'static str, &'static str)>) {
    (
        vec![
            Cmd::new("menu", "add path alias (also enter on + add)", Act::AddAlias),
            Cmd::new("ctrl+r", "reload file", Act::Reload),
            Cmd::new("menu", "quit codehead", Act::Quit),
        ],
        vec![
            ("↑↓", "field / alias box"),
            ("enter", "alias box: edit / done"),
            ("d d", "delete the highlighted alias"),
            ("F1-F7 · alt+1-7", "switch tab"),
            ("esc", "close path suggestions"),
            ("auto", "changes apply on leaving a field"),
        ],
    )
}

impl Cmd {
    fn new(key: &'static str, label: &'static str, act: Act) -> Self {
        Self { key, label, act }
    }
}

fn commands_projects() -> (Vec<Cmd>, Vec<(&'static str, &'static str)>) {
    (
        vec![
            Cmd::new("r", "read the briefs again", Act::Reload),
            Cmd::new("menu", "quit codehead", Act::Quit),
        ],
        crate::features::projects::ProjectsTab::help().to_vec(),
    )
}
