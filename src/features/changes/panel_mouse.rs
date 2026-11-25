use super::{Act, Btn, KeyModifiers, Modal, MouseButton, MouseEvent, MouseEventKind, PRow, Position, Rect, Then, View};

impl View {
    /// A mouse event over the panel.
    pub(in crate::features::changes) fn panel_mouse(&mut self, event: MouseEvent) -> Act {
        panel_mouse_view(self, event)
    }
}

fn confirm_clear_stash(view: &mut View) {
    view.modal_state.modal = Some(Modal::Confirm {
        question: format!(
            "clear the stash? {} entries, their lines gone for good",
            view.stash_state.stash.len()
        ),
        yes: false,
        then: Then::ClearStash,
    });
}

fn confirm_drop_stash(view: &mut View, id: String) {
    let title = view
        .stash_state
        .stash
        .iter()
        .find(|entry| entry.id == id)
        .map(crate::features::changes::stash::Entry::title)
        .unwrap_or_default();

    view.modal_state.modal = Some(Modal::Confirm {
        question: format!("remove “{title}” from the stash? its lines are gone for good"),
        yes: false,
        then: Then::DropStash(id),
    });
}

fn panel_mouse_commit(view: &mut View, i: usize, shift: bool, ci: usize) {
    view.scm.at = i;

    if shift && let Some(commit) = view.changes.commits.list.get(ci).filter(|commit| !commit.pushed) {
        let oid = commit.oid.clone();

        if !view.scm.commit_picks.remove(&oid) {
            view.scm.commit_picks.insert(oid);
        }
    } else {
        view.open_commit(ci, true);
    }
}

fn panel_mouse_file(view: &mut View, i: usize, shift: bool, fi: usize) -> Option<Act> {
    if shift && let Some(anchor) = view.scm.anchor {
        select_panel_range(view, i, anchor);

        view.scm.at = i;
        return Some(Act::None);
    }

    view.scm.at = i;
    view.scm.anchor = Some(i);
    view.jump_file(fi);
    None
}

fn select_panel_range(view: &mut View, i: usize, anchor: usize) {
    for j in anchor.min(i)..=anchor.max(i) {
        if let Some(PRow::File(fi, _)) = view.scm.rows.get(j)
            && let Some(file) = view.changes.files.get(*fi)
        {
            view.scm.picked.insert(file.path.clone());
        }
    }
}

fn picker_mouse(view: &mut View, at: Position) -> Option<Act> {
    // PR mode: a click in the open commit dropdown picks, outside it closes it
    if view.scm.pick.is_some() {
        if let Some(&(_, pick)) = view.scm.pick_rows.iter().find(|(rect, _)| rect.contains(at)) {
            view.pick(pick);
        } else {
            view.scm.pick = None;
        }

        return Some(Act::None);
    }

    None
}

fn panel_button_mouse(at: Position, view: &mut View) -> Option<Act> {
    if let Some((_, button)) = view.scm.buttons.iter().find(|(rect, _)| rect.contains(at)).cloned() {
        view.scm.msg_focus = false;

        match button {
            Btn::Restore(id) => return Some(view.restore(&id)),
            Btn::Drop(id) => confirm_drop_stash(view, id),
            Btn::Clear => confirm_clear_stash(view),
            Btn::Push => return Some(view.ask_push_all()),
        }

        return Some(Act::None);
    }

    None
}

fn panel_message_mouse(event: MouseEvent, at: Position, view: &mut View) -> Option<Act> {
    if (Rect {
        y: view.scm.msg_rect.y.saturating_sub(1),
        height: view.scm.msg_rect.height + 2,
        ..view.scm.rect
    })
    .contains(at)
    {
        view.scm.msg_focus = true;
        view.scm.msg.mouse(event);
        return Some(Act::None);
    }

    None
}

fn panel_mouse_kind(event: MouseEvent, at: Position, view: &mut View) -> Option<Act> {
    match event.kind {
        MouseEventKind::ScrollDown => {
            let count = view.scm.rows.len();

            view.scm.top = (view.scm.top + 3).min(count.saturating_sub(1));
            view.scm.at = view.scm.at.max(view.scm.top);
            return Some(Act::None);
        }
        MouseEventKind::ScrollUp => {
            view.scm.top = view.scm.top.saturating_sub(3);
            return Some(Act::None);
        }
        MouseEventKind::Down(MouseButton::Left) => {
            if let Some(act) = picker_mouse(view, at) {
                return Some(act);
            }
        }
        _ => {
            if view.scm.msg_rect.contains(at) || view.scm.msg_focus {
                view.scm.msg.mouse(event);
            }

            return Some(Act::None);
        }
    }

    None
}

fn panel_mouse_view(view: &mut View, event: MouseEvent) -> Act {
    let at = Position {
        x: event.column,
        y: event.row,
    };

    if let Some(act) = panel_mouse_kind(event, at, view) {
        return act;
    }

    view.scm.focus = true;

    if let Some(act) = panel_button_mouse(at, view) {
        return act;
    }

    if let Some(act) = panel_message_mouse(event, at, view) {
        return act;
    }

    view.scm.msg_focus = false;

    let Some(&(_, i, stuck)) = view.scm.lines.iter().find(|(y, ..)| *y == event.row) else {
        return Act::None;
    };

    let row = view.scm.rows.get(i).cloned();

    // a running push's rows: 0 clicks
    if matches!(row, Some(PRow::Pushing(_) | PRow::PushingCommit(_))) {
        return Act::None;
    }

    if let Some(fold) = row.as_ref().and_then(PRow::label) {
        panel_label_mouse(fold, i, stuck, view);

        return Act::None;
    }

    let shift = event.modifiers.contains(KeyModifiers::SHIFT);

    match row {
        Some(PRow::File(fi, _)) => {
            if let Some(act) = panel_mouse_file(view, i, shift, fi) {
                return act;
            }
        }
        Some(PRow::Comment(ci)) => {
            view.scm.at = i;
            view.jump_comment(ci);
        }
        Some(PRow::All) => {
            view.scm.at = i;
            view.open_all(true);
        }
        // a click on a commit: its files in the review (shift+click: pick / unpick it)
        Some(PRow::Commit(ci)) => panel_mouse_commit(view, i, shift, ci),
        // a click on a commit's file: the review on its box
        Some(PRow::CommitFile(ci, fi)) => {
            view.scm.at = i;
            view.open_commit_file(ci, fi);
        }
        Some(PRow::Pick) => {
            view.scm.at = i;
            view.scm.pick = Some(view.pick_index());
        }
        _ => view.scm.at = i,
    }

    Act::None
}

fn panel_label_mouse(fold: super::Fold, i: usize, stuck: bool, view: &mut View) {
    view.scm.at = i;

    if stuck {
        // scroll to it: right under the labels above it
        let before = (0..i).filter(|&j| view.scm.rows[j].label().is_some()).count();

        view.scm.top = i.saturating_sub(before);
    } else {
        view.toggle_fold(fold, None);
    }
}
