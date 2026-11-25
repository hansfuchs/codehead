use super::{Act, Edit, KeyCode, KeyEvent, KeyModifiers, Kind, Menu, Merge, Op, Outcome, Stop, Sub, TextArea, twice};
use super::{EditState, PendingAct, Pr, resolve_pending};

impl Merge {
    /// The changes view: a key on the bar rows. ←→: PR / view; ↑↓: row; ↓ from the views row,
    /// enter: into the diff.
    pub(super) fn bar_key(&mut self, stop: Stop, key: KeyEvent) -> Act {
        dispatch_bar_key(self, stop, key)
    }

    pub(super) fn edit_key(&mut self, key: KeyEvent) -> Act {
        dispatch_edit_key(self, key)
    }

    pub(super) fn menu_key(&mut self, key: KeyEvent) -> Act {
        dispatch_menu_key(self, key)
    }
}

fn dispatch_bar_key(merge: &mut Merge, stop: Stop, key: KeyEvent) -> Act {
    let to = |merge: &mut Merge, focus: Option<Stop>| {
        if let Some(tab) = merge.tab() {
            tab.focus = focus;
        }
    };

    match (stop, key.code) {
        (Stop::Tabs, KeyCode::Left | KeyCode::Right) => {
            let act = merge.step_pr(if key.code == KeyCode::Right { 1 } else { -1 });

            to(merge, Some(Stop::Tabs));
            return act;
        }
        (Stop::Subs, KeyCode::Left | KeyCode::Right) => {
            let act = merge.set_sub(if key.code == KeyCode::Right {
                Sub::Changes
            } else {
                Sub::Overview
            });

            to(merge, Some(Stop::Subs));
            return act;
        }
        (Stop::Subs, KeyCode::Up | KeyCode::Char('k')) => to(merge, Some(Stop::Tabs)),
        (Stop::Tabs, KeyCode::Down | KeyCode::Char('j')) => to(merge, Some(Stop::Subs)),
        (Stop::Subs, KeyCode::Down | KeyCode::Char('j')) | (_, KeyCode::Enter) => to(merge, None),
        _ => {}
    }

    Act::None
}

fn dispatch_edit_key(merge: &mut Merge, key: KeyEvent) -> Act {
    let save = key.code == KeyCode::Enter
        && (key
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER));

    let Some(tab) = merge.tab() else {
        return Act::None;
    };

    let Some((edit, textarea)) = tab.input.edit.as_mut() else {
        return Act::None;
    };

    let single = matches!(edit, Edit::Title);

    // esc: closes; edited text only on a 2nd esc (esc never drops edits unasked)
    if key.code == KeyCode::Esc {
        let dirty = textarea.edited();

        if !crate::ui::textarea::esc_drops(&mut tab.input.esc_at, dirty) {
            return Act::Toast("esc again: drop the edit · ctrl+⏎: save".into());
        }

        tab.input.edit = None;
        return if dirty {
            Act::Toast("edit dropped".into())
        } else {
            Act::None
        };
    }

    if save || (single && key.code == KeyCode::Enter) {
        let text = textarea.text();
        let pending = save_edit(tab, text);

        return resolve_pending(merge, pending);
    }

    if let Outcome::Ignored = textarea.key(key)
        && key.code == KeyCode::Enter
        && !single
    {
        textarea.newline();
    }

    Act::None
}

fn save_edit(tab: &mut super::Tab, text: String) -> PendingAct {
    let (edit, _) = tab.input.edit.take().unwrap_or((Edit::Title, TextArea::new(true, "")));
    let pr = tab.pr.clone();

    let op = match edit {
        Edit::Title if text.trim().is_empty() => return PendingAct::Act(Act::Toast("a title needs text".into())),
        Edit::Title => Op::Title(text.trim().to_string()),
        Edit::Body => Op::Body(text),
        _ if text.trim().is_empty() => return PendingAct::Act(Act::Toast("0 text: 0 comment".into())),
        Edit::Comment => Op::Comment(text),
        Edit::Reply(_, id) => Op::Reply(id, text),
        Edit::Change(_, _, kind, id) => Op::Edit(kind, id, text),
    };

    // at once on screen (the read confirms)
    if let Some(mut pr) = pr {
        match &op {
            Op::Title(title) => pr.title.clone_from(title),
            Op::Body(body) => pr.body.clone_from(body),
            _ => {}
        }

        tab.pr = Some(pr);
    }

    PendingAct::Operation(op)
}

fn dispatch_menu_key(merge: &mut Merge, key: KeyEvent) -> Act {
    let filter_text = match merge.tab_ref().and_then(|tab| tab.input.menu.as_ref()) {
        Some(Menu::Labels { filter, .. }) => filter.text(),
        _ => String::new(),
    };

    let label_opts = merge.label_opts(&filter_text);
    let pr = merge.tab_ref().and_then(|tab| tab.pr.clone());

    let Some(tab) = merge.tab() else {
        return Act::None;
    };

    let pending = update_menu(tab, key, &label_opts, pr.as_ref());

    resolve_pending(merge, pending)
}

fn update_menu(
    tab: &mut super::Tab,
    key: KeyEvent,
    label_opts: &[crate::infra::gh::Label],
    pr: Option<&Pr>,
) -> PendingAct {
    let Some(menu) = tab.input.menu.as_mut() else {
        return PendingAct::Act(Act::None);
    };

    if key.code == KeyCode::Esc {
        tab.input.menu = None;
        return PendingAct::Act(Act::None);
    }

    let count = match menu {
        Menu::Status(_, options) => options.len(),
        Menu::Mine { .. } => 2,
        Menu::Labels { .. } => label_opts.len(),
    };

    let cur = match menu {
        Menu::Status(cur, _) | Menu::Mine { cur, .. } | Menu::Labels { cur, .. } => cur,
    };

    match key.code {
        KeyCode::Up => *cur = cur.saturating_sub(1),
        KeyCode::Down => *cur = (*cur + 1).min(count.saturating_sub(1)),
        KeyCode::Enter => {
            let selection = *cur;

            return select_menu(tab, label_opts, pr, selection);
        }

        _ => {
            if let Menu::Labels { filter, cur } = menu {
                filter.key(key);
                *cur = 0;
            }
        }
    }

    PendingAct::Act(Act::None)
}

fn select_menu(
    tab: &mut super::Tab,
    label_opts: &[crate::infra::gh::Label],
    pr: Option<&Pr>,
    selection: usize,
) -> PendingAct {
    let Some(menu) = tab.input.menu.as_mut() else {
        return PendingAct::Act(Act::None);
    };

    match menu {
        Menu::Status(_, options) => {
            let op = options.get(selection).map(|x| x.1.clone());

            // close: a 2nd enter within ARM (the menu stays open for it)
            if matches!(op, Some(Op::Close)) && !twice(tab, Stop::Status) {
                return PendingAct::Act(Act::Toast("enter again: close the pull request".into()));
            }

            tab.input.menu = None;

            if let Some(op) = op {
                return PendingAct::Operation(op);
            }
        }
        Menu::Mine {
            thread, comment, armed, ..
        } => {
            let choice = CommentChoice {
                selection,
                thread: *thread,
                comment: *comment,
                armed: *armed,
            };

            if let Some(pending) = select_comment_action(&mut tab.input, pr, choice) {
                return pending;
            }
        }
        Menu::Labels { .. } => {
            tab.input.menu = None;

            if let Some(label) = label_opts.get(selection) {
                return PendingAct::Operation(Op::AddLabel(label.name.clone()));
            }
        }
    }

    PendingAct::Act(Act::None)
}

fn select_comment_action(input: &mut EditState, pr: Option<&Pr>, choice: CommentChoice) -> Option<PendingAct> {
    let CommentChoice {
        selection,
        thread,
        comment,
        armed,
    } = choice;

    let Some(chosen) = pr.and_then(|pr| pr.threads.get(thread)).and_then(|entry| {
        if comment == 0 {
            Some(entry.root.clone())
        } else {
            entry.replies.get(comment - 1).cloned()
        }
    }) else {
        input.menu = None;
        return Some(PendingAct::Act(Act::None));
    };

    if selection == 0 {
        input.menu = None;
        let mut textarea = TextArea::new(false, "");

        textarea.set_text(&chosen.body);
        textarea.move_end();
        input.edit = Some((Edit::Change(thread, comment, chosen.kind, chosen.id), textarea));
    } else if chosen.kind == Kind::Review {
        input.menu = None;
        return Some(PendingAct::Act(Act::Toast(
            "a review's text cannot be deleted: edit it".into(),
        )));
    } else if !armed {
        if let Some(Menu::Mine { armed, .. }) = input.menu.as_mut() {
            *armed = true;
        }

        return Some(PendingAct::Act(Act::Toast("enter again: delete the comment".into())));
    } else {
        input.menu = None;
        return Some(PendingAct::Operation(Op::Delete(chosen.kind, chosen.id)));
    }

    None
}

#[derive(Clone, Copy)]
struct CommentChoice {
    selection: usize,
    thread: usize,
    comment: usize,
    armed: bool,
}
