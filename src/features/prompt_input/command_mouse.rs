use super::{Command, MouseButton, MouseEvent, MouseEventKind, Position, TextArea};

impl Command {
    /// Click on a row: that level's row, opened / inserted. Wheel over the open level: its cursor.
    /// true: used.
    pub(crate) fn mouse(&mut self, event: MouseEvent, ta: &mut TextArea) -> bool {
        mouse_command(self, event, ta)
    }
}

fn click_row(
    ta: &mut TextArea,
    point: Position,
    at: crate::ui::textarea::Pos,
    query: &str,
    depth: usize,
    command: &mut Command,
) -> Option<bool> {
    let Some((_, level, row)) = command.rows.iter().find(|(rect, _, _)| rect.contains(point)).copied() else {
        return Some(true);
    };

    if level < depth {
        // a row of a parent: back to it, then its submenu (rows of a parent: unfiltered)
        command.path.truncate(level);
        command.cursor[level] = row;
        command.seen = Some((level, String::new()));

        if !query.is_empty() {
            ta.replace_to_cursor(at, &crate::infra::config::command_key().to_string());
        }

        super::activate(ta, at, "", command);
    } else {
        command.cursor[depth] = row;
        super::activate(ta, at, query, command);
    }

    None
}

fn mouse_command(command: &mut Command, event: MouseEvent, ta: &mut TextArea) -> bool {
    let point = Position {
        x: event.column,
        y: event.row,
    };

    if !command.areas.iter().any(|rect| rect.contains(point)) {
        return false;
    }

    let Some((at, query)) = command.sync(ta) else {
        return false;
    };

    let count = command.settle(&query);
    let depth = super::depth(command);

    match event.kind {
        MouseEventKind::ScrollUp if count > 0 => {
            command.cursor[depth] = command.cursor[depth].saturating_sub(3);
        }
        MouseEventKind::ScrollDown if count > 0 => {
            command.cursor[depth] = (command.cursor[depth] + 3).min(count - 1);
        }
        MouseEventKind::Down(MouseButton::Left) => {
            if let Some(used) = self::click_row(ta, point, at, &query, depth, command) {
                return used;
            }
        }
        _ => {}
    }

    true
}
