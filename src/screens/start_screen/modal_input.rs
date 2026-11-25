use super::form_input::FormAction;
use super::{AgentForm, App, ConfirmAction, KeyCode, KeyEvent, Line, Modal};

impl App {
    pub(super) fn modal_key(&mut self, key: KeyEvent) {
        let Some(modal) = self.overlays.modal.take() else {
            return;
        };

        match modal {
            Modal::Confirm {
                title,
                question,
                yes,
                action,
            } => confirm_key(self, title, question, yes, action, key),
            Modal::Agent(form) => agent_form_key(self, form, key),
            Modal::Info { title, lines, scroll } => info_key(self, title, lines, scroll, key),
        }
    }
}

fn picker_key(form: &mut AgentForm, key: KeyEvent) -> bool {
    use crate::features::agents::avatar_picker::Act;

    let Some(picker) = form.picker.as_mut() else {
        return false;
    };

    match picker.key(key) {
        Act::None => {}
        Act::Close => form.picker = None,
        Act::Take(sprite) => {
            form.sprite = sprite;
            form.picker = None;
        }
    }

    true
}

fn confirm_key(app: &mut App, title: String, question: String, yes: bool, action: ConfirmAction, key: KeyEvent) {
    match key.code {
        KeyCode::Char('y') => app.confirmed(action),
        KeyCode::Enter if yes => app.confirmed(action),
        KeyCode::Char('n') | KeyCode::Esc | KeyCode::Enter => app.declined(action),
        _ => {
            let yes = if matches!(key.code, KeyCode::Left | KeyCode::Right | KeyCode::Char('h' | 'l')) {
                !yes
            } else {
                yes
            };

            app.overlays.modal = Some(Modal::Confirm {
                title,
                question,
                yes,
                action,
            });
        }
    }
}

fn agent_form_key(app: &mut App, mut form: Box<AgentForm>, key: KeyEvent) {
    if picker_key(&mut form, key) {
        app.overlays.modal = Some(Modal::Agent(form));
        return;
    }

    if form.pick.is_some() {
        app.form_pick_key(&mut form, key);
        app.overlays.modal = Some(Modal::Agent(form));
        return;
    }

    match form.key(key) {
        FormAction::Keep => app.overlays.modal = Some(Modal::Agent(form)),
        FormAction::Close => {}
        FormAction::Discard => discard_agent_form(app, form),
        FormAction::Save => app.save_agent(form),
        FormAction::Models(model) => {
            app.open_form_pick(&mut form, model);
            app.overlays.modal = Some(Modal::Agent(form));
        }
    }
}

fn discard_agent_form(app: &mut App, form: Box<AgentForm>) {
    let what = if form.edit.is_some() {
        "edits of this agent"
    } else {
        "this new agent"
    };

    app.overlays.modal = Some(Modal::Confirm {
        title: "discard".into(),
        question: format!("drop the {what}?"),
        yes: false,
        action: ConfirmAction::DiscardAgent(form),
    });
}

fn info_key(app: &mut App, title: String, lines: Vec<Line<'static>>, scroll: u16, key: KeyEvent) {
    let scroll = match key.code {
        KeyCode::Enter | KeyCode::Esc | KeyCode::Char('q') => return,
        KeyCode::Down | KeyCode::Char('j') => scroll.saturating_add(1),
        KeyCode::Up | KeyCode::Char('k') => scroll.saturating_sub(1),
        _ => scroll,
    };

    app.overlays.modal = Some(Modal::Info { title, lines, scroll });
}
