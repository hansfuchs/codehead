use super::{Exit, KeyModifiers, MouseEvent, Pane, Position, View, open_url};
use std::ops::ControlFlow;
impl View {
    pub(super) fn mouse_agents(&mut self, event: MouseEvent, at: Position, click: bool) -> bool {
        if click
            && event.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::SUPER)
            && let Some(url) = crate::ui::urls::at(&self.selection.last_frame, at)
        {
            crate::infra::browser::open_url(&url);
            self.toast(format!("open {url}"));
            return true;
        }

        // the worktree's URL (portless): the browser
        if click
            && let Some((result, url)) = &self.urls_state.url_rect
            && result.contains(at)
        {
            let url = url.clone();

            open_url(&url);
            self.toast(format!("open {url}"));
            return true;
        }

        // the flow line of an agent card: its flow box opens (open: closes)
        let flow_at = self
            .agents
            .flow_slots
            .borrow()
            .iter()
            .find(|(_, result)| result.contains(at))
            .map(|(agent, _)| agent.clone());

        if click && let Some(agent) = flow_at {
            match self
                .flowbars
                .iter_mut()
                .find(|bytes| bytes.agent == agent && bytes.is_open())
            {
                Some(bytes) => bytes.close(),
                None => self.open_flow(&agent, false),
            }

            self.stream.dirty = true;
            return true;
        }

        false
    }

    pub(super) fn mouse_navigation(&mut self, at: Position, click: bool) -> bool {
        if click && self.layout.home_rect.contains(at) {
            self.exit = Exit::Back;
            self.mouse_exit = true;
            return true;
        }

        // the view bar
        if click && let Some(&(_, path)) = self.layout.bar_rects.iter().find(|(result, _)| result.contains(at)) {
            self.set_pane(path);
            return true;
        }

        // a repo tab of the changes view
        if click && self.panes.pane == Pane::Changes && self.checkout_state.checkout_tabs.click(at) {
            self.pick_checkout(self.checkout_state.checkout_tabs.i);
            self.checkout_state.repo_focus = self.checkout_state.checkouts.len() > 1;
            self.panes.prompt_focus = false;
            return true;
        }

        false
    }

    pub(super) fn mouse_act(&mut self, event: MouseEvent, at: Position, click: bool) -> ControlFlow<()> {
        if self.layout.view_rect.contains(at) {
            self.panes.prompt_focus = self.panes.prompt_focus && !click;
            self.checkout_state.repo_focus = self.checkout_state.repo_focus && !click;

            mouse_view(self, event);

            return ControlFlow::Break(());
        }

        if click && self.mini_chat_state.mini_rect.contains(at) {
            self.mini_chat_state.mini_chat = !self.mini_chat_state.mini_chat;
            self.stream.dirty = true;
            return ControlFlow::Break(());
        }

        // the chat's title row in the changes view: folds / opens it

        // the chat of the split: as the chat view below (scroll, links, selection + copy)
        if click && self.layout.input_rect.contains(at) {
            self.panes.prompt_focus = true;
        }

        ControlFlow::Continue(())
    }
}

fn mouse_view(view: &mut View, event: MouseEvent) {
    if view.panes.pane == Pane::Files {
        view.panes.files.mouse(event);
    } else if view.panes.pane == Pane::Merge {
        let act = view.merge_state.merge.mouse(event);

        view.merge_act(act);
    } else {
        let act = view.panes.changes.mouse(event);

        view.changes_act(act);
    }
}
