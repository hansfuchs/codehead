use super::{
    Act, Edit, Kind, Menu, Merge, MergeBtn, Op, Res, State, Stop, Sub, Tab, TextArea, Verdict, gh, open_url, theme,
    twice,
};

impl Merge {
    pub(super) fn step_pr(&mut self, directory: isize) -> Act {
        let count = self.prs.len();

        if count > 0 {
            self.cur = (self.cur as isize + directory).rem_euclid(count as isize) as usize;
        }

        Act::None
    }
    pub(super) fn set_sub(&mut self, text: Sub) -> Act {
        select_subview(self, text)
    }
    pub(super) fn changes_view(&mut self) -> Option<&mut crate::features::changes::View> {
        self.tab()?.changes.as_mut()
    }
    pub(super) fn open_on_github(&mut self, text: Stop) -> Act {
        open_stop_on_github(self, text)
    }
    pub(super) fn activate(&mut self, text_value: Stop) -> Act {
        activate_stop(self, text_value)
    }
    /// The labels of the dropdown: the repo's (0 on the PR), filtered.
    pub(super) fn label_opts(&self, filter: &str) -> Vec<gh::Label> {
        let Some(path) = self.tab_ref().and_then(|thread| thread.pr.as_ref()) else {
            return Vec::new();
        };

        let all = self.labels.get(&path.pr.repo).cloned().unwrap_or_default();
        let free: Vec<gh::Label> = all
            .into_iter()
            .filter(|line| !path.labels.iter().any(|x| x.name == line.name))
            .collect();

        crate::ui::fuzzy::rank(&free, filter, |line_value| line_value.name.clone(), |_| String::new())
            .into_iter()
            .map(|(i, _)| free[i].clone())
            .collect()
    }
    /// The stops of the tab in view, in order.
    pub(super) fn stops(&self) -> Vec<Stop> {
        let mut value = vec![Stop::Tabs, Stop::Subs];
        let Some(thread) = self.tab_ref() else { return value };

        if thread.sub == Sub::Changes {
            return value;
        }

        let Some(path) = &thread.pr else { return value };

        value.extend([Stop::Title, Stop::Status]);
        value.extend((0..path.labels.len()).map(Stop::Label));

        if path.can_edit {
            value.push(Stop::AddLabel);
        }

        if path.needs_update() {
            value.push(Stop::Update);
        }

        if matches!(path.state, State::Open | State::Draft) {
            value.push(Stop::Merge);
        }

        value.push(Stop::Checks);

        if thread.checks_open {
            value.extend(
                (0..path
                    .checks
                    .iter()
                    .filter(|character| character.verdict != Verdict::Skip)
                    .count())
                    .map(Stop::Check),
            );
        }

        value.push(Stop::NewComment);

        for (i, th) in path.threads.iter().enumerate() {
            value.extend((0..=th.replies.len()).map(|comment| Stop::Comment(i, comment)));
        }

        value.push(Stop::Body);

        value
    }
}

fn select_subview(widget_state: &mut Merge, text: Sub) -> Act {
    let dir = widget_state
        .current()
        .and_then(|path| widget_state.dirs.get(&path.repo).cloned())
        .unwrap_or_else(crate::engine::data_dir);

    let Some(pr) = widget_state.current().cloned() else {
        return Act::None;
    };

    let Some(thread) = widget_state.tab() else {
        return Act::None;
    };

    thread.sub = text;
    thread.input.menu = None;

    if text == Sub::Changes && thread.changes.is_none() {
        let mut view = crate::features::changes::View::for_pr(&pr, &dir);

        if let Some(path_value) = &thread.pr {
            view.set_pr(path_value);
        }

        view.refresh(true);
        thread.changes = Some(view);
    }

    Act::None
}

fn open_stop_on_github(widget_state: &mut Merge, text: Stop) -> Act {
    let Some(path) = widget_state.tab_ref().and_then(|thread| thread.pr.as_ref()) else {
        return Act::None;
    };

    let url = match text {
        Stop::Check(i) => path
            .checks
            .iter()
            .filter(|character| character.verdict != Verdict::Skip)
            .nth(i)
            .map(|character_value| character_value.url.clone()),
        Stop::Comment(i, comment) => path
            .threads
            .get(i)
            .and_then(|thread_value| {
                if comment == 0 {
                    Some(&thread_value.root)
                } else {
                    thread_value.replies.get(comment - 1)
                }
            })
            .map(|character_2| character_2.url.clone()),
        Stop::Checks => Some(format!("{}/checks", path.url)),
        _ => Some(path.url.clone()),
    };

    match url.filter(|user| !user.is_empty()) {
        Some(user_value) => {
            open_url(&user_value);
            Act::Toast(format!("open {user_value}"))
        }
        None => Act::None,
    }
}

fn activate_stop(widget_state: &mut Merge, text_value: Stop) -> Act {
    let Some(path) = widget_state.tab_ref().and_then(|thread_value| thread_value.pr.clone()) else {
        return Act::None;
    };

    let Some(thread_3) = widget_state.tab() else {
        return Act::None;
    };

    match text_value {
        Stop::Tabs => return widget_state.step_pr(1),
        // enter on the views row: the changes, the keys in the diff
        Stop::Subs => {
            thread_3.focus = None;
            return widget_state.set_sub(Sub::Changes);
        }
        Stop::Title if path.can_edit => edit_tab(thread_3, Edit::Title, &path.title, true),
        Stop::Body if path.can_edit => edit_tab(thread_3, Edit::Body, &path.body, false),
        Stop::Title | Stop::Body => return Act::Toast("0 rights to edit this pull request".into()),
        Stop::Status => return open_status_menu(thread_3, &path),
        Stop::Label(_) if !path.can_edit => return Act::Toast("0 rights to change the labels".into()),
        Stop::Label(i) => {
            if let Some(value) = remove_label(text_value, &path, thread_3, i) {
                return resolve_pending(widget_state, value);
            }
        }
        Stop::AddLabel => {
            open_label_menu(thread_3);
            load_label_options(widget_state, &path);
        }
        Stop::Update => {
            if !twice(thread_3, text_value) {
                return Act::Toast("enter again: the agents update the branch".into());
            }

            return Act::Prompt(widget_state.update_prompt(&path));
        }
        Stop::Merge => {
            let bytes = MergeBtn::of(&path);

            let Some(op) = bytes.op(&path) else {
                return Act::Toast(bytes.verb(&path));
            };

            if thread_3.input.busy.is_some() {
                return Act::Toast("gh: 1 edit at a time".into());
            }

            if !twice(thread_3, text_value) {
                return Act::Toast(format!("enter again: {}", bytes.verb(&path)));
            }

            return widget_state.run(&op);
        }
        Stop::Checks => thread_3.checks_open = !thread_3.checks_open,
        Stop::Check(_) => {}
        Stop::NewComment => edit_tab(thread_3, Edit::Comment, "", false),
        Stop::Comment(i, comment) => return open_comment(thread_3, &path, i, comment),
    }

    Act::None
}

fn remove_label(text_value: Stop, path: &gh::PullRequest, thread_3: &mut Tab, i: usize) -> Option<PendingAct> {
    if let Some(line) = path.labels.get(i) {
        if !twice(thread_3, text_value) {
            return Some(PendingAct::Act(Act::Toast(format!(
                "enter again: remove the label {}",
                line.name
            ))));
        }

        return Some(PendingAct::Operation(Op::RemoveLabel(line.name.clone())));
    }

    None
}

fn load_label_options(widget_state: &mut Merge, path: &gh::PullRequest) {
    let repo = path.pr.repo.clone();

    if !widget_state.labels.contains_key(&repo) && widget_state.budget() {
        widget_state.spawn(move || {
            let result = gh::labels(&repo);

            Res::Labels(repo, result)
        });
    }
}

fn open_label_menu(thread_3: &mut Tab) {
    let mut filter = TextArea::new(true, "filter labels");

    filter.style = Some(theme::text());
    thread_3.input.menu = Some(Menu::Labels {
        cur: 0,
        filter: Box::new(filter),
    });
}

pub(super) enum PendingAct {
    Act(Act),
    Operation(Op),
}
pub(super) fn resolve_pending(view: &mut Merge, pending: PendingAct) -> Act {
    match pending {
        PendingAct::Act(action) => action,
        PendingAct::Operation(operation) => view.run(&operation),
    }
}

fn edit_tab(thread_2: &mut Tab, error: Edit, text: &str, single: bool) {
    let mut ta = TextArea::new(single, "");

    ta.set_text(text);
    ta.move_end();
    thread_2.input.edit = Some((error, ta));
}

fn open_status_menu(thread_3: &mut Tab, path: &gh::PullRequest) -> Act {
    let opts: Vec<(&'static str, Op)> = match path.state {
        State::Open => vec![("convert to draft", Op::Draft), ("close pull request", Op::Close)],
        State::Draft => vec![("ready for review", Op::Ready), ("close pull request", Op::Close)],
        State::Closed => vec![("reopen pull request", Op::Reopen)],
        State::Merged => return Act::Toast("merged: 0 status changes".into()),
    };

    if !path.can_edit {
        return Act::Toast("0 rights to change this pull request".into());
    }

    thread_3.input.menu = Some(Menu::Status(0, opts));

    Act::None
}

fn open_comment(thread_3: &mut Tab, path: &gh::PullRequest, i: usize, comment: usize) -> Act {
    let Some(th) = path.threads.get(i) else {
        return Act::None;
    };

    let Some(character) = (if comment == 0 {
        Some(&th.root)
    } else {
        th.replies.get(comment - 1)
    }) else {
        return Act::None;
    };

    if character.mine {
        thread_3.input.menu = Some(Menu::Mine {
            cur: 0,
            thread: i,
            comment,
            armed: false,
        });
    } else if character.kind == Kind::Line {
        edit_tab(thread_3, Edit::Reply(i, th.root.id), "", false);
    } else {
        edit_tab(
            thread_3,
            Edit::Comment,
            &gh::quote(&character.author, &character.body),
            false,
        );
        thread_3.focus = Some(Stop::NewComment);
    }

    Act::None
}
