use super::{App, F_HARNESS, F_MODEL, F_NAME, F_PROMPT, F_VOICE, Instant, Modal, MouseEvent, Position};

pub(super) fn mouse_agent_form(app_state: &mut App, event: MouseEvent, at: Position, down: bool) -> bool {
    let Some(Modal::Agent(form)) = &mut app_state.overlays.modal else {
        return false;
    };

    let opens = if down && form.pick.is_none() {
        [F_HARNESS, F_MODEL].into_iter().find(|&i| form.areas[i].contains(at))
    } else {
        None
    };

    if down && (form.pick.is_some() || opens.is_some()) {
        let Some(Modal::Agent(mut form)) = app_state.overlays.modal.take() else {
            return true;
        };

        match opens {
            Some(i) => {
                form.field = i;
                app_state.open_form_pick(&mut form, i == F_MODEL);
            }
            None => app_state.form_pick_click(&mut form, at),
        }

        app_state.overlays.modal = Some(Modal::Agent(form));
        return true;
    }

    if down && let Some(i) = form.areas.iter().position(|result| result.contains(at)) {
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
            && let Some(count) = ta.copied.take()
        {
            app_state.overlays.toast = Some((format!("copied {count} chars"), Instant::now(), false));
        }
    }

    if down
        && let Some(&(_, save)) = app_state
            .preview
            .regions
            .buttons
            .iter()
            .find(|(result_value, _)| result_value.contains(at))
    {
        let Some(Modal::Agent(form)) = app_state.overlays.modal.take() else {
            return true;
        };

        if save {
            app_state.save_agent(form);
        }
    }

    true
}
