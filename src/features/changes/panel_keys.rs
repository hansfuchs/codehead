use super::{Act, Fold, KeyCode, KeyEvent, Modal, PRow, Sec, Then, View};

impl View {
    /// A key while the panel has the focus.
    pub(in crate::features::changes) fn panel_key(&mut self, key: KeyEvent) -> Act {
        if self.scm.msg_focus {
            return self.msg_key(key);
        }

        // PR mode: the commit dropdown's keys while it is open
        if let Some(act) = picker_key(key, self) {
            return act;
        }

        self.sync_rows();
        let last = self.scm.rows.len().saturating_sub(1);
        let row = self.scm.rows.get(self.scm.at).cloned();

        if panel_navigation_key(key, last, row.as_ref(), self) {
            return Act::None;
        }

        panel_action_key(key, last, row, self)
    }
}

fn picker_key(key: KeyEvent, view: &mut View) -> Option<Act> {
    if let Some(cur) = view.scm.pick {
        let count = view.pr_src.as_ref().map_or(0, |source| source.commits.len()) + 1;

        match key.code {
            KeyCode::Up | KeyCode::Char('k') => view.scm.pick = Some(cur.saturating_sub(1)),
            KeyCode::Down | KeyCode::Char('j') => view.scm.pick = Some((cur + 1).min(count - 1)),
            KeyCode::Home => view.scm.pick = Some(0),
            KeyCode::End => view.scm.pick = Some(count - 1),
            KeyCode::Enter | KeyCode::Right => view.pick(cur),
            KeyCode::Esc | KeyCode::Left => view.scm.pick = None,
            _ => {}
        }

        return Some(Act::None);
    }

    None
}

fn open_panel_editor_key(row: Option<&PRow>, view: &mut View) -> Option<Act> {
    match row {
        Some(PRow::File(i, _)) => {
            if let Some(file) = view.changes.files.get(*i) {
                return Some(Act::Open(file.path.clone(), file.first_line()));
            }
        }
        Some(PRow::Comment(ci)) => {
            if let Some(comment) = view.comment_state.comments.get(*ci) {
                return Some(Act::Open(comment.path.clone(), comment.line));
            }
        }
        _ => {}
    }

    None
}

fn delete_panel_row_key(row: Option<&PRow>, view: &mut View) -> Option<Act> {
    match row {
        Some(PRow::Comment(ci)) => {
            view.comment_state.comments.remove(*ci);
            view.save_comments();
            view.rebuild_here();
            return Some(Act::Toast(format!(
                "comment deleted · {} left",
                view.comment_state.comments.len()
            )));
        }
        Some(PRow::Stash(si)) => {
            if let Some(id) = view.stash_state.stash.get(*si).map(|entry| entry.id.clone()) {
                view.modal_state.modal = Some(Modal::Confirm {
                    question: String::new(),
                    yes: true,
                    then: Then::DropStash(id),
                });
                return Some(view.confirm(true));
            }
        }
        _ => {}
    }

    None
}

fn resolve_panel_comment_key(row: Option<&PRow>, view: &mut View) -> Option<Act> {
    if let Some(PRow::Comment(ci)) = row {
        let act = view.toggle_resolved(*ci);

        view.scm.rows = view.panel_rows();
        return Some(act);
    }

    None
}

fn select_panel_row_key(last: usize, row: Option<&PRow>, view: &mut View) -> Option<Act> {
    if let Some(PRow::File(i, _)) = row
        && let Some(path) = view.changes.files.get(*i).map(|file| file.path.clone())
        && !view.scm.picked.remove(&path)
    {
        view.scm.picked.insert(path);
    }

    // a comment: picked for the next prompt
    if let Some(PRow::Comment(ci)) = row
        && let Some(id) = view.comment_state.comments.get(*ci).map(|comment| comment.id.clone())
        && !view.scm.comment_picks.remove(&id)
    {
        view.scm.comment_picks.insert(id);
    }

    // a commit: unpushed ones only (a pushed one stays as it is)
    if let Some(PRow::Commit(i)) = row
        && let Some(commit) = view.changes.commits.list.get(*i)
    {
        if commit.pushed {
            return Some(Act::Toast(format!("{}: pushed, read only", commit.short)));
        }

        if !view.scm.commit_picks.remove(&commit.oid) {
            view.scm.commit_picks.insert(commit.oid.clone());
        }
    }

    view.scm.at = (view.scm.at + 1).min(last);
    None
}

fn open_panel_row_key(row: Option<PRow>, view: &mut View) -> Option<Act> {
    match row {
        Some(group) if group.label().is_some() => {
            view.toggle_fold(group.label().unwrap_or(Fold::Sec(Sec::Staged)), None);
        }
        Some(PRow::File(i, _)) => {
            view.jump_file(i);
            view.scm.focus = false;
        }
        // enter on a comment: its whole thread at its line, the keys there
        Some(PRow::Comment(ci)) => {
            if let Some(id) = view.comment_state.comments.get(ci).map(|comment| comment.id.clone()) {
                view.unfold_comment(&id);
            }

            view.jump_comment(ci);
            view.scm.focus = false;
        }
        // enter on a commit: its files in the review, the keys there
        Some(PRow::Commit(ci)) => view.open_commit(ci, false),
        // enter on a commit's file: the review on its box, the keys there
        Some(PRow::CommitFile(ci, fi)) => {
            view.open_commit_file(ci, fi);
            view.scm.focus = false;
        }
        Some(PRow::All) => view.open_all(false),
        // enter on another checkout's comments: the view goes there
        Some(PRow::Elsewhere(root, _)) => return Some(Act::Checkout(root)),
        // enter on a stash entry: its agent restores it
        Some(PRow::Stash(si)) => {
            if let Some(id) = view.stash_state.stash.get(si).map(|entry| entry.id.clone()) {
                return Some(view.restore(&id));
            }
        }
        _ => {}
    }

    None
}

fn focus_review_key(row: Option<&PRow>, view: &mut View) {
    match row {
        Some(PRow::File(i, _)) => view.jump_file(*i),
        Some(PRow::Comment(ci)) => view.jump_comment(*ci),
        Some(PRow::Commit(ci)) => view.open_commit(*ci, false),
        Some(PRow::CommitFile(ci, fi)) => view.open_commit_file(*ci, *fi),
        Some(PRow::All) => view.open_all(false),
        Some(PRow::Stash(si)) => {
            if let Some(i) = view
                .stash_state
                .stash
                .get(*si)
                .and_then(|entry| view.changes.files.iter().position(|file| file.path == entry.path))
            {
                view.jump_file(i);
            }
        }
        _ => {}
    }

    view.scm.focus = false;
}

fn fold_panel_row_key(row: Option<&PRow>, view: &mut View) {
    match row.and_then(PRow::label) {
        Some(fold) => view.toggle_fold(fold, Some(true)),
        // ← on the shown commit: the changes back, its files hide
        None if matches!(row, Some(PRow::Commit(ci)) if view.changes.commits.list.get(*ci).is_some_and(|commit| view.peeked() == Some(commit.oid.as_str()))) =>
        {
            view.peek = None;
            view.sync_rows();
        }
        None => {
            if let Some(parent) = view.parent(view.scm.at) {
                view.scm.at = parent;
            }
        }
    }
}

fn panel_navigation_key(key: KeyEvent, last: usize, row: Option<&PRow>, view: &mut View) -> bool {
    match key.code {
        KeyCode::Up | KeyCode::PageUp if view.scm.at == 0 && view.pr_src.is_none() && key.modifiers.is_empty() => {
            view.scm.msg_focus = true;
        }
        KeyCode::Up | KeyCode::Char('k') => view.move_to(view.scm.at.saturating_sub(1), false),
        KeyCode::Down | KeyCode::Char('j') => view.move_to((view.scm.at + 1).min(last), true),
        KeyCode::Home | KeyCode::Char('g') | KeyCode::PageUp => view.move_to(0, true),
        KeyCode::End | KeyCode::Char('G') | KeyCode::PageDown => view.move_to(last, false),
        KeyCode::Left => fold_panel_row_key(row, view),
        KeyCode::Enter | KeyCode::Right if matches!(row, Some(PRow::Pick)) => {
            view.scm.pick = Some(view.pick_index());
        }
        KeyCode::Right => focus_review_key(row, view),
        _ => return false,
    }

    true
}

fn panel_action_key(key: KeyEvent, last: usize, row: Option<PRow>, view: &mut View) -> Act {
    match key.code {
        KeyCode::Enter => {
            if let Some(act) = open_panel_row_key(row, view) {
                return act;
            }
        }
        KeyCode::Char(' ') if group_row(row.as_ref()) => {
            if let Some(group) = row {
                return view.pick_group(&group);
            }
        }
        KeyCode::Char(' ') => {
            if let Some(act) = select_panel_row_key(last, row.as_ref(), view) {
                return act;
            }
        }
        KeyCode::Char('[') => view.jump_section(false),
        KeyCode::Char(']') => view.jump_section(true),
        KeyCode::Char('P') => return view.ask_push_all(),
        KeyCode::Char('v') => {
            if let Some(act) = resolve_panel_comment_key(row.as_ref(), view) {
                return act;
            }
        }
        KeyCode::Char('s') => return view.panel_stage(),
        KeyCode::Char('p') => return view.panel_stash(),
        KeyCode::Char('r') => {
            view.refresh(true);
            return Act::Toast("changes: reading git".into());
        }
        KeyCode::Char('d') | KeyCode::Delete
            if matches!(row, Some(PRow::Comment(_) | PRow::Stash(_))) && !view.del_armed(true, view.scm.at) =>
        {
            return Act::Toast("d again: delete".into());
        }
        KeyCode::Char('d') | KeyCode::Delete => {
            if let Some(act) = delete_panel_row_key(row.as_ref(), view) {
                return act;
            }
        }
        KeyCode::Char('m') => view.scm.msg_focus = true,
        KeyCode::Char('e') => {
            if let Some(act) = open_panel_editor_key(row.as_ref(), view) {
                return act;
            }
        }
        KeyCode::Esc => {
            view.scm.picked.clear();
            view.scm.commit_picks.clear();
            view.scm.comment_picks.clear();
            view.layout.built = None;
        }
        _ => {}
    }

    Act::None
}

fn group_row(row: Option<&PRow>) -> bool {
    row.is_some_and(|row| row.label().is_some() || matches!(row, PRow::CommentFile(..)))
}
