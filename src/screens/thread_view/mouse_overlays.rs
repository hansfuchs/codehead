use super::{MouseEvent, MouseEventKind, Position, View};

impl View {
    pub(super) fn mouse_overlays(&mut self, event: MouseEvent) -> bool {
        if self.notes_panel.is_some() {
            mouse_notes(self, event);
            return true;
        }

        if self.ctx.is_some() {
            mouse_context(self, event);
            return true;
        }

        if self.menu.is_some() {
            mouse_menu(self, event);
            return true;
        }

        if self.ide.ide_pick.is_some() {
            mouse_ide(self, event);
            return true;
        }

        false
    }

    pub(super) fn mouse_activity(&mut self, event: MouseEvent) -> bool {
        self.flow_mouse(event)
            || self.subthreads.mouse(event)
            || self.subagents.mouse(event)
            || self.monitors.mouse(event)
            || self
                .prompt
                .mention
                .mouse(event, &mut self.prompt.input, &self.agents.roster)
            || self.prompt.command.mouse(event, &mut self.prompt.input)
            || self.prompt.shortcuts.mouse(event, &mut self.prompt.input)
            || self.prompt.paths.mouse(event, &mut self.prompt.input)
            || mouse_completions(self, event)
    }
}

fn mouse_completions(view: &mut View, event: MouseEvent) -> bool {
    if !view.comp_on() {
        return false;
    }

    let commands = view.shell_cmds();

    view.prompt
        .comp
        .mouse(event, &mut view.prompt.input, &view.shell_state.shell_cwd, &commands)
}

fn mouse_notes(view: &mut View, event: MouseEvent) {
    let Some(panel) = view.notes_panel.as_mut() else {
        return;
    };

    if !matches!(event.kind, MouseEventKind::Down(_)) {
        return;
    }

    let picked = panel.click(Position::new(event.column, event.row));

    if view.notes_pick(picked) {
        view.mouse_exit = true;
    }
}

fn mouse_context(view: &mut View, event: MouseEvent) {
    let Some(menu) = view.ctx.as_mut() else {
        return;
    };

    if !matches!(
        event.kind,
        MouseEventKind::Down(_) | MouseEventKind::ScrollUp | MouseEventKind::ScrollDown
    ) {
        return;
    }

    let output = menu.mouse(event);

    if view.ctx_out(&output) {
        view.mouse_exit = true;
    }
}

fn mouse_menu(view: &mut View, event: MouseEvent) {
    let Some(menu) = view.menu.as_mut() else {
        return;
    };

    let action = menu.mouse(event);

    if view.menu_action(action) {
        view.menu = None;
        view.mouse_exit = true;
    }
}

fn mouse_ide(view: &mut View, event: MouseEvent) {
    if !matches!(event.kind, MouseEventKind::Down(_)) {
        return;
    }

    let at = Position::new(event.column, event.row);

    match view
        .ide
        .ide_rows
        .iter()
        .find(|(area, _)| area.contains(at))
        .map(|(_, index)| *index)
    {
        Some(index) => view.ide_pick_open(index),
        None => view.ide.ide_pick = None,
    }

    view.stream.dirty = true;
}
