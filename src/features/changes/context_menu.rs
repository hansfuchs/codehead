use super::{Act, KeyCode, KeyEvent, KeyModifiers, Modal, Rect, Then, View, pr_label};
use crate::ui::context_menu::{Act as MenuAction, Item, key};

impl View {
    /// The context menu (ctrl+.): the actions of the row under the cursor (review or panel).
    pub(crate) fn ctx_items(&mut self) -> (String, Vec<crate::ui::context_menu::Item<crate::ui::context_menu::Act>>) {
        ctx_items_view(self)
    }

    /// The pill on top of the context menu: the open PR (the panel's ship section).
    pub(crate) fn ctx_head(&self) -> Option<String> {
        self.scm
            .focus
            .then(|| {
                self.pull_request
                    .pr
                    .as_ref()
                    .filter(|_| self.changes.stopped.is_none())
                    .map(pr_label)
            })
            .flatten()
    }

    /// Where the context menu opens: right of the panel at its cursor row (as the ship menu
    /// did), else above / below the review's cursor row.
    pub(crate) fn ctx_place(&self) -> crate::ui::context_menu::Place {
        ctx_place_view(self)
    }

    /// A context menu command with 0 key of its own.
    pub(crate) fn ctx_cmd(&mut self, character: &str) -> Act {
        ctx_cmd_view(self, character)
    }
}

fn comment_context_items(comment: &super::Comment, reply: bool, items: &mut Vec<Item<MenuAction>>) -> String {
    items.push(Item::new(
        "edit",
        "enter",
        MenuAction::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
    ));
    items.push(Item::new("reply", "c", key('c')));
    items.push(Item::new(
        if comment.open() { "resolve" } else { "open again" },
        "v",
        key('v'),
    ));
    items.push(Item::new("pick for the next prompt", "space", key(' ')));
    items.push(Item::new("1 row / whole text", "C", key('C')));
    items.push(Item::new(
        if reply { "delete the reply" } else { "delete" },
        "d d",
        key('d'),
    ));
    items.push(Item::new("open in the IDE", "e", key('e')));

    comment.id.clone()
}

fn file_context_items(view: &View, items: &mut Vec<Item<MenuAction>>) -> String {
    let lines = !view.selection_state.sel.is_empty();

    items.push(Item::new("open in the IDE", "e", key('e')));
    items.push(Item::new(
        if lines {
            "comment on the selected lines"
        } else {
            "comment on the line"
        },
        "c",
        key('c'),
    ));

    if lines {
        items.push(Item::new("stage the selected lines", "s", key('s')));
        items.push(Item::new("stash the selected lines", "p", key('p')));
    }

    items.push(Item::new("stage the file / hunk", "a", key('a')));
    items.push(Item::new("unstage the file / hunk", "u", key('u')));
    items.push(Item::new("reviewed on / off", "v", key('v')));

    view.file_at()
        .map_or_else(|| "changes".to_string(), |file| file.path.clone())
}

fn row_context_items(row: Option<&super::Row>, items: &mut Vec<Item<MenuAction>>, view: &View) -> String {
    if row.is_some_and(|row| row.finding.is_some()) {
        items.push(Item::new("into the comment batch", "c", key('c')));
        items.push(Item::new("dismiss", "d d", key('d')));
        return "finding".to_string();
    }

    if let Some((index, reply)) = view.row_comment(view.at) {
        return comment_context_items(&view.comment_state.comments[index], reply.is_some(), items);
    }

    file_context_items(view, items)
}

fn ctx_items_view(view: &mut View) -> (String, Vec<crate::ui::context_menu::Item<crate::ui::context_menu::Act>>) {
    if view.scm.focus {
        return view.panel_ctx_items();
    }

    let row = view.layout.rows.get(view.at);
    let mut value = Vec::new();
    let title = row_context_items(row, &mut value, view);

    value.extend([
        Item::new(
            "send the comment batch",
            "ctrl+enter",
            MenuAction::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL)),
        ),
        Item::new("the reviewer reads the diff", "V", key('V')),
        Item::new("classify again", "", MenuAction::Cmd("classify")),
        Item::new("side by side / unified", "w", key('w')),
        Item::new("read git again", "r", key('r')),
    ]);

    (title, value)
}

fn ctx_place_view(view: &View) -> crate::ui::context_menu::Place {
    use crate::ui::context_menu::Place;

    if view.scm.focus && view.scm.rect.width > 0 {
        let y = view
            .scm
            .lines
            .iter()
            .find(|line| line.1 == view.scm.at)
            .map_or(view.scm.rect.y + 1, |line| line.0);

        return Place::Beside(view.scm.rect, y);
    }

    let y =
        view.layout.area.y + (view.at.saturating_sub(view.top) as u16).min(view.layout.area.height.saturating_sub(1));

    Place::At(Rect {
        y,
        height: 1,
        ..view.layout.area
    })
}

fn ctx_cmd_view(view: &mut View, character: &str) -> Act {
    match character {
        "classify" => view.classify_again(),
        "uncommit" => view.ask_uncommit(),
        "clear stash" if !view.stash_state.stash.is_empty() => {
            view.modal_state.modal = Some(Modal::Confirm {
                question: format!(
                    "clear the stash? {} entries, their lines gone for good",
                    view.stash_state.stash.len()
                ),
                yes: false,
                then: Then::ClearStash,
            });
            Act::None
        }
        _ => Act::None,
    }
}
