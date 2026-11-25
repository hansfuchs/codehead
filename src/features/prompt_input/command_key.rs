use super::{BRANCH, Command, KeyCode, KeyEvent, Level, TextArea, dropdown};

impl Command {
    /// Key for the dropdown. true: used (the input must not see it).
    pub(crate) fn key(&mut self, key_value: KeyEvent, ta: &mut TextArea) -> bool {
        key_command(self, key_value, ta)
    }
}

struct MenuContext<'query> {
    at: crate::ui::textarea::Pos,
    query: &'query str,
    count: usize,
    directory: usize,
    menu: bool,
}

fn key_keycode_up_keycode_down(
    key_value: KeyEvent,
    ta: &mut TextArea,
    context: &MenuContext<'_>,
    command: &mut Command,
) {
    let MenuContext {
        at,
        query,
        count,
        directory,
        menu,
    } = *context;

    command.cursor[directory] = if key_value.code == KeyCode::Up {
        (command.cursor[directory] + count - 1) % count
    } else {
        (command.cursor[directory] + 1) % count
    };

    // items: the item in the text at once
    if !menu
        && let Some(item) = command
            .matched
            .1
            .get(command.cursor[directory])
            .and_then(|&i| command.items.get(i))
    {
        let typed = format!("{}{query}", crate::infra::config::command_key());
        let insert = item.insert.clone();

        command.preview.show(ta, at, &typed, &insert);
    }
}

fn key_command(command: &mut Command, key_value: KeyEvent, ta: &mut TextArea) -> bool {
    let Some((at, query)) = command.sync(ta) else {
        return false;
    };

    let count = command.settle(&query);
    let directory = super::depth(command);

    // 1st level, 0 commands match the text: not a command, keys pass
    if directory == 0 && count == 0 {
        return false;
    }

    let plain = key_value.modifiers.is_empty();
    let menu = !matches!(super::level(directory, command), Level::Items(_));

    match key_value.code {
        KeyCode::Up | KeyCode::Down if plain && count > 0 => {
            key_keycode_up_keycode_down(
                key_value,
                ta,
                &MenuContext {
                    at,
                    query: &query,
                    count,
                    directory,
                    menu,
                },
                command,
            );
        }
        KeyCode::PageUp if plain && count > 0 => {
            command.cursor[directory] = command.cursor[directory].saturating_sub(dropdown::MAX_ROWS);
        }
        KeyCode::PageDown if plain && count > 0 => {
            command.cursor[directory] = (command.cursor[directory] + dropdown::MAX_ROWS).min(count - 1);
        }
        KeyCode::Up | KeyCode::Down | KeyCode::PageUp | KeyCode::PageDown if plain => {}
        KeyCode::Tab | KeyCode::Enter if plain => super::activate(ta, at, &query, command),
        KeyCode::Right
            if plain
                && menu
                && query.is_empty()
                && !(directory == 0
                    && super::menu(Level::Commands, "", command).get(command.cursor[0]) == Some(&BRANCH)) =>
        {
            super::activate(ta, at, &query, command);
        }
        KeyCode::Esc if plain => super::back(ta, at, &query, command),
        // a filter char after a preview: the typed query again, the char after it
        KeyCode::Char(character) if plain && !character.is_whitespace() && command.preview.revert(ta) => {
            return false;
        }
        KeyCode::Left | KeyCode::Backspace if plain && directory > 0 && query.is_empty() => {
            super::back(ta, at, &query, command);
        }
        _ => return false,
    }

    true
}
