use super::{Command, Item, UnicodeWidthStr, matches};

impl Command {
    pub(super) fn update_items(&mut self, items: Vec<Item>) {
        update_items_command(self, items);
    }
}

fn update_items_command(command: &mut Command, items: Vec<Item>) {
    if command.items.is_empty() {
        command.items = items;
        super::set_items(command);
        return;
    }

    let depth = super::depth(command);
    let keep = command
        .matched
        .1
        .get(command.cursor[depth])
        .and_then(|&i| command.items.get(i))
        .map(|item| item.insert.clone());
    let query = command.matched.0.clone();

    command.items = items;

    command.items_w = command
        .items
        .iter()
        .map(|item| {
            item.label.width()
                + if item.detail.is_empty() {
                    0
                } else {
                    item.detail.width() + 2
                }
        })
        .max()
        .unwrap_or(0);

    let mut matched = matches(&command.items, &query);

    if Command::from_top(super::level(depth, command)) && !query.is_empty() {
        matched.reverse();
    }

    command.cursor[depth] = keep
        .and_then(|insert| matched.iter().position(|&i| command.items[i].insert == insert))
        .unwrap_or_else(|| command.cursor[depth].min(matched.len().saturating_sub(1)));
    command.matched = (query, matched);
}
