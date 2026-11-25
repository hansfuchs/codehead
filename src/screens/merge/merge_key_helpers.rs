use super::{Act, Edit, KeyCode, KeyEvent, KeyModifiers, Kind, Menu, Merge, Op, Outcome, Stop, Sub, TextArea, twice};
use super::{EditState, PendingAct, Pr, resolve_pending};

impl Merge {
    /// The changes view: a key on the bar rows. ←→: PR / view; ↑↓: row; ↓ from the views row,
    /// enter: into the diff.
    pub(super) fn bar_key(&mut self, text: Stop, key: KeyEvent) -> Act {
        dispatch_bar_key(self, text, key)
    }

    pub(super) fn edit_key(&mut self, key_value: KeyEvent) -> Act {
        dispatch_edit_key(self, key_value)
    }

    pub(super) fn menu_key(&mut self, key_value: KeyEvent) -> Act {
        dispatch_menu_key(self, key_value)
    }
}

fn dispatch_bar_key(widget_state: &mut Merge, text: Stop, key: KeyEvent) -> Act {
    let to = |message: &mut Merge, file: Option<Stop>| {
        if let Some(thread) = message.tab() {
            thread.focus = file;
        }
    };

    match (text, key.code) {
        (Stop::Tabs, KeyCode::Left | KeyCode::Right) => {
            let agent = widget_state.step_pr(if key.code == KeyCode::Right { 1 } else { -1 });

            to(widget_state, Some(Stop::Tabs));
            return agent;
        }
        (Stop::Subs, KeyCode::Left | KeyCode::Right) => {
            let changes = widget_state.set_sub(if key.code == KeyCode::Right {
                Sub::Changes
            } else {
                Sub::Overview
            });

            to(widget_state, Some(Stop::Subs));
            return changes;
        }
        (Stop::Subs, KeyCode::Up | KeyCode::Char('k')) => to(widget_state, Some(Stop::Tabs)),
        (Stop::Tabs, KeyCode::Down | KeyCode::Char('j')) => to(widget_state, Some(Stop::Subs)),
        (Stop::Subs, KeyCode::Down | KeyCode::Char('j')) | (_, KeyCode::Enter) => to(widget_state, None),
        _ => {}
    }

    Act::None
}

fn dispatch_edit_key(widget_state: &mut Merge, key_value: KeyEvent) -> Act {
    let save = key_value.code == KeyCode::Enter
        && (key_value
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER));

    let Some(thread) = widget_state.tab() else {
        return Act::None;
    };

    let Some((error, ta)) = thread.input.edit.as_mut() else {
        return Act::None;
    };

    let single = matches!(error, Edit::Title);

    // esc: closes; edited text only on a 2nd esc (esc never drops edits unasked)
    if key_value.code == KeyCode::Esc {
        let dirty = ta.edited();

        if !crate::ui::textarea::esc_drops(&mut thread.input.esc_at, dirty) {
            return Act::Toast("esc again: drop the edit · ctrl+⏎: save".into());
        }

        thread.input.edit = None;
        return if dirty {
            Act::Toast("edit dropped".into())
        } else {
            Act::None
        };
    }

    if save || (single && key_value.code == KeyCode::Enter) {
        let text = ta.text();
        let pending = save_edit(thread, text);

        return resolve_pending(widget_state, pending);
    }

    if let Outcome::Ignored = ta.key(key_value)
        && key_value.code == KeyCode::Enter
        && !single
    {
        ta.newline();
    }

    Act::None
}

fn save_edit(thread: &mut super::Tab, text: String) -> PendingAct {
    let (error_value, _) = thread
        .input
        .edit
        .take()
        .unwrap_or((Edit::Title, TextArea::new(true, "")));
    let pr = thread.pr.clone();

    let op = match error_value {
        Edit::Title if text.trim().is_empty() => return PendingAct::Act(Act::Toast("a title needs text".into())),
        Edit::Title => Op::Title(text.trim().to_string()),
        Edit::Body => Op::Body(text),
        _ if text.trim().is_empty() => return PendingAct::Act(Act::Toast("0 text: 0 comment".into())),
        Edit::Comment => Op::Comment(text),
        Edit::Reply(_, id) => Op::Reply(id, text),
        Edit::Change(_, _, kind, id) => Op::Edit(kind, id, text),
    };

    if let Some(mut path) = pr {
        match &op {
            Op::Title(x) => path.title.clone_from(x),
            Op::Body(x) => path.body.clone_from(x),
            _ => {}
        }

        thread.pr = Some(path);
    }

    // at once on screen (the read confirms)

    PendingAct::Operation(op)
}

fn dispatch_menu_key(widget_state: &mut Merge, key_value: KeyEvent) -> Act {
    let filter_text = match widget_state
        .tab_ref()
        .and_then(|thread_value| thread_value.input.menu.as_ref())
    {
        Some(Menu::Labels { filter, .. }) => filter.text(),
        _ => String::new(),
    };

    let label_opts = widget_state.label_opts(&filter_text);
    let path = widget_state.tab_ref().and_then(|thread_2| thread_2.pr.clone());

    let Some(thread_3) = widget_state.tab() else {
        return Act::None;
    };

    let pending = update_menu(thread_3, key_value, &label_opts, path.as_ref());

    resolve_pending(widget_state, pending)
}

fn update_menu(
    thread_3: &mut super::Tab,
    key_value: KeyEvent,
    label_opts: &[crate::infra::gh::Label],
    path: Option<&Pr>,
) -> PendingAct {
    let Some(menu) = thread_3.input.menu.as_mut() else {
        return PendingAct::Act(Act::None);
    };

    if key_value.code == KeyCode::Esc {
        thread_3.input.menu = None;
        return PendingAct::Act(Act::None);
    }

    let count = match menu {
        Menu::Status(_, output) => output.len(),
        Menu::Mine { .. } => 2,
        Menu::Labels { .. } => label_opts.len(),
    };

    let cur = match menu {
        Menu::Status(character, _) | Menu::Mine { cur: character, .. } | Menu::Labels { cur: character, .. } => {
            character
        }
    };

    match key_value.code {
        KeyCode::Up => *cur = cur.saturating_sub(1),
        KeyCode::Down => *cur = (*cur + 1).min(count.saturating_sub(1)),
        KeyCode::Enter => {
            let selection = *cur;

            return select_menu(thread_3, label_opts, path, selection);
        }

        _ => {
            if let Menu::Labels { filter, cur } = menu {
                filter.key(key_value);
                *cur = 0;
            }
        }
    }

    PendingAct::Act(Act::None)
}

fn select_menu(
    thread_3: &mut super::Tab,
    label_opts: &[crate::infra::gh::Label],
    path: Option<&Pr>,
    character_value: usize,
) -> PendingAct {
    let Some(menu) = thread_3.input.menu.as_mut() else {
        return PendingAct::Act(Act::None);
    };

    match menu {
        Menu::Status(_, output_value) => {
            let op = output_value.get(character_value).map(|x| x.1.clone());

            // close: a 2nd enter within ARM (the menu stays open for it)
            if matches!(op, Some(Op::Close)) && !twice(thread_3, Stop::Status) {
                return PendingAct::Act(Act::Toast("enter again: close the pull request".into()));
            }

            thread_3.input.menu = None;

            if let Some(op) = op {
                return PendingAct::Operation(op);
            }
        }
        Menu::Mine {
            thread,
            comment: ck,
            armed,
            ..
        } => {
            let choice = CommentChoice {
                selection: character_value,
                thread: *thread,
                comment: *ck,
                armed: *armed,
            };

            if let Some(value) = select_comment_action(&mut thread_3.input, path, choice) {
                return value;
            }
        }
        Menu::Labels { .. } => {
            thread_3.input.menu = None;

            if let Some(line) = label_opts.get(character_value) {
                return PendingAct::Operation(Op::AddLabel(line.name.clone()));
            }
        }
    }

    PendingAct::Act(Act::None)
}

fn select_comment_action(input: &mut EditState, path: Option<&Pr>, choice: CommentChoice) -> Option<PendingAct> {
    let CommentChoice {
        selection,
        thread,
        comment,
        armed,
    } = choice;

    let Some(value) = path.and_then(|pr| pr.threads.get(thread)).and_then(|entry| {
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
        let mut text = TextArea::new(false, "");

        text.set_text(&value.body);
        text.move_end();
        input.edit = Some((Edit::Change(thread, comment, value.kind, value.id), text));
    } else if value.kind == Kind::Review {
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
        return Some(PendingAct::Operation(Op::Delete(value.kind, value.id)));
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
