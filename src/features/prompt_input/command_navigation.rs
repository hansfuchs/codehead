use super::{
    Action, BRANCH, COMMANDS, Command, ControlFlow, FLOW, HANDOFF, KINDS, Kind, Level, Pos, RELOAD, SUB, TextArea,
    UnicodeWidthStr,
};

pub(super) fn reset(command: &mut Command) {
    command.at = None;
    command.path.clear();
    command.cursor = [0; 3];
    command.offset = [0; 3];
    command.seen = None;
    command.items.clear();
    command.matched = (String::new(), Vec::new());
    command.loading = false;
    command.source = None;
}

/// Index of the open level (0: commands).
pub(super) fn depth(command: &Command) -> usize {
    command.path.len()
}

pub(super) fn level(depth: usize, command: &Command) -> Level {
    if depth == 0 {
        Level::Commands
    } else {
        command.path[depth - 1]
    }
}

/// Rows of menu LEVEL that match Q (prefix first, then names that hold Q): indices into
/// COMMANDS / KINDS. `branch`: only with a thread.
pub(super) fn menu(level: Level, query: &str, command: &Command) -> Vec<usize> {
    let names: Vec<(usize, &str)> = match level {
        Level::Commands => COMMANDS
            .iter()
            .enumerate()
            .filter(|(i, _)| !matches!(*i, BRANCH | HANDOFF | RELOAD | FLOW | SUB) || command.can_branch)
            .map(|(i, row)| (i, row.0))
            .collect(),
        _ => KINDS.iter().enumerate().map(|(i, row)| (i, row.1)).collect(),
    };

    let lower = query.to_lowercase();
    let mut rows: Vec<usize> = names
        .iter()
        .filter(|(_, name)| name.starts_with(&lower))
        .map(|(i, _)| *i)
        .collect();

    rows.extend(
        names
            .iter()
            .filter(|(_, name)| !name.starts_with(&lower) && crate::domain::notes::fuzzy(&lower, name).is_some())
            .map(|(i, _)| *i),
    );

    rows
}

pub(super) fn set_items(command: &mut Command) {
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

    command.matched = (String::new(), (0..command.items.len()).collect());
    command.seen = None;
}

/// 1 level deeper, the command's action, or the item into the text.
pub(super) fn activate(ta: &mut TextArea, at: Pos, query: &str, command: &mut Command) {
    let depth = self::depth(command);

    match self::level(depth, command) {
        Level::Commands => {
            if let ControlFlow::Break(()) = activate_command(ta, at, query, command) {
                return;
            }
        }
        Level::Kinds => {
            let Some(&i) = self::menu(Level::Kinds, query, command).get(command.cursor[1]) else {
                return;
            };

            command.cursor[1] = i;
            command.path.push(Level::Items(KINDS[i].0));
            command.load(KINDS[i].0);
        }
        Level::Items(_) => {
            let Some(item) = command
                .matched
                .1
                .get(command.cursor[depth])
                .and_then(|&i| command.items.get(i))
            else {
                return;
            };

            ta.replace_to_cursor(at, &format!("{} ", item.insert));
            self::reset(command);
            return;
        }
    }

    command.seen = None;

    if !query.is_empty() {
        ta.replace_to_cursor(at, &crate::infra::config::command_key().to_string());
    }
}

pub(super) fn activate_command(ta: &mut TextArea, at: Pos, query: &str, command: &mut Command) -> ControlFlow<()> {
    let Some(&i) = self::menu(Level::Commands, query, command).get(command.cursor[0]) else {
        return ControlFlow::Break(());
    };

    command.cursor[0] = i;

    match i {
        super::BRANCH => {
            // the `>text` goes, the host branches the thread
            ta.replace_to_cursor(at, "");
            self::reset(command);
            command.action = Some(Action::Branch);
            return ControlFlow::Break(());
        }
        super::HANDOFF => {
            ta.replace_to_cursor(at, "");
            self::reset(command);
            command.action = Some(Action::Handoff);
            return ControlFlow::Break(());
        }
        super::RELOAD => {
            ta.replace_to_cursor(at, "");
            self::reset(command);
            command.action = Some(Action::Reload);
            return ControlFlow::Break(());
        }
        super::FLOW => {
            ta.replace_to_cursor(at, "");
            self::reset(command);
            command.action = Some(Action::Flow);
            return ControlFlow::Break(());
        }
        super::SUB => {
            // `>sub ` stays in the text: the task follows (@agent, #shortcut, any
            // text); the send starts the sub-thread
            ta.replace_to_cursor(at, ">sub ");
            self::reset(command);
            return ControlFlow::Break(());
        }
        super::SKILLS => {
            command.path.push(Level::Items(Kind::Skills));
            command.load(Kind::Skills);
        }
        _ => command.path.push(Level::Kinds),
    }

    ControlFlow::Continue(())
}

/// 1 level back; on the 1st: closed for this `>`.
pub(super) fn back(ta: &mut TextArea, at: Pos, query: &str, command: &mut Command) {
    if command.path.pop().is_none() {
        command.closed = Some(at);
        self::reset(command);
        return;
    }

    // the parent keeps its row; `seen` holds the empty filter the parent comes back with
    command.seen = Some((self::depth(command), String::new()));

    if !query.is_empty() {
        ta.replace_to_cursor(at, &crate::infra::config::command_key().to_string());
    }
}
