use super::{Focus, MouseButton, MouseEvent, MouseEventKind, Out, Position, ProjectsTab};

impl ProjectsTab {
    pub(crate) fn mouse(&mut self, event: MouseEvent) -> Out {
        mouse_projects_tab(self, event)
    }
}

fn click_project_tab(at: Position, model_state: &mut ProjectsTab) {
    if model_state.tabs.click(at) {
        model_state.focus = Focus::Repos;
        model_state.repo_i = model_state.tabs.i;
        model_state.cursor = 0;
        model_state.reload();
    } else if model_state.list.contains(at) {
        let i = (at.y - model_state.list.y) as usize;

        if i < model_state.items.len() {
            model_state.focus = Focus::List;

            open_selected_project(model_state, i);

            model_state.cursor = i;
        }
    }
}

fn open_selected_project(model_state: &mut ProjectsTab, i: usize) {
    if model_state.cursor == i {
        model_state.open_editor();
    }
}

fn mouse_projects_tab(receiver_state: &mut ProjectsTab, event: MouseEvent) -> Out {
    let at = Position {
        x: event.column,
        y: event.row,
    };

    if receiver_state.typing() {
        if let Some(fm) = receiver_state.form.as_mut() {
            fm.mouse(event, at);
        }

        return Out::None;
    }

    match event.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            self::click_project_tab(at, receiver_state);
        }
        MouseEventKind::ScrollDown => {
            receiver_state.cursor = (receiver_state.cursor + 1).min(receiver_state.items.len().saturating_sub(1));
        }
        MouseEventKind::ScrollUp => receiver_state.cursor = receiver_state.cursor.saturating_sub(1),
        _ => {}
    }

    Out::None
}

impl super::Form {
    fn mouse(&mut self, event: MouseEvent, at: Position) {
        self.focus_mouse(event, at);

        if self.on_title {
            self.title.mouse(event);
        } else {
            self.body.mouse(event);
        }
    }
    fn focus_mouse(&mut self, event: MouseEvent, at: Position) {
        if !matches!(event.kind, MouseEventKind::Down(MouseButton::Left)) {
            return;
        }

        if self.title.area().contains(at) {
            self.on_title = true;
        } else if self.body.area().contains(at) {
            self.on_title = false;
        }
    }
}
