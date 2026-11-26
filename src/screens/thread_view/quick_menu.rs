use super::{AGENT_ITEMS, Exit, KeyCode, KeyEvent, KeyModifiers, View, open_pr};

impl View {
    pub(super) fn open_menu(&mut self) {
        let items = self.menu_items();

        // cards: read in the background by the menu (0 wait on open)
        self.menu = Some(crate::screens::quick_menu::Menu::new(
            items,
            Some(self.id.clone()),
            self.agents.roster.clone(),
            Vec::new(),
        ));
    }

    /// The quick commands of the menu (per agent: its flow, remove, replace). Also sets
    /// `menu_agents`, the agents the per-agent ids point to.
    pub(super) fn menu_items(&mut self) -> Vec<crate::screens::quick_menu::Item> {
        let mut items = vec![
            menu_item("IDE on the repo root (several repos: pick 1)", "", 2, "thread"),
            menu_item("interrupt the agents", "esc", 12, "thread"),
            menu_item("yazi on the worktree (its pick: into the prompt)", "", 14, "thread"),
            menu_item("views: chat · changes · merge · files", "F1-F4", 15, "thread"),
            menu_item("agents box: flow, remove, replace", "tab", 17, "agents"),
        ];

        self.agents.menu_agents.clone_from(&self.agents.order);

        for (index, agent) in self.agents.order.iter().enumerate() {
            items.extend(agent_menu_items(self, index, agent));
        }

        items.extend(prompt_menu_items());
        items
    }

    pub(super) fn menu_action(&mut self, action: crate::screens::quick_menu::Action) -> bool {
        use crate::screens::quick_menu::Action;

        match action {
            Action::None => return false,
            Action::Close => {}
            Action::Back => {
                self.exit = Exit::Back;
                return true;
            }
            Action::Home(home) => {
                self.exit = Exit::Home(home);
                return true;
            }
            Action::Pr(entry) => return menu_pull_request(self, entry),
            Action::Open(thread) => return menu_thread(self, thread),
            Action::Jump(thread, time) => return menu_jump(self, thread, time),
            Action::Command(id) => return menu_command(self, id),
        }

        self.menu = None;
        false
    }

    /// A key / click of the notifications dropdown: a jump goes to the note's event.
    pub(super) fn notes_pick(&mut self, pick: crate::features::notifications::Pick) -> bool {
        use crate::features::notifications::Pick;
        self.stream.dirty = true;

        match pick {
            Pick::Stay => false,
            Pick::Close => {
                self.notes_panel = None;
                false
            }
            Pick::Jump(note) => {
                self.notes_panel = None;

                note_jump(self, &note)
            }
        }
    }
}

fn menu_item(
    label: impl Into<String>,
    key: impl Into<String>,
    id: usize,
    section: &'static str,
) -> crate::screens::quick_menu::Item {
    crate::screens::quick_menu::Item {
        label: label.into(),
        key: key.into(),
        id,
        section,
    }
}
fn prompt_menu_items() -> Vec<crate::screens::quick_menu::Item> {
    vec![
        menu_item("search the chat", "ctrl+f", 16, "chat"),
        menu_item("tool calls: open / close all", "ctrl+e", 1, "chat"),
        menu_item("thoughts: open / close all", "ctrl+t", 13, "chat"),
        menu_item("tokens: details (or a click on the tokens box)", "ctrl+o", 18, "chat"),
        menu_item("scroll the stream", "wheel · pgup", 10, "chat"),
        menu_item(
            "select + copy · double click: word · click code: copy",
            "drag",
            9,
            "chat",
        ),
        menu_item("send · new line", "⌘/ctrl+⏎ · ⏎", 11, "prompt"),
        menu_item("agents: mention · →: replace an agent", "@", 4, "prompt"),
        menu_item(
            "commands: files, notes, threads, skills, branch",
            crate::infra::config::command_key().to_string(),
            5,
            "prompt",
        ),
        menu_item("paste a clipboard image", "ctrl+v", 7, "prompt"),
    ]
}

fn agent_menu_items(view: &View, index: usize, agent: &str) -> [crate::screens::quick_menu::Item; 3] {
    let flow = view
        .flowbars
        .iter()
        .find(|flow| flow.agent == agent)
        .and_then(|flow| flow.state.as_ref())
        .map_or_else(
            || "0 flow".to_string(),
            |state| format!("{} · step {}", state.title, state.node + 1),
        );
    let labels = [
        format!("@{agent}: flow ({flow})"),
        format!("@{agent}: remove"),
        format!("@{agent}: replace with …"),
    ];
    let mut row = 0;

    labels.map(|label| {
        let id = AGENT_ITEMS + index * 3 + row;

        row += 1;
        menu_item(label, "", id, "agents")
    })
}

fn menu_command(view: &mut View, id: usize) -> bool {
    if id == 3 {
        view.exit = Exit::Back;
        return true;
    }

    view.menu = None;

    match id {
        1 => {
            view.stream.all_runs = !view.stream.all_runs;
            view.stream.dirty = true;
        }
        13 => view.toggle_thoughts(),
        18 => view.cost_box.toggle(),
        2 => {
            view.ide_key();
            view.stream.dirty = true;
        }
        4 | 5 => menu_insert(view, id),
        7 => return view.key(KeyEvent::new(KeyCode::Char('v'), KeyModifiers::CONTROL)),
        12 => view.interrupt(),
        14 => view.panes.yazi = Some(view.cwd.clone()),
        16 => return view.key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL)),
        17 => view.focus_agents(None),
        id if id >= AGENT_ITEMS => return menu_agent_row(view, id),
        _ => {}
    }

    false
}

fn menu_insert(view: &mut View, id: usize) {
    view.prompt.input.move_end();
    let text = view.prompt.input.text();

    let gap = if text.is_empty() || text.ends_with(char::is_whitespace) {
        ""
    } else {
        " "
    };

    let prefix = if id == 4 {
        "@".to_string()
    } else {
        crate::infra::config::command_key().to_string()
    };

    view.prompt.input.insert(&format!("{gap}{prefix}"));
}

fn menu_thread(view: &mut View, thread: crate::ui::thread_card::Card) -> bool {
    view.menu = None;

    if thread.id == view.id {
        return false;
    }

    view.exit = Exit::Open {
        id: thread.id.clone(),
        dir: thread.dir.clone().unwrap_or_else(|| view.cwd.clone()),
        title: thread.title,
        note: String::new(),
    };
    true
}

fn menu_jump(view: &mut View, thread: crate::ui::thread_card::Card, time: u64) -> bool {
    view.menu = None;

    if thread.id == view.id {
        view.stream.jump = Some(time);
        view.stream.dirty = true;
        return false;
    }

    crate::domain::search::jump_to(&thread.id, time);
    view.exit = Exit::Open {
        id: thread.id.clone(),
        dir: thread.dir.clone().unwrap_or_else(|| view.cwd.clone()),
        title: thread.title,
        note: String::new(),
    };
    true
}

fn menu_pull_request(view: &mut View, entry: crate::screens::merge::pr_list::Entry) -> bool {
    view.menu = None;

    match entry.thread {
        Some(thread) if thread.id == view.id => view.show_pr(&entry.item.pr),
        Some(thread) => {
            open_pr(&thread.id, &entry.item.pr);
            view.exit = Exit::Open {
                id: thread.id.clone(),
                dir: thread.dir.clone(),
                title: thread.title.clone(),
                note: String::new(),
            };
            return true;
        }
        None => {
            view.exit = Exit::Home(crate::screens::quick_menu::Home::Merge(Some(entry.item.pr.clone())));
            return true;
        }
    }

    false
}

fn menu_agent_row(view: &mut View, id: usize) -> bool {
    view.menu = None;
    let (i, row) = ((id - AGENT_ITEMS) / 3, (id - AGENT_ITEMS) % 3);

    let Some(agent) = view
        .agents
        .menu_agents
        .get(i)
        .cloned()
        .filter(|agent| view.agents.members.contains(agent))
    else {
        return false;
    };

    if row == 0 {
        view.open_flow(&agent, false);
    } else {
        view.focus_agents(Some(&agent));

        set_agent_menu_focus(view, row);
    }

    false
}

fn note_jump(view: &mut View, note: &crate::domain::notifications::Note) -> bool {
    if !note.comment.is_empty() {
        crate::domain::changes::threads::jump_to(&note.thread, &note.comment);
    }

    view.menu_action(crate::screens::quick_menu::Action::Jump(note.card(), note.ts))
}
fn set_agent_menu_focus(view: &mut View, row: usize) {
    if let Some(focus) = view.agents.agents_focus.as_mut() {
        focus.cur = row;

        if row == 1 {
            focus.armed = true;
        } else {
            focus.replace = Some((0, 0));
        }
    }
}
