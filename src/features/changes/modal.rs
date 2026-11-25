use super::{
    Act, Frame, KeyCode, KeyEvent, KeyModifiers, Modal, MouseButton, MouseEvent, MouseEventKind, Rect, View,
    modal_draw, modal_state,
};
use crate::domain::changes::rank::Level;
use crate::ui::textarea::TextArea;

#[derive(Clone, Copy)]
enum ModalAction {
    Continue,
    Close,
    Save,
    Confirm(bool),
}

impl View {
    pub(super) fn modal_key(&mut self, key: KeyEvent) -> Act {
        modal_key_view(self, key)
    }

    /// A mouse event while a modal is open: the text takes it, a click on a button answers.
    pub(super) fn modal_mouse(&mut self, event: MouseEvent) -> Act {
        modal_mouse_view(self, event)
    }

    pub(super) fn draw_modal(&mut self, frame: &mut Frame, area: Rect) {
        draw_modal_view(self, frame, area);
    }
}

fn scroll_comment_key(key: KeyEvent, scroll: &mut usize) {
    *scroll = if key.code == KeyCode::Up {
        scroll.saturating_sub(1)
    } else {
        *scroll + 1
    };
}

fn confirm_key(key: KeyEvent, yes: &mut bool) -> ModalAction {
    match key.code {
        KeyCode::Esc | KeyCode::Char('n') => return ModalAction::Close,
        KeyCode::Left | KeyCode::Right | KeyCode::Tab | KeyCode::Char('h' | 'l') => *yes = !*yes,
        KeyCode::Char('y') => return ModalAction::Confirm(true),
        KeyCode::Enter => return ModalAction::Confirm(*yes),
        _ => {}
    }

    ModalAction::Continue
}

fn comment_level_key(key: KeyEvent, send: bool, text: &mut TextArea, level: &mut Level, on: &mut bool) -> ModalAction {
    match key.code {
        KeyCode::Esc => return ModalAction::Close,
        KeyCode::Enter if send => return ModalAction::Save,
        KeyCode::Left | KeyCode::Right => *level = level.step(if key.code == KeyCode::Right { 1 } else { -1 }),
        KeyCode::Up => {}
        KeyCode::Down | KeyCode::Enter | KeyCode::Tab => *on = false,
        _ => {
            *on = false;
            text.key(key);
        }
    }

    ModalAction::Continue
}

fn comment_text_key(
    key: KeyEvent,
    send: bool,
    text: &mut TextArea,
    scroll: &mut usize,
    on_level: &mut bool,
    target: &super::Target,
) -> ModalAction {
    match key.code {
        KeyCode::Esc => return ModalAction::Close,
        KeyCode::Enter if send => return ModalAction::Save,
        KeyCode::Up if !key.modifiers.contains(KeyModifiers::ALT) && text.on_first_row() && target.has_level() => {
            *on_level = true;
        }
        KeyCode::BackTab if target.has_level() => *on_level = true,
        KeyCode::Up if key.modifiers.contains(KeyModifiers::ALT) => *scroll = scroll.saturating_sub(1),
        KeyCode::Down if key.modifiers.contains(KeyModifiers::ALT) => *scroll += 1,
        _ => {
            if !matches!(text.key(key), crate::ui::textarea::Outcome::Handled) && key.code == KeyCode::Enter {
                text.newline();
            }
        }
    }

    ModalAction::Continue
}

fn modal_key_guard(view: &mut View, key: KeyEvent, send: bool) -> Option<Act> {
    if let Some(Modal::Comment {
        text, on_level: false, ..
    }) = view.modal_state.modal.as_mut()
        && !send
        && view.comment_state.cmention.key(key, text, &view.agents)
    {
        return Some(Act::None);
    }

    if key.code == KeyCode::Esc
        && let Some(Modal::Comment { text, .. }) = &view.modal_state.modal
    {
        let dirty = text.edited();

        if !crate::ui::textarea::esc_drops(&mut view.modal_state.esc_at, dirty) {
            return Some(Act::Toast("esc again: drop the comment · ctrl+⏎: save it".into()));
        }

        view.modal_state.modal = None;
        return Some(Act::None);
    }

    None
}

fn modal_action(view: &mut View, action: ModalAction) -> Act {
    match action {
        ModalAction::Continue => Act::None,
        ModalAction::Save => view.save_comment(),
        ModalAction::Confirm(yes) => view.confirm(yes),
        ModalAction::Close => {
            view.modal_state.modal = None;
            Act::None
        }
    }
}

fn modal_key_view(view: &mut View, key: KeyEvent) -> Act {
    let send = key
        .modifiers
        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER);

    if let Some(action) = modal_key_guard(view, key, send) {
        return action;
    }

    let action = match view.modal_state.modal.as_mut() {
        Some(Modal::Comment { scroll, .. })
            if key.modifiers.contains(KeyModifiers::ALT) && matches!(key.code, KeyCode::Up | KeyCode::Down) =>
        {
            scroll_comment_key(key, scroll);
            ModalAction::Continue
        }
        Some(Modal::Comment {
            text,
            level,
            on_level: on @ true,
            ..
        }) => comment_level_key(key, send, text, level, on),
        Some(Modal::Comment {
            text,
            scroll,
            on_level,
            target,
            ..
        }) => comment_text_key(key, send, text, scroll, on_level, target),
        Some(Modal::Confirm { yes, .. }) => confirm_key(key, yes),
        None => ModalAction::Continue,
    };

    modal_action(view, action)
}

fn modal_mouse_view(view: &mut View, event: MouseEvent) -> Act {
    let at = ratatui::layout::Position {
        x: event.column,
        y: event.row,
    };
    let click = matches!(event.kind, MouseEventKind::Down(MouseButton::Left));

    if click && let Some(&(_, yes)) = view.modal_state.modal_btns.iter().find(|(rect, _)| rect.contains(at)) {
        return modal_button(view, yes);
    }

    let in_quote = view.modal_state.modal_quote.contains(at);

    if let Some(Modal::Comment { text, scroll, .. }) = view.modal_state.modal.as_mut() {
        match event.kind {
            MouseEventKind::ScrollUp if in_quote => *scroll = scroll.saturating_sub(3),
            MouseEventKind::ScrollDown if in_quote => *scroll += 3,
            _ => {
                text.mouse(event);
            }
        }
    }

    Act::None
}

fn draw_modal_view(view: &mut View, frame: &mut Frame, area: Rect) {
    let modal_state::ModalState {
        modal,
        modal_btns,
        modal_quote,
        ..
    } = &mut view.modal_state;

    modal_btns.clear();

    let Some(modal) = modal.as_mut() else { return };
    let mut draw = modal_draw::ModalDraw {
        buttons: modal_btns,
        quote_rect: modal_quote,
        mention: &mut view.comment_state.cmention,
        agents: &view.agents,
    };

    draw.draw(frame, area, modal);
}

fn modal_button(view: &mut View, yes: bool) -> Act {
    if !matches!(view.modal_state.modal, Some(Modal::Comment { .. })) {
        return view.confirm(yes);
    }

    if yes {
        return view.save_comment();
    }

    view.modal_state.modal = None;
    Act::None
}
