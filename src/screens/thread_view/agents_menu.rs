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
        let Some(mut focus) = self.agents.agents_focus.take() else {
            return;
        };

        let count = self.agents.order.len();

        if count == 0 {
            return;
        }

        focus.at = focus.at.min(count - 1);
        let agent = self.agents.order[focus.at].clone();

        // the replace picker: the agents to take over
        if let Some((cur, off)) = focus.replace {
            agents_key_cands(self, key, focus, agent, cur, off);
            return;
        }

        if key.code != KeyCode::Enter {
            focus.armed = false;
        }

        match key.code {
            // esc: back to the prompt
            KeyCode::Esc => return,
            KeyCode::Left | KeyCode::Char('h') => {
                focus.at = (focus.at + count - 1) % count;
                focus.cur = 0;
            }
            KeyCode::Right | KeyCode::Char('l') => {
                focus.at = (focus.at + 1) % count;
                focus.cur = 0;
            }
            KeyCode::Up | KeyCode::Char('k') => focus.cur = focus.cur.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') => focus.cur = (focus.cur + 1).min(AGENT_MENU.len() - 1),
            KeyCode::Char(digit @ '1'..='3') => {
                focus.cur = usize::from(digit as u8 - b'1');
                self.agents.agents_focus = Some(focus);
                return self.agents_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
            }
            KeyCode::Enter => match focus.cur {
                0 => {
                    self.open_flow(&agent, false);
                    return;
                }
                1 if self.agents.members.len() < 2 => self.toast("the thread's last agent: replace it instead"),
                1 if focus.armed => {
                    self.host("leave", json!({"op": "leave", "agent": agent}));
                    self.toast(format!("{agent} leaves the thread"));
                    return;
                }
                1 => focus.armed = true,
                _ if self.replace_candidates().is_empty() => self.toast("0 other agents · menu → new agent"),
                _ => focus.replace = Some((0, 0)),
            },
            _ => {}
        }

        self.agents.agents_focus = Some(focus);
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
            self.run_replace(new, old, &text);
        }
    }

    /// The agents box's menu above the highlighted agent (or its replace picker).
    pub(super) fn draw_agents_menu(&mut self, frame: &mut Frame, bounds: Rect) {
        let Some(focus) = self.agents.agents_focus.clone() else {
            return;
        };

        let Some(agent) = self.agents.order.get(focus.at).cloned() else {
            return;
        };

        // the agent's card (0 drawn: a narrow or short screen): else over the prompt
        let slot = self
            .agents
            .slots
            .borrow()
            .iter()
            .find(|(name, _)| *name == agent)
            .map(|(_, rect)| *rect);
        let anchor = slot.unwrap_or(Rect {
            y: self.layout.input_rect.y.saturating_sub(1),
            height: 1,
            width: 1,
            ..self.layout.input_rect
        });

        if let Some(cursor) = focus.replace {
            draw_agents_menu_rows(self, frame, bounds, &agent, anchor, cursor);
            return;
        }

        let details = agent_menu_details(self, &agent, &focus);
        let rows: Vec<crate::ui::dropdown::Row> = AGENT_MENU
            .iter()
            .zip(details)
            .map(|(label, detail)| crate::ui::dropdown::Row {
                text: (*label).into(),
                detail,
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
                cursor: focus.cur,
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

fn agents_key_cands(
    view: &mut View,
    key: KeyEvent,
    mut focus: super::AgentsFocus,
    agent: String,
    cur: usize,
    off: usize,
) {
    let cands = view.replace_candidates();

    match key.code {
        KeyCode::Esc | KeyCode::Left => focus.replace = None,
        KeyCode::Up | KeyCode::Char('k') => focus.replace = Some((cur.saturating_sub(1), off)),
        KeyCode::Down | KeyCode::Char('j') => {
            focus.replace = Some(((cur + 1).min(cands.len().saturating_sub(1)), off));
        }
        KeyCode::Enter => {
            if let Some(new) = cands.get(cur) {
                view.replace_agent(new.name.clone(), agent);
                return;
            }
        }
        _ => {}
    }

    view.agents.agents_focus = Some(focus);
}

fn agent_menu_details(view: &View, agent: &str, focus: &super::AgentsFocus) -> [String; 3] {
    let flow = view
        .flowbars
        .iter()
        .find(|bar| bar.agent == agent)
        .and_then(|bar| bar.state.as_ref())
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
        .map(|candidate| crate::ui::dropdown::Row {
            text: candidate.name.clone(),
            detail: crate::ui::cards::tech(candidate),
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

    if let Some(focus) = view.agents.agents_focus.as_mut() {
        focus.replace = Some((cur, off));
    }
}
