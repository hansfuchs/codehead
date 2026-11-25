use super::{MouseButton, MouseEvent, MouseEventKind, Path, Position, ShellComp, TextArea};

impl ShellComp {
    /// Click on a row: taken. Wheel: the cursor. True: used.
    pub(crate) fn mouse(&mut self, event: MouseEvent, ta: &mut TextArea, dir: &Path, thread: &[String]) -> bool {
        mouse_shell_comp(self, event, ta, dir, thread)
    }
}

fn mouse_mouseeventkind_down_mousebutton_left(
    ta: &mut TextArea,
    path: Position,
    ctx: &super::Ctx,
    entries: &[super::Entry],
    shell: &mut ShellComp,
) {
    if let Some(&(_, i)) = shell.rows.iter().find(|(result, _)| result.contains(path)) {
        let error = entries[i].clone();

        shell.pick(ta, ctx, &error);
    }
}

fn mouse_shell_comp(
    shell: &mut ShellComp,
    event: MouseEvent,
    ta: &mut TextArea,
    dir: &Path,
    thread: &[String],
) -> bool {
    let path = Position {
        x: event.column,
        y: event.row,
    };

    if !shell.area.contains(path) {
        return false;
    }

    let Some((ctx, entries)) = shell.open(ta, dir, thread) else {
        return false;
    };

    match event.kind {
        MouseEventKind::ScrollUp => shell.cursor = shell.cursor.saturating_sub(3),
        MouseEventKind::ScrollDown => shell.cursor = (shell.cursor + 3).min(entries.len() - 1),
        MouseEventKind::Down(MouseButton::Left) => {
            self::mouse_mouseeventkind_down_mousebutton_left(ta, path, &ctx, &entries, shell);
        }
        _ => {}
    }

    true
}
