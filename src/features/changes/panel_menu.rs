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
    items: &mut Vec<crate::ui::context_menu::Item<crate::ui::context_menu::Act>>,
    si: usize,
    view: &mut View,
) -> String {
    items.push(Item::new("restore (its agent)", "enter", enter));
    items.push(Item::new("remove", "d d", key('d')));
    items.push(Item::new(
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
    items: &mut Vec<crate::ui::context_menu::Item<crate::ui::context_menu::Act>>,
    ci: usize,
    view: &mut View,
) -> String {
    let commit = view.changes.commits.list.get(ci);

    items.push(Item::new("its files in the review", "enter", *enter));

    if commit.is_some_and(|commit| !commit.pushed) {
        items.push(Item::new("pick", "space", key(' ')));
        items.push(Item::new("uncommit (asks first)", "", MenuAction::Cmd("uncommit")));
    }

    commit.map_or_else(String::new, |commit| commit.short.clone())
}

fn panel_ctx_items_comment(
    enter: &crate::ui::context_menu::Act,
    items: &mut Vec<crate::ui::context_menu::Item<crate::ui::context_menu::Act>>,
    ci: usize,
    view: &mut View,
) -> String {
    let open = view
        .comment_state
        .comments
        .get(ci)
        .is_some_and(crate::features::changes::Comment::open);

    items.push(Item::new("edit at its line", "enter", *enter));
    items.push(Item::new(if open { "resolve" } else { "open again" }, "v", key('v')));
    items.push(Item::new("pick for the next prompt", "space", key(' ')));
    items.push(Item::new("open in the IDE", "e", key('e')));
    items.push(Item::new("delete", "d d", key('d')));

    view.comment_state
        .comments
        .get(ci)
        .map_or_else(String::new, |comment| comment.id.clone())
}

fn panel_ctx_items_file(
    enter: &crate::ui::context_menu::Act,
    items: &mut Vec<crate::ui::context_menu::Item<crate::ui::context_menu::Act>>,
    i: usize,
    view: &mut View,
) -> String {
    items.push(Item::new("jump to it in the review", "enter", *enter));
    items.push(Item::new("open in the IDE", "e", key('e')));
    items.push(Item::new("pick", "space", key(' ')));
    items.push(Item::new("stage the picked files (staged: unstage)", "s", key('s')));
    items.push(Item::new("stash the picked files", "p", key('p')));

    view.changes
        .files
        .get(i)
        .map_or_else(String::new, |file| file.path.clone())
}

fn panel_ctx_items_view(view: &mut View) -> (String, Vec<crate::ui::context_menu::Item<crate::ui::context_menu::Act>>) {
    view.sync_rows();
    let enter = MenuAction::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    let row = view.scm.rows.get(view.scm.at).cloned();
    let mut items = Vec::new();

    let title = match row {
        Some(PRow::File(i, _)) => panel_ctx_items_file(&enter, &mut items, i, view),
        Some(PRow::Comment(ci)) => panel_ctx_items_comment(&enter, &mut items, ci, view),
        Some(PRow::Commit(ci)) => panel_ctx_items_commit(&enter, &mut items, ci, view),
        Some(PRow::Stash(si)) => panel_ctx_items_stash(enter, &mut items, si, view),
        _ => "panel".to_string(),
    };

    items.extend([
        Item::new("commit message", "m", key('m')),
        Item::new("read git again", "r", key('r')),
    ]);

    // the ship steps valid now (a stopped rebase: only its steps), as the ship menu had them
    for (i, step) in view.ship_rows().into_iter().enumerate() {
        let mut it = Item::new(step.label, "", MenuAction::Step(i)).in_section(match step.group {
            crate::features::changes::ShipGroup::Ship => "ship/ship",
            crate::features::changes::ShipGroup::Git => "ship/git",
            crate::features::changes::ShipGroup::Pr => "ship/pr",
            crate::features::changes::ShipGroup::Rebase => "ship/rebase",
        });

        it.state = step.state;
        it.agent = step.agent;
        it.off = step.off;
        it.asks = step.asks;
        items.push(it);
    }

    (title, items)
}
