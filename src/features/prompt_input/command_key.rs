use super::{BRANCH, Command, KeyCode, KeyEvent, Level, TextArea, dropdown};

impl Command {
    /// Key for the dropdown. true: used (the input must not see it).
    pub(crate) fn key(&mut self, key: KeyEvent, ta: &mut TextArea) -> bool {
        key_command(self, key, ta)
    }
}

struct MenuContext<'query> {
    at: crate::ui::textarea::Pos,
    query: &'query str,
    count: usize,
    depth: usize,
    menu: bool,
}

fn move_cursor(key: KeyEvent, ta: &mut TextArea, context: &MenuContext<'_>, command: &mut Command) {
    let MenuContext {
        at,
        query,
        count,
        depth,
        menu,
    } = *context;

    command.cursor[depth] = if key.code == KeyCode::Up {
        (command.cursor[depth] + count - 1) % count
    } else {
        (command.cursor[depth] + 1) % count
    };

    // items: the item in the text at once
    if !menu
        && let Some(item) = command
            .matched
            .1
            .get(command.cursor[depth])
            .and_then(|&i| command.items.get(i))
    {
        let typed = format!("{}{query}", crate::infra::config::command_key());
        let insert = item.insert.clone();

        command.preview.show(ta, at, &typed, &insert);
    }
}

fn key_command(command: &mut Command, key: KeyEvent, ta: &mut TextArea) -> bool {
    let Some((at, query)) = command.sync(ta) else {
        return false;
    };

    let count = command.settle(&query);
    let depth = super::depth(command);

    // 1st level, 0 commands match the text: not a command, keys pass
    if depth == 0 && count == 0 {
        return false;
    }

    let plain = key.modifiers.is_empty();
    let menu = !matches!(super::level(depth, command), Level::Items(_));

    match key.code {
        KeyCode::Up | KeyCode::Down if plain && count > 0 => {
            move_cursor(
                key,
                ta,
                &MenuContext {
                    at,
                    query: &query,
                    count,
                    depth,
                    menu,
                },
                command,
            );
        }
        KeyCode::PageUp if plain && count > 0 => {
            command.cursor[depth] = command.cursor[depth].saturating_sub(dropdown::MAX_ROWS);
        }
        KeyCode::PageDown if plain && count > 0 => {
            command.cursor[depth] = (command.cursor[depth] + dropdown::MAX_ROWS).min(count - 1);
        }
        KeyCode::Up | KeyCode::Down | KeyCode::PageUp | KeyCode::PageDown if plain => {}
        KeyCode::Tab | KeyCode::Enter if plain => super::activate(ta, at, &query, command),
        KeyCode::Right
            if plain
                && menu
                && query.is_empty()
                && !(depth == 0
                    && super::menu(Level::Commands, "", command).get(command.cursor[0]) == Some(&BRANCH)) =>
        {
            super::activate(ta, at, &query, command);
        }
        KeyCode::Esc if plain => super::back(ta, at, &query, command),
        // a filter char after a preview: the typed query again, the char after it
        KeyCode::Char(character) if plain && !character.is_whitespace() && command.preview.revert(ta) => {
            return false;
        }
        KeyCode::Left | KeyCode::Backspace if plain && depth > 0 && query.is_empty() => {
            super::back(ta, at, &query, command);
        }
        _ => return false,
    }

    true
}
