use super::{AGENT_MENU, Frame, KeyCode, KeyEvent, KeyModifiers, Rect, View, json};

impl View {
    /// Agents of the roster that can replace AGENT: 0 on the thread.
    pub(super) fn replace_candidates(&self) -> Vec<crate::domain::agents::Agent> {
        self.agents
            .roster
            .iter()
            .filter(|agent| !self.agents.members.contains(&agent.name))
            .cloned()
            .collect()
    }

    /// A key while the agents box has the keys.
    pub(super) fn agents_key(&mut self, key: KeyEvent) {
        let Some(mut fo) = self.agents.agents_focus.take() else {
            return;
        };

        let count = self.agents.order.len();

        if count == 0 {
            return;
        }

        fo.at = fo.at.min(count - 1);
        let agent = self.agents.order[fo.at].clone();

        // the replace picker: the agents to take over
        if let Some((cur, off)) = fo.replace {
            agents_key_cands(self, key, fo, agent, cur, off);
            return;
        }

        if key.code != KeyCode::Enter {
            fo.armed = false;
        }

        match key.code {
            // esc: back to the prompt
            KeyCode::Esc => return,
            KeyCode::Left | KeyCode::Char('h') => {
                fo.at = (fo.at + count - 1) % count;
                fo.cur = 0;
            }
            KeyCode::Right | KeyCode::Char('l') => {
                fo.at = (fo.at + 1) % count;
                fo.cur = 0;
            }
            KeyCode::Up | KeyCode::Char('k') => fo.cur = fo.cur.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') => fo.cur = (fo.cur + 1).min(AGENT_MENU.len() - 1),
            KeyCode::Char(character @ '1'..='3') => {
                fo.cur = usize::from(character as u8 - b'1');
                self.agents.agents_focus = Some(fo);
                return self.agents_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
            }
            KeyCode::Enter => match fo.cur {
                0 => {
                    self.open_flow(&agent, false);
                    return;
                }
                1 if self.agents.members.len() < 2 => self.toast("the thread's last agent: replace it instead"),
                1 if fo.armed => {
                    self.host("leave", json!({"op": "leave", "agent": agent}));
                    self.toast(format!("{agent} leaves the thread"));
                    return;
                }
                1 => fo.armed = true,
                _ if self.replace_candidates().is_empty() => self.toast("0 other agents · menu → new agent"),
                _ => fo.replace = Some((0, 0)),
            },
            _ => {}
        }

        self.agents.agents_focus = Some(fo);
    }

    /// NEW takes over from OLD: OLD leaves, NEW gets the handoff (and OLD's flow). OLD mid-turn:
    /// after its turn.
    pub(super) fn replace_agent(&mut self, new: String, old: String) {
        let text = format!("@{new}");

        if self.thread.busy.contains(&old) {
            self.toast(format!(
                "{new} replaces {old} after {old}'s turn · esc: interrupt {old}"
            ));
            self.prompt.queued = Some((new, old, text));
        } else {
            self.run_replace(new, old, &(text));
        }
    }

    /// The agents box's menu above the highlighted agent (or its replace picker).
    pub(super) fn draw_agents_menu(&mut self, frame: &mut Frame, bounds: Rect) {
        let Some(fo) = self.agents.agents_focus.clone() else {
            return;
        };

        let Some(agent) = self.agents.order.get(fo.at).cloned() else {
            return;
        };

        // the agent's card (0 drawn: a narrow or short screen): else over the prompt
        let slot = self
            .agents
            .slots
            .borrow()
            .iter()
            .find(|(agent_value, _)| *agent_value == agent)
            .map(|(_, result)| *result);
        let anchor = slot.unwrap_or(Rect {
            y: self.layout.input_rect.y.saturating_sub(1),
            height: 1,
            width: 1,
            ..self.layout.input_rect
        });

        if let Some(cursor) = fo.replace {
            draw_agents_menu_rows(self, frame, bounds, &agent, anchor, cursor);
            return;
        }

        let details = agent_menu_details(self, &agent, &fo);
        let rows: Vec<crate::ui::dropdown::Row> = AGENT_MENU
            .iter()
            .zip(details)
            .map(|(thread, directory)| crate::ui::dropdown::Row {
                text: (*thread).into(),
                detail: directory,
                now: false,
                marks: Vec::new(),
            })
            .collect();

        let Some(row) = crate::ui::dropdown::place(anchor, 50, rows.len(), bounds, true) else {
            return;
        };

        crate::ui::dropdown::list(
            frame,
            row,
            crate::ui::dropdown::List {
                label: &format!("@{agent}"),
                note: "←→ agent · ↑↓ · enter · esc",
                rows: &rows,
                cursor: fo.cur,
                offset: 0,
                empty: "",
            },
        );
    }
}

fn agent_flow_label(state: &crate::engine::flows::run::FlowState) -> String {
    if state.finished {
        format!("{} · finished", state.title)
    } else {
        format!("{} · step {}", state.title, state.node + 1)
    }
}

fn agents_key_cands(view: &mut View, key: KeyEvent, mut fo: super::AgentsFocus, agent: String, cur: usize, off: usize) {
    let cands = view.replace_candidates();

    match key.code {
        KeyCode::Esc | KeyCode::Left => fo.replace = None,
        KeyCode::Up | KeyCode::Char('k') => fo.replace = Some((cur.saturating_sub(1), off)),
        KeyCode::Down | KeyCode::Char('j') => {
            fo.replace = Some(((cur + 1).min(cands.len().saturating_sub(1)), off));
        }
        KeyCode::Enter => {
            if let Some(new) = cands.get(cur) {
                view.replace_agent(new.name.clone(), agent);
                return;
            }
        }
        _ => {}
    }

    view.agents.agents_focus = Some(fo);
}

fn agent_menu_details(view: &View, agent: &str, focus: &super::AgentsFocus) -> [String; 3] {
    let flow = view
        .flowbars
        .iter()
        .find(|bytes| bytes.agent == agent)
        .and_then(|bytes| bytes.state.as_ref())
        .map(agent_flow_label);

    [
        flow.unwrap_or_else(|| "0 flow: pick one".into()),
        if focus.armed {
            format!("enter again: remove @{agent}")
        } else {
            "leaves the thread".into()
        },
        "a new agent, the flow goes along".into(),
    ]
}

fn draw_agents_menu_rows(
    view: &mut View,
    frame: &mut Frame<'_>,
    bounds: Rect,
    agent: &str,
    anchor: Rect,
    (cur, off): (usize, usize),
) {
    let rows: Vec<crate::ui::dropdown::Row> = view
        .replace_candidates()
        .iter()
        .map(|agent_value| crate::ui::dropdown::Row {
            text: agent_value.name.clone(),
            detail: crate::ui::cards::tech(agent_value),
            now: false,
            marks: Vec::new(),
        })
        .collect();

    let Some(row) = crate::ui::dropdown::place(anchor, 44, rows.len(), bounds, true) else {
        return;
    };

    let (off, _) = crate::ui::dropdown::list(
        frame,
        row,
        crate::ui::dropdown::List {
            label: &format!("replace @{agent} with"),
            note: "↑↓ · enter · esc: back",
            rows: &rows,
            cursor: cur,
            offset: off,
            empty: "0 other agents",
        },
    );

    if let Some(fo) = view.agents.agents_focus.as_mut() {
        fo.replace = Some((cur, off));
    }
}
