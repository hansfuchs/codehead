use super::{KeyCode, KeyEvent, KeyModifiers, View};

impl View {
    pub(super) fn key_global(&mut self, key: KeyEvent) -> Option<bool> {
        if key.code == KeyCode::Char('m') && key.modifiers == KeyModifiers::CONTROL {
            self.focus_prompt();
            return Some(false);
        }

        // notifications (ctrl+n): over everything, the menu too
        if let Some(path) = self.notes_panel.as_mut() {
            let pick = path.key(key);

            return Some(self.notes_pick(pick));
        }

        if crate::features::notifications::is_key(&key) {
            self.menu = None;
            self.notes_panel = Some(crate::features::notifications::Panel::open(Some(&self.id)));
            self.stream.dirty = true;
            return Some(false);
        }

        if let Some(message) = self.menu.as_mut() {
            let agent = message.key(key);

            return Some(self.menu_action(agent));
        }

        // the menu key (ctrl+p): the menu from every field, box and dropdown
        if crate::infra::config::menu_key().matches(&key) {
            self.open_menu();
            return Some(false);
        }

        // the context menu (ctrl+.): its keys first
        if let Some(message) = self.ctx.as_mut() {
            let out = message.key(key);

            return Some(self.ctx_out(&(out)));
        }

        if crate::ui::context_menu::is_key(&key) {
            self.open_ctx();
            return Some(false);
        }

        None
    }
    pub(super) fn key_focus(&mut self, key: KeyEvent) -> Option<bool> {
        if matches!(key.code, KeyCode::Tab | KeyCode::BackTab)
            && !key.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
            && self.ide.ide_pick.is_none()
            && !self.tab_is_local()
        {
            return Some(key_focus_body(self, key));
        }

        if self.agents.agents_focus.is_some() {
            self.agents_key(key);
            self.stream.dirty = true;
            return Some(false);
        }

        // the IDE's repo picker: its keys first
        if self.ide.ide_pick.is_some() {
            self.ide_pick_key(key);
            self.stream.dirty = true;
            return Some(false);
        }

        None
    }
}

fn key_focus_body(view: &mut View, key: KeyEvent) -> bool {
    view.cycle(if key.code == KeyCode::Tab { 1 } else { -1 });
    false
}
