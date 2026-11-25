use super::{
    AgentForm, F_BUTTONS, F_COLOR, F_EFFORT, F_HARNESS, F_MCP, F_MODE, F_MODEL, F_SET, F_SPRITE, F_STYLE, FORM_FIELDS,
    Input, KeyCode, KeyEvent, KeyModifiers, agents,
};

pub(super) enum FormAction {
    Keep,
    Close,
    Discard,
    Save,
    Models(bool),
}

impl AgentForm {
    pub(super) fn key(&mut self, key: KeyEvent) -> FormAction {
        let control = key.modifiers.contains(KeyModifiers::CONTROL);

        match key.code {
            KeyCode::Esc if !self.dirty() => return FormAction::Close,
            KeyCode::Esc => return FormAction::Discard,
            KeyCode::Char('s') | KeyCode::Enter if control => return FormAction::Save,
            KeyCode::Enter if matches!(self.field, F_SET | F_SPRITE) => {
                self.picker = Some(crate::features::agents::avatar_picker::Picker::new(&self.sprite));
                return FormAction::Keep;
            }
            KeyCode::Enter if self.field == F_BUTTONS => {
                return button_action(self.save);
            }
            KeyCode::Enter if matches!(self.field, F_HARNESS | F_MODEL) => {
                return FormAction::Models(self.field == F_MODEL);
            }
            _ => {}
        }

        if self.navigation_key(key) || self.choice_key(key) {
            return FormAction::Keep;
        }

        self.error = None;

        if let Some(input) = self.text() {
            let _: Input = input.key(key);
        }

        FormAction::Keep
    }

    fn navigation_key(&mut self, key: KeyEvent) -> bool {
        let direction = match key.code {
            KeyCode::Down | KeyCode::Enter if !self.multiline() => 1,
            KeyCode::Up if !self.multiline() => -1,
            KeyCode::Down if key.modifiers.is_empty() && self.text().is_some_and(|input| input.on_last_row()) => 1,
            KeyCode::Up if key.modifiers.is_empty() && self.text().is_some_and(|input| input.on_first_row()) => -1,
            _ => return false,
        };

        self.field = cycle(self.field, direction, FORM_FIELDS);
        true
    }

    fn choice_key(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Char('r')
                if !key.modifiers.contains(KeyModifiers::CONTROL) && matches!(self.field, F_SET | F_SPRITE) =>
            {
                self.random_sprite();
            }
            KeyCode::Left | KeyCode::Right if self.field == F_MCP => {
                self.choice_key_direction(key);
            }
            KeyCode::Char(' ') if self.field == F_MCP => self.toggle_server(),
            KeyCode::Left | KeyCode::Right
                if matches!(self.field, F_MODEL | F_EFFORT | F_COLOR | F_MODE | F_STYLE | F_BUTTONS) =>
            {
                self.choice_key_body(key);
            }
            _ => return false,
        }

        true
    }

    fn choice_key_body(&mut self, key: KeyEvent) {
        self.cycle_choice(if key.code == KeyCode::Left { -1 } else { 1 });
    }

    fn choice_key_direction(&mut self, key: KeyEvent) {
        let direction = if key.code == KeyCode::Left { -1 } else { 1 };

        self.mcp_cur = cycle(self.mcp_cur, direction, self.mcp.len().max(1));
    }

    fn toggle_server(&mut self) {
        if let Some(server) = self.mcp.get_mut(self.mcp_cur) {
            server.1 = !server.1;
        }
    }

    fn cycle_choice(&mut self, direction: isize) {
        match self.field {
            F_MODEL if self.is_claude() => self.model = cycle(self.model, direction, self.models.len()),
            F_MODEL => {}
            F_EFFORT => self.effort = cycle(self.effort, direction, agents::EFFORTS.len()),
            F_COLOR => self.color = cycle(self.color, direction, agents::COLORS.len()),
            F_MODE => self.mode = cycle(self.mode, direction, agents::MODES.len()),
            F_STYLE => self.style = cycle(self.style, direction, self.styles.len()),
            _ => self.save = !self.save,
        }
    }
}

fn cycle(index: usize, direction: isize, count: usize) -> usize {
    (index as isize + direction).rem_euclid(count as isize) as usize
}

fn button_action(save: bool) -> FormAction {
    if save { FormAction::Save } else { FormAction::Close }
}
