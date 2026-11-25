use super::{MouseButton, MouseEvent, MouseEventKind, Path, Position, ShellComp, TextArea};

impl ShellComp {
    /// Click on a row: taken. Wheel: the cursor. True: used.
    pub(crate) fn mouse(
        &mut self,
        event: MouseEvent,
        ta: &mut TextArea,
        dir: &Path,
        thread_commands: &[String],
    ) -> bool {
        mouse_shell_comp(self, event, ta, dir, thread_commands)
    }
}

fn click_row(ta: &mut TextArea, point: Position, ctx: &super::Ctx, entries: &[super::Entry], shell: &mut ShellComp) {
    if let Some(&(_, i)) = shell.rows.iter().find(|(rect, _)| rect.contains(point)) {
        let entry = entries[i].clone();

        shell.pick(ta, ctx, &entry);
    }
}

fn mouse_shell_comp(
    shell: &mut ShellComp,
    event: MouseEvent,
    ta: &mut TextArea,
    dir: &Path,
    thread_commands: &[String],
) -> bool {
    let point = Position {
        x: event.column,
        y: event.row,
    };

    if !shell.area.contains(point) {
        return false;
    }

    let Some((ctx, entries)) = shell.open(ta, dir, thread_commands) else {
        return false;
    };

    match event.kind {
        MouseEventKind::ScrollUp => shell.cursor = shell.cursor.saturating_sub(3),
        MouseEventKind::ScrollDown => shell.cursor = (shell.cursor + 3).min(entries.len() - 1),
        MouseEventKind::Down(MouseButton::Left) => {
            self::click_row(ta, point, &ctx, &entries, shell);
        }
        _ => {}
    }

    true
}
