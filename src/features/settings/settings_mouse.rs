use super::{Action, Form, Hit, MouseButton, MouseEvent, MouseEventKind, Position, Settings, Sub};

impl Form {
    pub(crate) fn mouse(&mut self, event: MouseEvent) -> Action {
        mouse_form(self, event)
    }
}

impl Settings {
    pub(crate) fn mouse(&mut self, event: MouseEvent) -> Action {
        mouse_settings(self, event)
    }
}

fn open_alias_field(i: usize, form: &mut Form) {
    let alias = super::alias_of(i, form);

    if form.editing.is_none_or(|open| super::alias_of(open, form) != alias) {
        form.commit();
    }

    form.focus = form.plain() + alias;
    form.editing = Some(i);
}

fn focus_plain_field(i: usize, form: &mut Form) {
    if form.focus != i {
        form.commit();
    }

    form.editing = None;
    form.focus = i;
}

fn mouse_form(form: &mut Form, event: MouseEvent) -> Action {
    super::updates::sync_paths(form);

    if form
        .path_focus
        .is_some_and(|index| form.paths.mouse(event, &mut form.fields[index]))
    {
        return Action::None;
    }

    let at = Position {
        x: event.column,
        y: event.row,
    };

    match event.kind {
        MouseEventKind::ScrollUp => {
            form.offset = form.offset.saturating_sub(3);
            return Action::None;
        }
        MouseEventKind::ScrollDown => {
            form.offset += 3;
            return Action::None;
        }
        _ => {}
    }

    if matches!(event.kind, MouseEventKind::Down(MouseButton::Left))
        && let Some(&(_, hit)) = form.rows.iter().find(|(rect, _)| rect.contains(at))
    {
        match hit {
            Hit::Add => {
                form.focus = super::add_item(form);
                form.add_alias();
                return Action::None;
            }
            Hit::Reset(i) => {
                form.focus = i;
                form.editing = None;
                super::reset(i, form);
                return Action::None;
            }
            Hit::Field(i) if i < form.plain() => {
                focus_plain_field(i, form);
            }
            Hit::Field(i) => {
                open_alias_field(i, form);
            }
        }
    }

    if let Some(i) = super::field(form) {
        form.fields[i].mouse(event);
    }

    Action::None
}

fn mouse_settings(settings: &mut Settings, event: MouseEvent) -> Action {
    let at = Position {
        x: event.column,
        y: event.row,
    };

    if matches!(event.kind, MouseEventKind::Down(MouseButton::Left))
        && let Some(&(_, sub)) = settings.bar_hits.iter().find(|(rect, _)| rect.contains(at))
    {
        settings.switch(sub);
        settings.bar = false;
        return Action::None;
    }

    settings.bar = false;

    match settings.sub {
        Sub::Codehead => settings.form.mouse(event),
        Sub::Harnesses => settings.harness.mouse(event),
        Sub::Mcp => {
            settings.mcp.mouse(event);
            Action::None
        }
        Sub::Shortcuts => {
            settings.shortcuts.mouse(event);
            Action::None
        }
    }
}
