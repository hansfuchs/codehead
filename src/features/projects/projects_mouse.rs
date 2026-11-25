use super::{Focus, MouseButton, MouseEvent, MouseEventKind, Out, Position, ProjectsTab};

impl ProjectsTab {
    pub(crate) fn mouse(&mut self, event: MouseEvent) -> Out {
        mouse_projects_tab(self, event)
    }
}

fn click_project_tab(at: Position, tab: &mut ProjectsTab) {
    if tab.tabs.click(at) {
        tab.focus = Focus::Repos;
        tab.repo_i = tab.tabs.i;
        tab.cursor = 0;
        tab.reload();
    } else if tab.list.contains(at) {
        let i = (at.y - tab.list.y) as usize;

        if i < tab.items.len() {
            tab.focus = Focus::List;

            open_selected_project(tab, i);

            tab.cursor = i;
        }
    }
}

fn open_selected_project(tab: &mut ProjectsTab, i: usize) {
    if tab.cursor == i {
        tab.open_editor();
    }
}

fn mouse_projects_tab(tab: &mut ProjectsTab, event: MouseEvent) -> Out {
    let at = Position {
        x: event.column,
        y: event.row,
    };

    if tab.typing() {
        if let Some(form) = tab.form.as_mut() {
            form.mouse(event, at);
        }

        return Out::None;
    }

    match event.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            self::click_project_tab(at, tab);
        }
        MouseEventKind::ScrollDown => {
            tab.cursor = (tab.cursor + 1).min(tab.items.len().saturating_sub(1));
        }
        MouseEventKind::ScrollUp => tab.cursor = tab.cursor.saturating_sub(1),
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
