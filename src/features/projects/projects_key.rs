use super::{Duration, Focus, Form, Instant, KeyCode, KeyEvent, KeyModifiers, Out, ProjectsTab, projects};

impl ProjectsTab {
    pub(crate) fn key(&mut self, key: KeyEvent) -> Out {
        key_projects_tab(self, key)
    }
}

fn form_escape(tab: &mut ProjectsTab) -> Option<Out> {
    let form = tab.form.as_ref()?;
    let again = tab
        .esc_at
        .is_some_and(|pressed| pressed.elapsed() < Duration::from_secs(2));

    if !form.edited() || again {
        tab.form = None;
        tab.esc_at = None;
    } else {
        tab.esc_at = Some(Instant::now());
        return Some(Out::Toast("unsaved edits: esc again drops them · ctrl+s saves".into()));
    }

    None
}

fn key_projects_tab(tab: &mut ProjectsTab, key: KeyEvent) -> Out {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL) || key.modifiers.contains(KeyModifiers::SUPER);

    if tab.form.is_some() {
        return form_key(tab, key, ctrl);
    }

    if tab.del {
        tab.del = false;

        if key.code == KeyCode::Char('d') {
            return tab.delete();
        }

        return Out::None;
    }

    let direction = if key.code == KeyCode::Right { 1 } else { -1 };

    if tab.focus == Focus::Repos {
        match key.code {
            KeyCode::Up => return Out::Up,
            KeyCode::Down | KeyCode::Enter => tab.focus = Focus::List,
            KeyCode::Left | KeyCode::Right => {
                super::step_repo(direction, tab);
            }
            _ => {}
        }

        return Out::None;
    }

    let count = tab.items.len();

    match key.code {
        KeyCode::Up | KeyCode::Char('k') if tab.cursor == 0 => tab.focus = Focus::Repos,
        KeyCode::Up | KeyCode::Char('k') => tab.cursor -= 1,
        KeyCode::Down | KeyCode::Char('j') => {
            tab.cursor = (tab.cursor + 1).min(count.saturating_sub(1));
        }
        KeyCode::Left | KeyCode::Right => {
            super::step_repo(direction, tab);
        }
        KeyCode::Char('n') if tab.repo().is_some() => {
            tab.form = Some(Form::new(None, "", projects::TEMPLATE));
            tab.esc_at = None;
        }
        KeyCode::Enter | KeyCode::Char('e') if count > 0 => tab.open_editor(),
        KeyCode::Char('x') if count > 0 => return toggle_done(tab),
        KeyCode::Char('d') if count > 0 => {
            tab.del = true;
        }
        KeyCode::Char('o') if count > 0 => return super::open_ide(tab),
        _ => {}
    }

    Out::None
}

fn form_key(tab: &mut ProjectsTab, key: KeyEvent, ctrl: bool) -> Out {
    match key.code {
        KeyCode::Char('s') if ctrl => tab.save(),
        KeyCode::Esc => form_escape(tab).unwrap_or(Out::None),
        _ => {
            if let Some(form) = tab.form.as_mut() {
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

fn toggle_done(tab: &mut ProjectsTab) -> Out {
    let project = tab.items[tab.cursor].clone();

    if let Err(error) = projects::set_done(&project, !project.done) {
        return Out::Toast(error);
    }

    tab.reload();
    tab.cursor = tab.items.iter().position(|item| item.slug == project.slug).unwrap_or(0);
    Out::Toast(format!(
        "{}: {}",
        project.slug,
        if project.done { "active" } else { "done" }
    ))
}
