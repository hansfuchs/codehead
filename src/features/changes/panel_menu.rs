use super::{KeyCode, KeyEvent, KeyModifiers, PRow, View};
use crate::ui::context_menu::{Act as MenuAction, Item, key};

impl View {
    /// The context menu of the panel's row under the cursor.
    pub(in crate::features::changes) fn panel_ctx_items(
        &mut self,
    ) -> (String, Vec<crate::ui::context_menu::Item<crate::ui::context_menu::Act>>) {
        panel_ctx_items_view(self)
    }
}

fn panel_ctx_items_stash(
    enter: crate::ui::context_menu::Act,
    value: &mut Vec<crate::ui::context_menu::Item<crate::ui::context_menu::Act>>,
    si: usize,
    view: &mut View,
) -> String {
    value.push(Item::new("restore (its agent)", "enter", enter));
    value.push(Item::new("remove", "d d", key('d')));
    value.push(Item::new(
        "clear the stash (asks first)",
        "",
        MenuAction::Cmd("clear stash"),
    ));
    view.stash_state
        .stash
        .get(si)
        .map_or_else(String::new, crate::features::changes::stash::Entry::title)
}

fn panel_ctx_items_commit(
    enter: &crate::ui::context_menu::Act,
    value: &mut Vec<crate::ui::context_menu::Item<crate::ui::context_menu::Act>>,
    ci: usize,
    view: &mut View,
) -> String {
    let character_value = view.changes.commits.list.get(ci);

    value.push(Item::new("its files in the review", "enter", *enter));

    if character_value.is_some_and(|character_2| !character_2.pushed) {
        value.push(Item::new("pick", "space", key(' ')));
        value.push(Item::new("uncommit (asks first)", "", MenuAction::Cmd("uncommit")));
    }

    character_value.map_or_else(String::new, |character_3| character_3.short.clone())
}

fn panel_ctx_items_comment(
    enter: &crate::ui::context_menu::Act,
    value: &mut Vec<crate::ui::context_menu::Item<crate::ui::context_menu::Act>>,
    ci: usize,
    view: &mut View,
) -> String {
    let open = view
        .comment_state
        .comments
        .get(ci)
        .is_some_and(crate::features::changes::Comment::open);

    value.push(Item::new("edit at its line", "enter", *enter));
    value.push(Item::new(if open { "resolve" } else { "open again" }, "v", key('v')));
    value.push(Item::new("pick for the next prompt", "space", key(' ')));
    value.push(Item::new("open in the IDE", "e", key('e')));
    value.push(Item::new("delete", "d d", key('d')));

    view.comment_state
        .comments
        .get(ci)
        .map_or_else(String::new, |character| character.id.clone())
}

fn panel_ctx_items_file(
    enter: &crate::ui::context_menu::Act,
    value: &mut Vec<crate::ui::context_menu::Item<crate::ui::context_menu::Act>>,
    i: usize,
    view: &mut View,
) -> String {
    value.push(Item::new("jump to it in the review", "enter", *enter));
    value.push(Item::new("open in the IDE", "e", key('e')));
    value.push(Item::new("pick", "space", key(' ')));
    value.push(Item::new("stage the picked files (staged: unstage)", "s", key('s')));
    value.push(Item::new("stash the picked files", "p", key('p')));

    view.changes
        .files
        .get(i)
        .map_or_else(String::new, |file| file.path.clone())
}

fn panel_ctx_items_view(view: &mut View) -> (String, Vec<crate::ui::context_menu::Item<crate::ui::context_menu::Act>>) {
    view.sync_rows();
    let enter = MenuAction::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    let row = view.scm.rows.get(view.scm.at).cloned();
    let mut value = Vec::new();

    let title = match row {
        Some(PRow::File(i, _)) => panel_ctx_items_file(&enter, &mut value, i, view),
        Some(PRow::Comment(ci)) => panel_ctx_items_comment(&enter, &mut value, ci, view),
        Some(PRow::Commit(ci)) => panel_ctx_items_commit(&enter, &mut value, ci, view),
        Some(PRow::Stash(si)) => panel_ctx_items_stash(enter, &mut value, si, view),
        _ => "panel".to_string(),
    };

    value.extend([
        Item::new("commit message", "m", key('m')),
        Item::new("read git again", "r", key('r')),
    ]);

    // the ship steps valid now (a stopped rebase: only its steps), as the ship menu had them
    for (i, row_value) in view.ship_rows().into_iter().enumerate() {
        let mut it = Item::new(row_value.label, "", MenuAction::Step(i)).in_section(match row_value.group {
            crate::features::changes::ShipGroup::Ship => "ship/ship",
            crate::features::changes::ShipGroup::Git => "ship/git",
            crate::features::changes::ShipGroup::Pr => "ship/pr",
            crate::features::changes::ShipGroup::Rebase => "ship/rebase",
        });

        it.state = row_value.state;
        it.agent = row_value.agent;
        it.off = row_value.off;
        it.asks = row_value.asks;
        value.push(it);
    }

    (title, value)
}
