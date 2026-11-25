use super::{Act, Comment, KeyCode, KeyEvent, KeyModifiers, Op, Spot, Status, View, batch, read_only_key, threads};

impl View {
    pub(crate) fn key(&mut self, key: KeyEvent) -> Act {
        key_view(self, key)
    }
}

fn send_modifier(key: KeyEvent) -> bool {
    key.modifiers
        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER)
}

fn blocked_shortcut(key: KeyEvent, ctrl: bool) -> bool {
    let KeyCode::Char(character) = key.code else {
        return false;
    };

    let modified = key
        .modifiers
        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER);
    let refresh_or_delete =
        ctrl && !key.modifiers.intersects(KeyModifiers::ALT | KeyModifiers::SUPER) && matches!(character, 'r' | 'x');

    modified && !refresh_or_delete
}

fn key_overlay(view: &mut View, key: KeyEvent) -> Option<Act> {
    if view.modal_state.modal.is_some() {
        return Some(view.modal_key(key));
    }

    if view.filter_state.filtering {
        return Some(view.filter_key(key));
    }

    view.del_prev = view.del.take();

    if !view.scm.msg_focus && crate::ui::textarea::is_filter_key(&key) {
        view.filter_state.filtering = true;
        view.scm.focus = false;
        view.filter_state.filter_ta.move_end();
        return Some(Act::None);
    }

    None
}

fn key_gate(view: &mut View, key: KeyEvent, ctrl: bool) -> Option<Act> {
    if !view.scm.msg_focus && blocked_shortcut(key, ctrl) {
        return Some(Act::None);
    }

    if view.scm.focus {
        return Some(view.panel_key(key));
    }

    let plain = !key
        .modifiers
        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER);

    if key.code == KeyCode::Left && plain {
        view.toggle_focus();
        return Some(Act::None);
    }

    if let Some(peek) = view.peek.as_mut() {
        if key.code == KeyCode::Esc && !peek.takes_esc() {
            view.peek = None;
            return Some(Act::None);
        }

        return Some(peek.key(key));
    }

    if let Some(commit) = &view.commit_src
        && !read_only_key(key)
    {
        return Some(Act::Toast(format!("commit {}: read only · esc: back", commit.short)));
    }

    if view.all && matches!(key.code, KeyCode::Char('s' | 'p' | 'a' | 'u' | 'm' | 'S' | 'V' | 'R')) {
        return Some(Act::Toast(
            "all changes: comments only · git steps on the uncommitted changes (esc)".into(),
        ));
    }

    None
}

fn movement_key(view: &mut View, key: KeyEvent, ctrl: bool) -> Option<Act> {
    let last = view.layout.rows.len().saturating_sub(1);

    match key.code {
        KeyCode::Down if !ctrl => view.at = (view.at + 1).min(last),
        KeyCode::Up if !ctrl => view.at = view.at.saturating_sub(1),
        KeyCode::Char('j') | KeyCode::Down => view.jump(true, |row| row.head),
        KeyCode::Char('k') | KeyCode::Up => view.jump(false, |row| row.head),
        KeyCode::PageDown => view.at = (view.at + view.page()).min(last),
        KeyCode::PageUp => view.at = view.at.saturating_sub(view.page()),
        KeyCode::Char('n') => view.jump(true, |row| row.hunk || row.head),
        KeyCode::Char('N') => view.jump(false, |row| row.hunk || row.head),
        KeyCode::Char('g') | KeyCode::Home => view.at = 0,
        KeyCode::Char('G') | KeyCode::End => view.at = last,
        KeyCode::Esc => clear_selection_key(view),
        KeyCode::Char('w') => return Some(toggle_side_key(view)),
        KeyCode::Enter if !send_modifier(key) => return Some(open_row_key(view).unwrap_or(Act::None)),
        _ => return None,
    }

    Some(Act::None)
}

fn comment_key(view: &mut View, key: KeyEvent) -> Option<Act> {
    let comment = view.layout.rows.get(view.at).and_then(|row| row.comment);
    let finding = view.layout.rows.get(view.at).and_then(|row| row.finding);
    let last = view.layout.rows.len().saturating_sub(1);

    match key.code {
        KeyCode::Char(' ') if comment.is_some() => select_comment_key(view, last),
        KeyCode::Char(' ') => select_line_key(view, last),
        KeyCode::Char('v') if comment.is_some() => return comment.map(|index| view.toggle_resolved(index)),
        KeyCode::Char('C') if view.row_comment(view.at).is_some() => {
            return Some(fold_comment_key(view));
        }
        KeyCode::Char('C') => return Some(fold_comments_key(view)),
        KeyCode::Enter if send_modifier(key) => return Some(send_comments_key(view)),
        KeyCode::Char('c') => return Some(comment_at_cursor_key(view, finding)),
        KeyCode::Char('d') | KeyCode::Delete if comment.is_some() || finding.is_some() => {
            return Some(delete_comment_key(view).unwrap_or(Act::None));
        }
        _ => return None,
    }

    Some(Act::None)
}

fn comment_at_cursor_key(view: &mut View, finding: Option<usize>) -> Act {
    if view.selection_state.sel.is_empty() && finding.is_some() {
        return view.take_finding();
    }

    if view.selection_state.sel.is_empty()
        && let Some((index, _)) = view.row_comment(view.at)
    {
        view.reply_comment(index);
        return Act::None;
    }

    new_comment_key(view);
    Act::None
}

fn operation_key(view: &mut View, key: KeyEvent) -> Act {
    match key.code {
        KeyCode::Char('r') => {
            view.refresh(true);
            Act::Toast("changes: reading git".into())
        }
        KeyCode::Char('v') => view.toggle_reviewed(),
        KeyCode::Char('t') => Act::ToggleChat,
        KeyCode::Char('s' | 'p') => stage_selection_key(view, key),
        KeyCode::Char('a' | 'u') => stage_hunk_key(view, key).unwrap_or(Act::None),
        KeyCode::Char('m') => {
            view.focus_message();
            Act::None
        }
        KeyCode::Char('P') => view.ask_push_all(),
        KeyCode::Char('V') => view.start_review(),
        KeyCode::Char('e') => view.open_spot().map_or(Act::None, |(path, line)| Act::Open(path, line)),
        _ => Act::None,
    }
}

fn delete_comment_key(view: &mut View) -> Option<Act> {
    if !view.del_armed(false, view.at) {
        let what = if view
            .layout
            .rows
            .get(view.at)
            .is_some_and(|result| result.finding.is_some())
        {
            "dismiss the finding"
        } else {
            "delete the comment"
        };

        return Some(Act::Toast(format!("d again: {what}")));
    }

    if let Some((ci, Some(row))) = view.row_comment(view.at) {
        let id = view.comment_state.comments[ci].id.clone();

        if let Err(error) = view.write_through(|thread| threads::edit(thread, &id, Some(row), "")) {
            return Some(Act::Toast(error));
        }

        view.rebuild_on_comment(ci);
        return Some(Act::Toast(format!("{id}: reply deleted")));
    }

    if let Some(ci) = view.layout.rows.get(view.at).and_then(|result| result.comment) {
        view.comment_state.comments.remove(ci);
        view.save_comments();
        view.rebuild_here();
        return Some(Act::Toast(format!(
            "comment deleted · {} left",
            view.comment_state.comments.len()
        )));
    }

    // a reply: that reply goes
    if let Some(fi) = view.layout.rows.get(view.at).and_then(|result| result.finding) {
        view.drop_finding(fi);
        view.rebuild_here();
        return Some(Act::Toast("finding dismissed".into()));
    }

    None
}

fn new_comment_key(view: &mut View) {
    let spots = view.spots();

    if !spots.is_empty() {
        view.open_comment(spots);
    } else if let (Some(file), Some(row)) = (view.file_at(), view.layout.rows.get(view.at)) {
        let spot = Spot {
            path: file.path.clone(),
            line: row.at.unwrap_or_else(|| file.first_line()),
            lines: String::new(),
        };

        view.open_comment(vec![spot]);
    }
}

fn stage_hunk_key(view: &mut View, key: KeyEvent) -> Option<Act> {
    let stage = key.code == KeyCode::Char('a');

    if let (Some(file), Some(row)) = (view.file_at(), view.layout.rows.get(view.at)) {
        let (path, new) = (file.path.clone(), file.status == Status::New);
        let hunk = (!row.head && !new).then_some(row.at).flatten();

        let op = match (stage, hunk) {
            (true, Some(line)) => Op::StageHunk(path, line),
            (false, Some(line)) => Op::UnstageHunk(path, line),
            (true, None) => Op::Stage(path),
            (false, None) => Op::Unstage(path),
        };

        return Some(view.start_op(op));
    }

    None
}

fn stage_selection_key(view: &mut View, key: KeyEvent) -> Act {
    let picks = view.picks();

    if picks.is_empty() {
        let what = if view.selection_state.sel.is_empty() {
            "0 lines selected · space / click: select"
        } else {
            "0 changed lines selected (context only)"
        };

        return Act::Toast(what.into());
    }

    view.selection_state.sel.clear();

    if key.code == KeyCode::Char('p') {
        return view.stash_op(picks);
    }

    view.start_op(Op::StageLines(
        picks.into_iter().map(|(path, _, pk)| (path, pk)).collect(),
    ))
}

fn toggle_side_key(view: &mut View) -> Act {
    let now = view.side_by_side(view.layout.area.width.saturating_sub(1));

    view.side = Some(!now);
    view.rebuild_on_file();
    Act::Toast(if now { "unified".into() } else { "side by side".into() })
}

fn open_row_key(view: &mut View) -> Option<Act> {
    if view
        .layout
        .rows
        .get(view.at)
        .is_some_and(|result| result.comment.is_some())
    {
        return Some(view.open_comment_row(view.at, true));
    } else if let Some(file) = view.file_at() {
        let on = !view.folded(file);

        view.fold(on);
    }

    None
}

fn send_comments_key(view: &mut View) -> Act {
    // the open comments not sent yet; they stay (open) until an agent resolves them
    let new: Vec<&Comment> = view
        .comment_state
        .comments
        .iter()
        .filter(|character| character.open() && !character.sent)
        .collect();

    let Some(text) = batch(&new) else {
        return Act::Toast(format!(
            "0 new comments · {} open, sent already · c: comment on the line",
            view.comment_count()
        ));
    };

    for comment in view
        .comment_state
        .comments
        .iter_mut()
        .filter(|character| character.open())
    {
        comment.sent = true;
    }

    view.save_comments();
    view.layout.built = None;

    Act::Send(text)
}

fn clear_selection_key(view: &mut View) {
    if view.filter_state.filter.is_empty() {
        view.selection_state.sel.clear();
        view.scm.comment_picks.clear();
        view.layout.built = None;
    } else {
        view.set_filter("");
        view.layout.built = None;
    }
}

fn fold_comments_key(view: &mut View) -> Act {
    view.comment_state.comments_folded = !view.comment_state.comments_folded;
    view.comment_state.comment_flip.clear();
    view.rebuild_here();
    Act::Toast(if view.comment_state.comments_folded {
        "comments: 1 row each".into()
    } else {
        "comments: whole text".into()
    })
}

fn fold_comment_key(view: &mut View) -> Act {
    let Some((ci, _)) = view.row_comment(view.at) else {
        return Act::None;
    };

    let id = view.comment_state.comments[ci].id.clone();

    view.flip_comment(&id);
    view.rebuild_on_comment(ci);
    Act::None
}

fn select_line_key(view: &mut View, last: usize) {
    let on = !view.selected(view.at);

    view.select_row(view.at, on);
    view.at = (view.at + 1).min(last);
}

fn select_comment_key(view: &mut View, last: usize) {
    if let Some(id) = view
        .layout
        .rows
        .get(view.at)
        .and_then(|result| result.comment)
        .and_then(|ci| view.comment_state.comments.get(ci))
        .map(|character| character.id.clone())
        && !view.scm.comment_picks.remove(&id)
    {
        view.scm.comment_picks.insert(id);
    }

    view.layout.built = None;
    view.at = (view.at + 1).min(last);
}

fn key_view(view: &mut View, key: KeyEvent) -> Act {
    if matches!(key.code, KeyCode::Modifier(_)) {
        return Act::None;
    }

    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

    if let Some(action) = key_overlay(view, key) {
        return action;
    }

    if let Some(action) = key_gate(view, key, ctrl) {
        return action;
    }

    if let Some(action) = movement_key(view, key, ctrl) {
        return action;
    }

    if let Some(action) = comment_key(view, key) {
        return action;
    }

    operation_key(view, key)
}
