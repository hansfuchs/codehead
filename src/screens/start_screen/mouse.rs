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
        if let Some(done) = mouse_overlays(self, event, at) {
            return done;
        }

        let down = matches!(event.kind, MouseEventKind::Down(MouseButton::Left));

        if mouse_agent_form(self, event, at, down) {
            return;
        }

        if self.overlays.modal.is_some() {
            click_modal_button(self, at, down);
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

pub(super) fn mouse_navigation(app: &mut App, event: MouseEvent, at: Position, down: bool) -> bool {
    if mouse_thread_list(app, event, at, down) {
        return true;
    }

    if down && let Some(&(_, tab)) = app.preview.regions.tabs.iter().find(|(rect, _)| rect.contains(at)) {
        app.switch_tab(tab);
        app.tabs_focus = false;
        return true;
    }

    if app.compose.screen == Screen::Pick && app.tab == Tab::Settings {
        app.tabs_focus = false;

        let action = app
            .settings
            .as_mut()
            .map_or(crate::features::settings::Action::None, |settings| {
                settings.mouse(event)
            });

        app.settings_action(action);
        return true;
    }

    false
}

pub(super) fn mouse_overlays(app: &mut App, event: MouseEvent, at: Position) -> Option<()> {
    if mouse_link(app, event, at) {
        return Some(());
    }

    if let Some(ctx) = app.overlays.ctx.as_mut() {
        if matches!(
            event.kind,
            MouseEventKind::Down(_) | MouseEventKind::ScrollUp | MouseEventKind::ScrollDown
        ) {
            let out = ctx.mouse(event);

            app.ctx_out(out);
        }

        return Some(());
    }

    if let Some(menu) = app.overlays.menu.as_mut() {
        let action = menu.mouse(event);

        return {
            app.menu_action(action);
            Some(())
        };
    }

    if app.overlays.modal.is_none()
        && app.compose.screen == Screen::Pick
        && app.tab == Tab::Projects
        && app.project_selection.projects_area.contains(at)
    {
        let output = app.project_selection.projects_tab.mouse(event);

        return {
            app.projects_out(output);
            Some(())
        };
    }

    if app.overlays.modal.is_none()
        && app.compose.screen == Screen::Pick
        && app.tab == Tab::Flows
        && app.flows_area.contains(at)
    {
        let output = app.flows_tab.mouse(event);

        return {
            app.flows_out(output);
            Some(())
        };
    }

    if app.overlays.modal.is_none()
        && app.compose.screen == Screen::Pick
        && app.tab == Tab::Merge
        && app.merge_area.contains(at)
    {
        let output = app.merge_tab.mouse(event);

        return {
            app.merge_out(output);
            Some(())
        };
    }

    None
}

pub(super) fn mouse_link(app: &mut App, event: MouseEvent, at: Position) -> bool {
    if matches!(event.kind, MouseEventKind::Down(MouseButton::Left))
        && event.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::SUPER)
        && let Some(url) = crate::ui::urls::at(&app.preview.last_frame, at)
    {
        crate::infra::browser::open_url(&url);
        app.toast(format!("open {url}"), false);
        return true;
    }

    false
}

pub(super) fn click_modal_button(app: &mut App, at: Position, down: bool) {
    if !down {
        return;
    }

    if let Some(&(_, yes)) = app.preview.regions.buttons.iter().find(|(rect, _)| rect.contains(at))
        && matches!(app.overlays.modal, Some(Modal::Confirm { .. }))
        && let Some(Modal::Confirm { action, .. }) = app.overlays.modal.take()
    {
        answer_confirm(app, yes, action);
    }
}

pub(super) fn answer_confirm(app: &mut App, yes: bool, action: super::ConfirmAction) {
    if yes {
        app.confirmed(action);
    } else {
        app.declined(action);
    }
}

pub(super) fn mouse_pick(app: &mut App, event: MouseEvent) -> bool {
    let Some(panel) = app.overlays.notes_panel.as_mut() else {
        return false;
    };

    if matches!(event.kind, MouseEventKind::Down(_)) {
        let pick = panel.click(Position {
            x: event.column,
            y: event.row,
        });

        app.notes_pick(pick);
    }

    true
}
