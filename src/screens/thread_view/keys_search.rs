use super::{KeyEvent, Pane, View};

impl View {
    pub(super) fn key_search(&mut self, key: KeyEvent) -> Option<bool> {
        let filter = crate::ui::textarea::is_filter_key(&key);

        if filter && (self.panes.pane == Pane::Chat || (self.layout.wide_split && self.panes.prompt_focus)) {
            open_chat_search(self);
            return Some(false);
        }

        if filter {
            self.panes.prompt_focus = false;
        }

        if !self.find.focus {
            return None;
        }

        match self.find.key(key, &self.thread) {
            crate::features::chat::chat_search::Act::Pass => None,
            crate::features::chat::chat_search::Act::None | crate::features::chat::chat_search::Act::Close => {
                self.stream.dirty = true;
                Some(false)
            }
            crate::features::chat::chat_search::Act::Show(index) => {
                self.find_show(index);
                Some(false)
            }
        }
    }
}

fn open_chat_search(view: &mut View) {
    view.flowbars
        .iter_mut()
        .for_each(crate::features::flows::flow_bar::FlowBar::close);
    view.find.open();
    view.find_to = view.find.current();
    view.stream.dirty = true;
}
