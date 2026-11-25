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

    let directory = super::depth(command);
    let keep = command
        .matched
        .1
        .get(command.cursor[directory])
        .and_then(|&i| command.items.get(i))
        .map(|i| i.insert.clone());
    let query = command.matched.0.clone();

    command.items = items;
    command.items_w = command
        .items
        .iter()
        .map(|i| i.label.width() + if i.detail.is_empty() { 0 } else { i.detail.width() + 2 })
        .max()
        .unwrap_or(0);
    let mut matched = matches(&command.items, &query);

    if Command::from_top(super::level(directory, command)) && !query.is_empty() {
        matched.reverse();
    }

    command.cursor[directory] = keep
        .and_then(|text| matched.iter().position(|&i| command.items[i].insert == text))
        .unwrap_or_else(|| command.cursor[directory].min(matched.len().saturating_sub(1)));
    command.matched = (query, matched);
}
