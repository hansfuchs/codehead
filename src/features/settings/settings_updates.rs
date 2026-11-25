use super::Form;

impl Form {}

pub(super) fn sync_paths(form: &mut Form) {
    let focus = super::field(form)
        .filter(|&i| i < form.path_fields || (i >= form.plain() && !(i - form.plain()).is_multiple_of(2)));

    if focus != form.path_focus {
        form.paths.reset_input();
        form.path_focus = focus;
    }
}
