use super::flow_preview::{FlowMenuDraw, FlowPreview};
use super::{App, FlowMenu, Frame, KeyCode, KeyEvent, Position, Rect, UnicodeWidthStr, dropdown, flow_default_get};

impl App {
    /// The repo (else the dir) the flow default belongs to.
    pub(super) fn flow_key(&self) -> String {
        self.repository.repo.as_ref().map_or_else(
            || self.cfg.cwd.display().to_string(),
            |result| result.path.display().to_string(),
        )
    }

    /// The flows of the task's repo read again; the lead's default: the repo's last flow. A pick
    /// of a flow the repo lacks: 0 flow.
    pub(crate) fn load_task_flows(&mut self) {
        let dir = self
            .repository
            .repo
            .as_ref()
            .map_or_else(|| self.cfg.cwd.clone(), |result| result.path.clone());

        self.flow_selection.flow_lib = crate::engine::flows::Library::load(Some(&dir));
        let lib = &self.flow_selection.flow_lib;

        self.flow_selection.flow_default =
            flow_default_get(&self.flow_key()).filter(|text| lib.flows.iter().any(|file| file.slug == *text));

        self.flow_selection.agent_flows.retain(|_, (text_value, _)| {
            text_value.is_empty() || lib.flows.iter().any(|file_value| file_value.slug == *text_value)
        });

        self.flow_selection.flow_menu = None;
    }

    /// The new thread's lead: the 1st picked agent in the cards' order.
    pub(super) fn task_lead(&self) -> Option<String> {
        self.agent_list
            .agents
            .iter()
            .find(|agent| self.compose.picks.contains(&agent.name))
            .map(|agent_value| agent_value.name.clone())
    }

    /// AGENT's flow for the new thread: (slug, start step); "" = 0 flow. The lead with 0 pick: the
    /// repo's last flow.
    pub(super) fn flow_of(&self, agent: &str) -> (String, usize) {
        if let Some(file) = self.flow_selection.agent_flows.get(agent) {
            return file.clone();
        }

        match &self.flow_selection.flow_default {
            Some(directory) if self.task_lead().as_deref() == Some(agent) => (directory.clone(), 0),
            _ => (String::new(), 0),
        }
    }

    /// AGENT's flow as the dropdown's row (0: 0 flow, I: the I-th flow) and its start step.
    pub(super) fn flow_at(&self, agent: &str) -> (usize, usize) {
        let (slug, start) = self.flow_of(agent);

        match self
            .flow_selection
            .flow_lib
            .flows
            .iter()
            .position(|file| file.slug == slug)
        {
            Some(i) => (i + 1, start),
            None => (0, 0),
        }
    }

    /// The resolved steps of flow I (1-based; 0: 0 flow → 0 steps).
    pub(super) fn flow_steps(&self, i: usize) -> Vec<crate::engine::flows::Resolved> {
        i.checked_sub(1)
            .and_then(|i| self.flow_selection.flow_lib.flows.get(i))
            .map(|file| crate::engine::flows::resolve(file, &self.flow_selection.flow_lib))
            .unwrap_or_default()
    }

    /// The flow field for the setup tutorial's coach: (the flow dropdown open, the picked flow's
    /// slug; None: 0 flow).
    pub(crate) fn flow_peek(&self) -> (bool, Option<String>) {
        let lead = self.task_lead().map(|line| self.flow_of(&line).0);
        let slug = lead
            .into_iter()
            .chain(self.flow_selection.agent_flows.values().map(|file| file.0.clone()))
            .find(|text| !text.is_empty());

        (self.flow_selection.flow_menu.is_some(), slug)
    }

    /// The flow dropdown of the card under the cursor (`f`), on its flow (steps closed).
    pub(super) fn open_flow_menu(&mut self) {
        let Some(agent) = self
            .agent_list
            .agents
            .get(self.compose.pick_cursor)
            .map(|agent_value| agent_value.name.clone())
        else {
            return;
        };

        let (cursor, _) = self.flow_at(&agent);

        self.flow_selection.flow_menu = Some(FlowMenu {
            agent,
            cursor,
            offset: 0,
            sub: None,
        });
    }

    /// Flow I picked for the dropdown's agent, from step START: the dropdown closes. A flow for an
    /// unpicked agent picks it.
    pub(super) fn pick_flow(&mut self, i: usize, start: usize) {
        let Some(agent) = self.flow_selection.flow_menu.take().map(|message| message.agent) else {
            return;
        };

        let slug = i
            .checked_sub(1)
            .and_then(|i| self.flow_selection.flow_lib.flows.get(i))
            .map(|file| file.slug.clone())
            .unwrap_or_default();

        let start = if slug.is_empty() {
            0
        } else {
            start.min(self.flow_steps(i).len().saturating_sub(1))
        };

        if !slug.is_empty() && !self.compose.picks.contains(&agent) {
            self.compose.picks.push(agent.clone());
        }

        self.flow_selection.agent_flows.insert(agent, (slug, start));
    }

    /// Keys of the open flow dropdown: ↑↓ move, enter / → the flow's steps (0 flow: picked),
    /// a letter: the next flow with it, esc / ← close. In the steps: ↑↓, enter picks, esc / ← back.
    pub(super) fn flow_menu_key(&mut self, key: KeyEvent) {
        let Some(message) = self.flow_selection.flow_menu.as_ref() else {
            return;
        };

        let (cur, sub) = (message.cursor, message.sub);
        let agent = message.agent.clone();
        let (fi, fs) = self.flow_at(&agent);
        let count = self.flow_selection.flow_lib.flows.len() + 1;

        if flow_step_key(self, key, cur, sub) {
            return;
        }

        let set = |app_value: &mut App, character: usize| {
            set_flow_cursor(app_value, character);
        };

        match key.code {
            KeyCode::Esc | KeyCode::Left | KeyCode::Char('h') => self.flow_selection.flow_menu = None,
            KeyCode::Up | KeyCode::Char('k') => set(self, (cur + count - 1) % count),
            KeyCode::Down | KeyCode::Char('j') => set(self, (cur + 1) % count),
            KeyCode::Enter | KeyCode::Right | KeyCode::Char(' ' | 'l') => {
                flow_menu_key_message_(self, cur, fi, fs);
            }
            KeyCode::Char('0') => self.pick_flow(0, 0),
            KeyCode::Char(character_value) if character_value.is_alphanumeric() => {
                flow_menu_key_character_(self, cur, set, character_value);
            }
            _ => {}
        }
    }

    /// A click with the flow dropdown open: a flow row opens its steps (0 flow: picked), a step
    /// row picks it, a click next to them closes the dropdown.
    pub(super) fn flow_menu_click(&mut self, at: Position) {
        if let Some(&(_, text)) = self
            .preview
            .regions
            .flow_steps
            .iter()
            .find(|(result, _)| result.contains(at))
        {
            let cur = self
                .flow_selection
                .flow_menu
                .as_ref()
                .map_or(0, |message| message.cursor);

            return self.pick_flow(cur, text);
        }

        let Some(&(_, i)) = self
            .preview
            .regions
            .flow_rows
            .iter()
            .find(|(result_value, _)| result_value.contains(at))
        else {
            self.flow_selection.flow_menu = None;
            return;
        };

        if i == 0 {
            return self.pick_flow(0, 0);
        }

        let agent = self
            .flow_selection
            .flow_menu
            .as_ref()
            .map(|message_value| message_value.agent.clone())
            .unwrap_or_default();
        let (fi, fs) = self.flow_at(&agent);
        let start = if i == fi { fs } else { 0 };

        self.flow_selection.flow_menu = Some(FlowMenu {
            agent,
            cursor: i,
            offset: self
                .flow_selection
                .flow_menu
                .as_ref()
                .map_or(0, |message_2| message_2.offset),
            sub: Some((start, 0)),
        });
    }

    /// The open flow dropdown at its agent's card, the steps dropdown of the flow under the cursor
    /// beside it.
    pub(super) fn draw_flow_menu(&mut self, frame: &mut Frame, bounds: Rect) {
        let Some((agent, cur, off, sub)) = self
            .flow_selection
            .flow_menu
            .as_ref()
            .map(|message| (message.agent.clone(), message.cursor, message.offset, message.sub))
        else {
            return;
        };

        let (fi, fs) = self.flow_at(&agent);
        let items = flow_menu_items(self, fi);

        let count_value = items.len();
        let width = items
            .iter()
            .map(|result| result.text.width() + result.detail.width() + 8)
            .max()
            .unwrap_or(0)
            .clamp(30, 60) as u16;
        let anchor = flow_menu_anchor(self, &agent);

        let Some(result_2) = dropdown::place(anchor, width, count_value, bounds, false) else {
            return;
        };

        let note = if sub.is_some() {
            "← back".to_string()
        } else {
            "→ start step".to_string()
        };

        // the parent: gray while the steps have the keys
        let (off, drawn) = dropdown::list_in(
            frame,
            result_2,
            dropdown::List {
                label: &format!("flow of {agent}"),
                note: &note,
                rows: &items,
                cursor: cur,
                offset: off,
                empty: "0 flows",
            },
            sub.is_none(),
        );

        self.preview.regions.flow_menu = result_2;
        self.preview.regions.flow_rows.clone_from(&drawn);

        if let Some(message_value) = self.flow_selection.flow_menu.as_mut() {
            message_value.offset = off;
        }

        // the flow under the cursor: its rail beside the list at once (the flows tab's look);
        // enter / →: the rail takes the keys (the start step)
        self.preview.regions.flow_steps.clear();

        draw_flow_steps(
            self,
            frame,
            &(FlowMenuDraw {
                bounds,
                list_area: result_2,
                list_rows: drawn,
                cursor: cur,
                selected_flow: fi,
                selected_step: fs,
                steps: sub,
            }),
        );
    }
}

fn set_flow_cursor(app_value: &mut App, character: usize) {
    if let Some(message_3) = app_value.flow_selection.flow_menu.as_mut() {
        message_3.cursor = character;
    }
}

fn open_flow_steps(cur: usize, fi: usize, fs: usize, message_4: &mut FlowMenu) {
    message_4.sub = Some((if cur == fi { fs } else { 0 }, 0));
}

fn flow_step_key(app_state: &mut App, key: KeyEvent, cur: usize, sub: Option<(usize, usize)>) -> bool {
    if let Some((text, _)) = sub {
        flow_step_key_steps(app_state, key, cur, text);
        return true;
    }

    false
}

fn flow_step_key_steps(app_state: &mut App, key: KeyEvent, cur: usize, text: usize) {
    let steps = app_state.flow_steps(cur).len().max(1);

    let set = |app: &mut App, text_value: usize| {
        if let Some(message_value) = app.flow_selection.flow_menu.as_mut() {
            message_value.sub = Some((text_value, message_value.sub.map_or(0, |x| x.1)));
        }
    };

    match key.code {
        KeyCode::Esc | KeyCode::Left | KeyCode::Char('h') => {
            if let Some(message_2) = app_state.flow_selection.flow_menu.as_mut() {
                message_2.sub = None;
            }
        }
        KeyCode::Up | KeyCode::Char('k') => set(app_state, (text + steps - 1) % steps),
        KeyCode::Down | KeyCode::Char('j') => set(app_state, (text + 1) % steps),
        KeyCode::Enter | KeyCode::Char(' ') => app_state.pick_flow(cur, text),
        _ => {}
    }
}

fn flow_menu_items(app_state: &mut App, fi: usize) -> Vec<dropdown::Row> {
    let mut items = vec![dropdown::Row {
        text: "0 flow".into(),
        detail: "free-form".into(),
        now: fi == 0,
        marks: Vec::new(),
    }];

    for (i, fl) in app_state.flow_selection.flow_lib.flows.iter().enumerate() {
        let count = crate::engine::flows::resolve(fl, &app_state.flow_selection.flow_lib).len();

        items.push(dropdown::Row {
            text: format!("◆ {}", fl.name),
            detail: format!("{count} steps ›"),
            now: fi == i + 1,
            marks: Vec::new(),
        });
    }

    items
}

fn flow_menu_anchor(app_state: &mut App, agent: &String) -> Rect {
    app_state
        .preview
        .regions
        .menu_rows
        .iter()
        .find(|(_, i)| {
            app_state
                .agent_list
                .agents
                .get(*i)
                .is_some_and(|agent_value| agent_value.name == *agent)
        })
        .map_or(Rect::default(), |(result_value, _)| *result_value)
}

fn draw_flow_steps(app_state: &mut App, frame: &mut Frame<'_>, menu: &FlowMenuDraw) {
    let Some(flow) = menu
        .cursor
        .checked_sub(1)
        .and_then(|index| app_state.flow_selection.flow_lib.flows.get(index))
        .cloned()
    else {
        return;
    };

    let preview = FlowPreview::new(flow, &app_state.flow_selection.flow_lib);

    let Some(area) = preview.place(menu) else {
        return;
    };

    let (rows, offset) = preview.draw(frame, area, menu);

    app_state.preview.regions.flow_steps = rows;

    if let Some(flow_menu) = app_state.flow_selection.flow_menu.as_mut()
        && let Some((step, _)) = flow_menu.sub
    {
        flow_menu.sub = Some((step, offset));
    }
}

fn flow_menu_key_message_(app_state: &mut App, cur: usize, fi: usize, fs: usize) {
    if cur == 0 {
        app_state.pick_flow(0, 0);
    } else if let Some(message_4) = app_state.flow_selection.flow_menu.as_mut() {
        // the steps: on the picked start when it is this flow
        open_flow_steps(cur, fi, fs, message_4);
    }
}

fn flow_menu_key_character_(app_state: &mut App, cur: usize, set: impl Fn(&mut App, usize), character_value: char) {
    let character_2 = character_value.to_ascii_lowercase();

    let hits: Vec<usize> = (0..app_state.flow_selection.flow_lib.flows.len())
        .filter(|&i| {
            app_state.flow_selection.flow_lib.flows[i]
                .name
                .to_lowercase()
                .starts_with(character_2)
        })
        .map(|i| i + 1)
        .collect();

    if let Some(&next) = hits.iter().find(|&&i| i > cur).or(hits.first()) {
        set(app_state, next);
    }
}
