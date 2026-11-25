use super::{App, F_HARNESS, F_MODEL, F_NAME, F_PROMPT, F_VOICE, Instant, Modal, MouseEvent, Position};

pub(super) fn mouse_agent_form(app: &mut App, event: MouseEvent, at: Position, down: bool) -> bool {
    let Some(Modal::Agent(form)) = &mut app.overlays.modal else {
        return false;
    };

    let opens = if down && form.pick.is_none() {
        [F_HARNESS, F_MODEL].into_iter().find(|&i| form.areas[i].contains(at))
    } else {
        None
    };

    if down && (form.pick.is_some() || opens.is_some()) {
        let Some(Modal::Agent(mut form)) = app.overlays.modal.take() else {
            return true;
        };

        match opens {
            Some(i) => {
                form.field = i;
                app.open_form_pick(&mut form, i == F_MODEL);
            }
            None => app.form_pick_click(&mut form, at),
        }

        app.overlays.modal = Some(Modal::Agent(form));
        return true;
    }

    if down && let Some(i) = form.areas.iter().position(|rect| rect.contains(at)) {
        form.field = i;
    }

    let field = form.field;

    for (i, ta) in [
        (F_NAME, &mut form.name),
        (F_HARNESS, &mut form.harness),
        (F_VOICE, &mut form.voice),
        (F_PROMPT, &mut form.prompt),
    ] {
        if i == field
            && ta.mouse(event)
            && let Some(chars) = ta.copied.take()
        {
            app.overlays.toast = Some((format!("copied {chars} chars"), Instant::now(), false));
        }
    }

    if down && let Some(&(_, save)) = app.preview.regions.buttons.iter().find(|(rect, _)| rect.contains(at)) {
        let Some(Modal::Agent(form)) = app.overlays.modal.take() else {
            return true;
        };

        if save {
            app.save_agent(form);
        }
    }

    true
}
