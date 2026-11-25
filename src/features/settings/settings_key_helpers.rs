use super::{Action, Form, KeyCode, KeyEvent, KeyModifiers, Settings, Sub};

impl Form {
    pub(crate) fn suggestions_key(&mut self, key: KeyEvent) -> bool {
        super::updates::sync_paths(self);
        self.path_focus
            .is_some_and(|index| self.paths.key(key, &mut self.fields[index]))
    }
}

impl Settings {
    pub(crate) fn suggestions_key(&mut self, key: KeyEvent) -> bool {
        !self.bar && self.sub == Sub::Codehead && self.form.suggestions_key(key)
    }
}

pub(super) fn key_inner(key: KeyEvent, form: &mut Form) -> Action {
    let armed = std::mem::take(&mut form.del_armed);

    if form.suggestions_key(key) {
        return Action::None;
    }

    if key.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::SUPER) && command_key(key, form) {
        return Action::None;
    }

    if let Some(i) = form.editing {
        edit_alias_key(key, i, form);
        return Action::None;
    }

    match key.code {
        // esc: the field's edit undone, 0 commit
        KeyCode::Esc => super::revert(form),
        KeyCode::Up if key.modifiers.is_empty() => {
            if let Some(action) = previous_field(form) {
                return action;
            }
        }
        KeyCode::Down if key.modifiers.is_empty() => {
            form.commit();
            form.focus = (form.focus + 1).min(super::add_item(form));
        }
        KeyCode::PageUp => {
            previous_section(form);
        }
        KeyCode::PageDown => {
            next_section(form);
        }
        KeyCode::Enter if form.aliases_on && form.focus == super::add_item(form) => {
            form.add_alias();
        }
        KeyCode::Enter if form.focus >= form.plain() => {
            form.editing = Some(form.plain() + (form.focus - form.plain()) * 2);
        }
        KeyCode::Enter => {
            form.commit();
            form.focus = (form.focus + 1).min(super::add_item(form));
        }
        // d d: delete the alias box
        KeyCode::Char('d')
            if key.modifiers.is_empty() && (form.plain()..super::add_item(form)).contains(&form.focus) =>
        {
            delete_alias_key(form, armed);
        }
        _ => {
            if let Some(i) = super::field(form) {
                form.fields[i].key(key);
            }
        }
    }

    Action::None
}

fn delete_alias_key(form: &mut Form, armed: bool) {
    if armed {
        super::delete_alias(form.focus - form.plain(), form);
    } else {
        form.del_armed = true;
    }
}

fn next_section(form: &mut Form) {
    form.commit();
    form.focus = super::starts(form)
        .into_iter()
        .find(|&start| start > form.focus)
        .unwrap_or(super::add_item(form));
}

fn previous_section(form: &mut Form) {
    form.commit();
    form.focus = super::starts(form)
        .into_iter()
        .rev()
        .find(|&start| start < form.focus)
        .unwrap_or(0);
}

fn previous_field(form: &mut Form) -> Option<Action> {
    form.commit();

    if form.focus == 0 {
        return Some(Action::Header);
    }

    form.focus -= 1;

    None
}

fn command_key(key: KeyEvent, form: &mut Form) -> bool {
    match key.code {
        KeyCode::Char('s' | 'S') => {
            form.commit();
            return true;
        }
        KeyCode::Char('r' | 'R') => {
            form.applied = super::reload(form);
            return true;
        }
        // ctrl+a: the text field's row start (+ add / enter adds an alias)
        KeyCode::Char('d' | 'D') if form.focus < form.plain() => {
            super::reset(form.focus, form);
            return true;
        }
        KeyCode::Char('d' | 'D') if (form.plain()..super::add_item(form)).contains(&form.focus) => {
            super::delete_alias(form.focus - form.plain(), form);
            return true;
        }
        _ => {}
    }

    false
}

fn edit_alias_key(key: KeyEvent, i: usize, form: &mut Form) {
    // the open alias box: ↑↓ name / target, enter / esc: lock it again
    match key.code {
        KeyCode::Enter => {
            form.editing = None;
            form.commit();
        }
        // esc: the alias as it was at unlock, locked, 0 commit
        KeyCode::Esc => super::revert(form),
        KeyCode::Up if key.modifiers.is_empty() && !(i - form.plain()).is_multiple_of(2) => {
            form.editing = Some(i - 1);
        }
        KeyCode::Down if key.modifiers.is_empty() && (i - form.plain()).is_multiple_of(2) => {
            form.editing = Some(i + 1);
        }
        KeyCode::Up | KeyCode::Down if key.modifiers.is_empty() => {}
        _ => {
            form.fields[i].key(key);
        }
    }
}
