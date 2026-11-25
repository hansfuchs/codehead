use super::{Command, MouseButton, MouseEvent, MouseEventKind, Position, TextArea};

impl Command {
    /// Click on a row: that level's row, opened / inserted. Wheel over the open level: its cursor.
    /// true: used.
    pub(crate) fn mouse(&mut self, event: MouseEvent, ta: &mut TextArea) -> bool {
        mouse_command(self, event, ta)
    }
}

fn mouse_mouseeventkind_down_mousebutton_left(
    ta: &mut TextArea,
    path_value: Position,
    at: crate::ui::textarea::Pos,
    query: &str,
    directory: usize,
    command: &mut Command,
) -> Option<bool> {
    let Some((_, level, row)) = command
        .rows
        .iter()
        .find(|(result_value, _, _)| result_value.contains(path_value))
        .copied()
    else {
        return Some(true);
    };

    if level < directory {
        // a row of a parent: back to it, then its submenu (rows of a parent: unfiltered)
        command.path.truncate(level);
        command.cursor[level] = row;
        command.seen = Some((level, String::new()));

        if !query.is_empty() {
            ta.replace_to_cursor(at, &crate::infra::config::command_key().to_string());
        }

        super::activate(ta, at, "", command);
    } else {
        command.cursor[directory] = row;
        super::activate(ta, at, query, command);
    }

    None
}

fn mouse_command(command: &mut Command, event: MouseEvent, ta: &mut TextArea) -> bool {
    let path_value = Position {
        x: event.column,
        y: event.row,
    };

    if !command.areas.iter().any(|result| result.contains(path_value)) {
        return false;
    }

    let Some((at, query)) = command.sync(ta) else {
        return false;
    };

    let count = command.settle(&query);
    let directory = super::depth(command);

    match event.kind {
        MouseEventKind::ScrollUp if count > 0 => {
            command.cursor[directory] = command.cursor[directory].saturating_sub(3);
        }
        MouseEventKind::ScrollDown if count > 0 => {
            command.cursor[directory] = (command.cursor[directory] + 3).min(count - 1);
        }
        MouseEventKind::Down(MouseButton::Left) => {
            if let Some(value) =
                self::mouse_mouseeventkind_down_mousebutton_left(ta, path_value, at, &query, directory, command)
            {
                return value;
            }
        }
        _ => {}
    }

    true
}
