use super::{Duration, Focus, Form, Instant, KeyCode, KeyEvent, KeyModifiers, Out, ProjectsTab, projects};

impl ProjectsTab {
    pub(crate) fn key(&mut self, key_value: KeyEvent) -> Out {
        key_projects_tab(self, key_value)
    }
}

fn form_escape(model_state: &mut ProjectsTab) -> Option<Out> {
    let fm = model_state.form.as_ref()?;
    let again = model_state
        .esc_at
        .is_some_and(|thread| thread.elapsed() < Duration::from_secs(2));

    if !fm.edited() || again {
        model_state.form = None;
        model_state.esc_at = None;
    } else {
        model_state.esc_at = Some(Instant::now());
        return Some(Out::Toast("unsaved edits: esc again drops them · ctrl+s saves".into()));
    }

    None
}

fn key_projects_tab(receiver_state: &mut ProjectsTab, key_value: KeyEvent) -> Out {
    let ctrl = key_value.modifiers.contains(KeyModifiers::CONTROL) || key_value.modifiers.contains(KeyModifiers::SUPER);

    if receiver_state.form.is_some() {
        return form_key(receiver_state, key_value, ctrl);
    }

    if receiver_state.del {
        receiver_state.del = false;

        if key_value.code == KeyCode::Char('d') {
            return receiver_state.delete();
        }

        return Out::None;
    }

    let direction = if key_value.code == KeyCode::Right { 1 } else { -1 };

    if receiver_state.focus == Focus::Repos {
        match key_value.code {
            KeyCode::Up => return Out::Up,
            KeyCode::Down | KeyCode::Enter => receiver_state.focus = Focus::List,
            KeyCode::Left | KeyCode::Right => {
                super::step_repo(direction, receiver_state);
            }
            _ => {}
        }

        return Out::None;
    }

    let count = receiver_state.items.len();

    match key_value.code {
        KeyCode::Up | KeyCode::Char('k') if receiver_state.cursor == 0 => receiver_state.focus = Focus::Repos,
        KeyCode::Up | KeyCode::Char('k') => receiver_state.cursor -= 1,
        KeyCode::Down | KeyCode::Char('j') => {
            receiver_state.cursor = (receiver_state.cursor + 1).min(count.saturating_sub(1));
        }
        KeyCode::Left | KeyCode::Right => {
            super::step_repo(direction, receiver_state);
        }
        KeyCode::Char('n') if receiver_state.repo().is_some() => {
            receiver_state.form = Some(Form::new(None, "", projects::TEMPLATE));
            receiver_state.esc_at = None;
        }
        KeyCode::Enter | KeyCode::Char('e') if count > 0 => receiver_state.open_editor(),
        KeyCode::Char('x') if count > 0 => return toggle_done(receiver_state),
        KeyCode::Char('d') if count > 0 => {
            receiver_state.del = true;
        }
        KeyCode::Char('o') if count > 0 => return super::open_ide(receiver_state),
        _ => {}
    }

    Out::None
}

fn form_key(state: &mut ProjectsTab, key: KeyEvent, ctrl: bool) -> Out {
    match key.code {
        KeyCode::Char('s') if ctrl => state.save(),
        KeyCode::Esc => form_escape(state).unwrap_or(Out::None),
        _ => {
            if let Some(form) = state.form.as_mut() {
                form.key(key);
            }

            Out::None
        }
    }
}

impl Form {
    fn key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Tab | KeyCode::BackTab => self.on_title = !self.on_title,
            KeyCode::Enter | KeyCode::Down if self.on_title => self.on_title = false,
            KeyCode::Up if !self.on_title && self.body.on_first_row() => self.on_title = true,
            _ if self.on_title => {
                self.title.key(key);
            }
            _ => {
                self.body.key(key);
            }
        }
    }
}

fn toggle_done(receiver_state: &mut ProjectsTab) -> Out {
    let path = receiver_state.items[receiver_state.cursor].clone();

    if let Err(error) = projects::set_done(&path, !path.done) {
        return Out::Toast(error);
    }

    receiver_state.reload();
    receiver_state.cursor = receiver_state
        .items
        .iter()
        .position(|x| x.slug == path.slug)
        .unwrap_or(0);
    Out::Toast(format!("{}: {}", path.slug, if path.done { "active" } else { "done" }))
}
