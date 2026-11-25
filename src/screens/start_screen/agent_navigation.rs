use super::{App, Field, Pasted, Screen, agents, tilde};

impl App {
    /// Notes filter input → FILTER (list rebuilt, cursor to the top).
    pub(super) fn sync_filter(&mut self) {
        let query = self.note_list.filter_ta.text();

        if self.note_list.filter.as_deref() != Some(query.as_str()) {
            self.note_list.filter = Some(query);
            self.note_list.cursor = 0;
            self.rebuild();
        }
    }

    /// Threads filter input → TFILTER (cursor to the top).
    pub(super) fn sync_afilter(&mut self) {
        let query = self.agent_list.afilter_ta.text();

        if self.agent_list.afilter.as_deref() != Some(query.as_str()) {
            sync_afilter_body(self, query);
        }
    }

    /// Agents by harness (groups and agents A→Z). Only the agents tab filters.
    pub(super) fn avisible(&self) -> Vec<(String, Vec<usize>)> {
        let query = if self.compose.screen == Screen::Compose {
            ""
        } else {
            self.agent_list.afilter.as_deref().unwrap_or("")
        }
        .trim()
        .to_lowercase();

        crate::ui::cards::groups(&self.agent_list.agents, |agent| {
            crate::ui::cards::matches(agent, &query)
        })
    }

    /// Agents tab: the card in direction (DX, DY) of the cursor card. ↑ from the top row: None.
    pub(super) fn agent_step(&self, dx: isize, dy: isize) -> Option<usize> {
        Self::grid_step(&self.agent_list.aplace, self.agent_list.acursor, dx, dy)
    }

    pub(super) fn grid_step(places: &[(usize, u16, usize)], cursor: usize, dx: isize, dy: isize) -> Option<usize> {
        let at = places.iter().position(|&(i, _, _)| i == cursor)?;
        let (_, x, y) = places[at];

        if dx != 0 {
            let to = at as isize + dx;

            return (0..places.len() as isize).contains(&to).then(|| places[to as usize].0);
        }

        let rows = places
            .iter()
            .map(|&(_, _, result)| result)
            .filter(|&result_value| if dy < 0 { result_value < y } else { result_value > y });
        let row = if dy < 0 { rows.max()? } else { rows.min()? };

        places
            .iter()
            .filter(|&&(_, _, row_value)| row_value == row)
            .min_by_key(|&&(_, cx, _)| cx.abs_diff(x))
            .map(|&(i, _, _)| i)
    }

    /// The prompt has the keys: the @ dropdown can be up.
    pub(super) fn mention_on(&self) -> bool {
        self.overlays.modal.is_none()
            && self.compose.screen == Screen::Compose
            && !self.tabs_focus
            && self.compose.field == Field::Prompt
    }

    /// Agents for the @ dropdown: name + color.
    pub(super) fn agent_names(&self) -> Vec<(String, ratatui::style::Color)> {
        self.agent_list
            .agents
            .iter()
            .map(|agent| (agent.name.clone(), agents::color(agent.color.as_deref())))
            .collect()
    }

    /// A paste for the prompt: text in, a file → its marker in, the file listed in the message.
    pub(super) fn paste_prompt(&mut self, path_value: Pasted) {
        self.compose.field = Field::Prompt;

        match path_value {
            Pasted::Text(thread) => self.compose.prompt.paste(&thread),
            Pasted::File { path, marker } => {
                self.compose.prompt.insert(&marker.clone());
                self.toast(format!("{marker}: {}", tilde(&path.display().to_string())), false);
                self.compose.files.push((marker, path));
            }
            Pasted::Nothing => self.toast("clipboard: 0 image, 0 text", true),
        }
    }
}

fn sync_afilter_body(app_state: &mut App, query: String) {
    app_state.agent_list.afilter = Some(query);

    if let Some(&first) = app_state.avisible().first().and_then(|(_, group)| group.first()) {
        app_state.agent_list.acursor = first;
    }

    app_state.agent_list.aoffset = 0;
}
