use super::{
    App, KeyModifiers, Modal, MouseButton, MouseEvent, MouseEventKind, Position, Screen, Tab, mouse_agent_form,
    mouse_compose, mouse_lists, mouse_thread_list,
};

impl App {
    pub(crate) fn mouse(&mut self, event: MouseEvent) {
        if mouse_pick(self, event) {
            return;
        }

        self.mouse_inner(event);

        if !matches!(event.kind, MouseEventKind::Moved) {
            self.backup();
        }
    }

    pub(super) fn mouse_inner(&mut self, event: MouseEvent) {
        let at = Position {
            x: event.column,
            y: event.row,
        };

        self.preview.hover = Some(at);

        // ctrl+click on a URL anywhere on screen: the browser
        if let Some(value) = mouse_overlays(self, event, at) {
            return value;
        }

        let down = matches!(event.kind, MouseEventKind::Down(MouseButton::Left));

        if mouse_agent_form(self, event, at, down) {
            return;
        }

        if self.overlays.modal.is_some() {
            mouse_inner_branch(self, at, down);
            return;
        }

        // the threads tab's list (filter, repo + view tabs, cards, actions, form): its own clicks;
        // this screen's rows (worktrees, claude views): a click picks, a 2nd opens
        if mouse_navigation(self, event, at, down) {
            return;
        }

        if mouse_compose(self, event, at, down) {
            return;
        }

        let list = self.preview.regions.list;
        let in_list = list.contains(at);
        let in_preview = self.preview.regions.preview.contains(at);

        // a filter input in edit: its own mouse (cursor, selection + copy)
        mouse_lists(self, event, at, list, in_list, in_preview);
    }
}

pub(super) fn mouse_navigation(app_state: &mut App, event: MouseEvent, at: Position, down: bool) -> bool {
    if mouse_thread_list(app_state, event, at, down) {
        return true;
    }

    if down
        && let Some(&(_, tab)) = app_state
            .preview
            .regions
            .tabs
            .iter()
            .find(|(result_3, _)| result_3.contains(at))
    {
        app_state.switch_tab(tab);
        app_state.tabs_focus = false;
        return true;
    }

    if app_state.compose.screen == Screen::Pick && app_state.tab == Tab::Settings {
        app_state.tabs_focus = false;
        let action = app_state
            .settings
            .as_mut()
            .map_or(crate::features::settings::Action::None, |text| text.mouse(event));

        app_state.settings_action(action);
        return true;
    }

    false
}

pub(super) fn mouse_overlays(app_state: &mut App, event: MouseEvent, at: Position) -> Option<()> {
    if mouse_link(app_state, event, at) {
        return Some(());
    }

    if let Some(character) = app_state.overlays.ctx.as_mut() {
        if matches!(
            event.kind,
            MouseEventKind::Down(_) | MouseEventKind::ScrollUp | MouseEventKind::ScrollDown
        ) {
            let out = character.mouse(event);

            app_state.ctx_out(out);
        }

        return Some(());
    }

    if let Some(menu) = app_state.overlays.menu.as_mut() {
        let agent = menu.mouse(event);

        return {
            app_state.menu_action(agent);
            Some(())
        };
    }

    if app_state.overlays.modal.is_none()
        && app_state.compose.screen == Screen::Pick
        && app_state.tab == Tab::Projects
        && app_state.project_selection.projects_area.contains(at)
    {
        let output = app_state.project_selection.projects_tab.mouse(event);

        return {
            app_state.projects_out(output);
            Some(())
        };
    }

    if app_state.overlays.modal.is_none()
        && app_state.compose.screen == Screen::Pick
        && app_state.tab == Tab::Flows
        && app_state.flows_area.contains(at)
    {
        let output_value = app_state.flows_tab.mouse(event);

        return {
            app_state.flows_out(output_value);
            Some(())
        };
    }

    if app_state.overlays.modal.is_none()
        && app_state.compose.screen == Screen::Pick
        && app_state.tab == Tab::Merge
        && app_state.merge_area.contains(at)
    {
        let output_2 = app_state.merge_tab.mouse(event);

        return {
            app_state.merge_out(output_2);
            Some(())
        };
    }

    None
}

pub(super) fn mouse_link(app_state: &mut App, event: MouseEvent, at: Position) -> bool {
    if matches!(event.kind, MouseEventKind::Down(MouseButton::Left))
        && event.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::SUPER)
        && let Some(url) = crate::ui::urls::at(&app_state.preview.last_frame, at)
    {
        crate::infra::browser::open_url(&url);
        app_state.toast(format!("open {url}"), false);
        return true;
    }

    false
}

pub(super) fn mouse_inner_branch(app_state: &mut App, at: Position, down: bool) {
    if !down {
        return;
    }

    if let Some(&(_, yes)) = app_state
        .preview
        .regions
        .buttons
        .iter()
        .find(|(result_2, _)| result_2.contains(at))
        && matches!(app_state.overlays.modal, Some(Modal::Confirm { .. }))
        && let Some(Modal::Confirm { action, .. }) = app_state.overlays.modal.take()
    {
        mouse_inner_branch_branch(app_state, yes, action);
    }
}

pub(super) fn mouse_inner_branch_branch(app_state: &mut App, yes: bool, action: super::ConfirmAction) {
    if yes {
        app_state.confirmed(action);
    } else {
        app_state.declined(action);
    }
}

pub(super) fn mouse_pick(app_state: &mut App, event: MouseEvent) -> bool {
    let Some(panel) = app_state.overlays.notes_panel.as_mut() else {
        return false;
    };

    if matches!(event.kind, MouseEventKind::Down(_)) {
        let pick = panel.click(Position {
            x: event.column,
            y: event.row,
        });

        app_state.notes_pick(pick);
    }

    true
}
