use crate::features::changes::tests_support::*;

use super::*;

/// Tab in the thread view: the keys to the panel / the review (its focus ring).
fn alt_tap(view: &mut crate::features::changes::View) {
    let on = !view.scm.focus;

    view.focus_panel(on);
}

/// A view of fixture repo W (read now), the panel with the keys.
fn commit_view(width: &std::path::Path, id: &str) -> View {
    let mut view = View::new(id, width);

    view.marks.clear();
    view.comment_state.comments.clear();
    view.stash_state.stash.clear();
    view.changes = crate::features::changes::load(width).unwrap();
    view.loaded = Some(std::time::Instant::now());
    view.scm.focus = true;

    view
}

fn press(view: &mut View, character: KeyCode) -> Act {
    view.key(KeyEvent::new(character, KeyModifiers::NONE))
}

/// The panel's cursor on the commit with SUBJECT.
fn on_commit(view: &mut View, subject: &str) {
    draw(view, 140, 40);
    view.scm.at = view
        .scm
        .rows
        .iter()
        .position(|result| matches!(result, PRow::Commit(i) if view.changes.commits.list[*i].subject == subject))
        .unwrap();
}

/// The git step on its way ends: its note.
fn wait_op(view: &mut View) -> String {
    for _ in 0..300 {
        if view.poll_op() {
            return view.take_note().unwrap_or_default();
        }

        std::thread::sleep(std::time::Duration::from_millis(20));
    }

    panic!("git step: 0 end");
}

/// The read on its way ends.
fn wait_read(view: &mut View) {
    for _ in 0..300 {
        if view.loading.is_none() || view.poll() {
            return;
        }

        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

#[path = "changes_panel_navigation_tests.rs"]
mod navigation;

#[path = "changes_panel_ship_tests.rs"]
mod ship;

#[path = "changes_panel_commits_tests.rs"]
mod commits;

#[path = "changes_panel_push_tests.rs"]
mod push;

#[path = "changes_panel_comments_tests.rs"]
mod comments;
