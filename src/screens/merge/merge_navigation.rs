use super::{
    Act, Edit, Kind, Menu, Merge, MergeBtn, Op, Res, State, Stop, Sub, Tab, TextArea, Verdict, gh, open_url, theme,
    twice,
};

impl Merge {
    pub(super) fn step_pr(&mut self, step: isize) -> Act {
        let count = self.prs.len();

        if count > 0 {
            self.cur = (self.cur as isize + step).rem_euclid(count as isize) as usize;
        }

        Act::None
    }

    pub(super) fn set_sub(&mut self, sub: Sub) -> Act {
        select_subview(self, sub)
    }

    pub(super) fn changes_view(&mut self) -> Option<&mut crate::features::changes::View> {
        self.tab()?.changes.as_mut()
    }

    pub(super) fn open_on_github(&mut self, stop: Stop) -> Act {
        open_stop_on_github(self, stop)
    }

    pub(super) fn activate(&mut self, stop: Stop) -> Act {
        activate_stop(self, stop)
    }

    /// The labels of the dropdown: the repo's (0 on the PR), filtered.
    pub(super) fn label_opts(&self, filter: &str) -> Vec<gh::Label> {
        let Some(pr) = self.tab_ref().and_then(|tab| tab.pr.as_ref()) else {
            return Vec::new();
        };

        let all = self.labels.get(&pr.pr.repo).cloned().unwrap_or_default();
        let free: Vec<gh::Label> = all
            .into_iter()
            .filter(|label| !pr.labels.iter().any(|on_pr| on_pr.name == label.name))
            .collect();

        crate::ui::fuzzy::rank(&free, filter, |label| label.name.clone(), |_| String::new())
            .into_iter()
            .map(|(i, _)| free[i].clone())
            .collect()
    }

    /// The stops of the tab in view, in order.
    pub(super) fn stops(&self) -> Vec<Stop> {
        let mut stops = vec![Stop::Tabs, Stop::Subs];
        let Some(tab) = self.tab_ref() else { return stops };

        if tab.sub == Sub::Changes {
            return stops;
        }

        let Some(pr) = &tab.pr else { return stops };

        stops.extend([Stop::Title, Stop::Status]);
        stops.extend((0..pr.labels.len()).map(Stop::Label));

        if pr.can_edit {
            stops.push(Stop::AddLabel);
        }

        if pr.needs_update() {
            stops.push(Stop::Update);
        }

        if matches!(pr.state, State::Open | State::Draft) {
            stops.push(Stop::Merge);
        }

        stops.push(Stop::Checks);

        if tab.checks_open {
            stops.extend((0..pr.checks.iter().filter(|check| check.verdict != Verdict::Skip).count()).map(Stop::Check));
        }

        stops.push(Stop::NewComment);

        for (i, thread) in pr.threads.iter().enumerate() {
            stops.extend((0..=thread.replies.len()).map(|comment| Stop::Comment(i, comment)));
        }

        stops.push(Stop::Body);

        stops
    }
}

fn select_subview(merge: &mut Merge, sub: Sub) -> Act {
    let dir = merge
        .current()
        .and_then(|pr| merge.dirs.get(&pr.repo).cloned())
        .unwrap_or_else(crate::engine::data_dir);

    let Some(pr) = merge.current().cloned() else {
        return Act::None;
    };

    let Some(tab) = merge.tab() else {
        return Act::None;
    };

    tab.sub = sub;
    tab.input.menu = None;

    if sub == Sub::Changes && tab.changes.is_none() {
        let mut view = crate::features::changes::View::for_pr(&pr, &dir);

        if let Some(pull_request) = &tab.pr {
            view.set_pr(pull_request);
        }

        view.refresh(true);
        tab.changes = Some(view);
    }

    Act::None
}

fn open_stop_on_github(merge: &mut Merge, stop: Stop) -> Act {
    let Some(pr) = merge.tab_ref().and_then(|tab| tab.pr.as_ref()) else {
        return Act::None;
    };

    let url = match stop {
        Stop::Check(i) => pr
            .checks
            .iter()
            .filter(|check| check.verdict != Verdict::Skip)
            .nth(i)
            .map(|check| check.url.clone()),
        Stop::Comment(i, comment) => pr
            .threads
            .get(i)
            .and_then(|thread| {
                if comment == 0 {
                    Some(&thread.root)
                } else {
                    thread.replies.get(comment - 1)
                }
            })
            .map(|entry| entry.url.clone()),
        Stop::Checks => Some(format!("{}/checks", pr.url)),
        _ => Some(pr.url.clone()),
    };

    match url.filter(|url| !url.is_empty()) {
        Some(url) => {
            open_url(&url);
            Act::Toast(format!("open {url}"))
        }
        None => Act::None,
    }
}

fn activate_stop(merge: &mut Merge, stop: Stop) -> Act {
    let Some(pr) = merge.tab_ref().and_then(|tab| tab.pr.clone()) else {
        return Act::None;
    };

    let Some(tab) = merge.tab() else {
        return Act::None;
    };

    match stop {
        Stop::Tabs => return merge.step_pr(1),
        // enter on the views row: the changes, the keys in the diff
        Stop::Subs => {
            tab.focus = None;
            return merge.set_sub(Sub::Changes);
        }
        Stop::Title if pr.can_edit => edit_tab(tab, Edit::Title, &pr.title, true),
        Stop::Body if pr.can_edit => edit_tab(tab, Edit::Body, &pr.body, false),
        Stop::Title | Stop::Body => return Act::Toast("0 rights to edit this pull request".into()),
        Stop::Status => return open_status_menu(tab, &pr),
        Stop::Label(_) if !pr.can_edit => return Act::Toast("0 rights to change the labels".into()),
        Stop::Label(i) => {
            if let Some(pending) = remove_label(stop, &pr, tab, i) {
                return resolve_pending(merge, pending);
            }
        }
        Stop::AddLabel => {
            open_label_menu(tab);
            load_label_options(merge, &pr);
        }
        Stop::Update => {
            if !twice(tab, stop) {
                return Act::Toast("enter again: the agents update the branch".into());
            }

            return Act::Prompt(merge.update_prompt(&pr));
        }
        Stop::Merge => {
            let button = MergeBtn::of(&pr);

            let Some(op) = button.op(&pr) else {
                return Act::Toast(button.verb(&pr));
            };

            if tab.input.busy.is_some() {
                return Act::Toast("gh: 1 edit at a time".into());
            }

            if !twice(tab, stop) {
                return Act::Toast(format!("enter again: {}", button.verb(&pr)));
            }

            return merge.run(&op);
        }
        Stop::Checks => tab.checks_open = !tab.checks_open,
        Stop::Check(_) => {}
        Stop::NewComment => edit_tab(tab, Edit::Comment, "", false),
        Stop::Comment(i, comment) => return open_comment(tab, &pr, i, comment),
    }

    Act::None
}

fn remove_label(stop: Stop, pr: &gh::PullRequest, tab: &mut Tab, i: usize) -> Option<PendingAct> {
    if let Some(label) = pr.labels.get(i) {
        if !twice(tab, stop) {
            return Some(PendingAct::Act(Act::Toast(format!(
                "enter again: remove the label {}",
                label.name
            ))));
        }

        return Some(PendingAct::Operation(Op::RemoveLabel(label.name.clone())));
    }

    None
}

fn load_label_options(merge: &mut Merge, pr: &gh::PullRequest) {
    let repo = pr.pr.repo.clone();

    if !merge.labels.contains_key(&repo) && merge.budget() {
        merge.spawn(move || {
            let result = gh::labels(&repo);

            Res::Labels(repo, result)
        });
    }
}

fn open_label_menu(tab: &mut Tab) {
    let mut filter = TextArea::new(true, "filter labels");

    filter.style = Some(theme::text());
    tab.input.menu = Some(Menu::Labels {
        cur: 0,
        filter: Box::new(filter),
    });
}

pub(super) enum PendingAct {
    Act(Act),
    Operation(Op),
}

pub(super) fn resolve_pending(merge: &mut Merge, pending: PendingAct) -> Act {
    match pending {
        PendingAct::Act(action) => action,
        PendingAct::Operation(operation) => merge.run(&operation),
    }
}

fn edit_tab(tab: &mut Tab, edit: Edit, text: &str, single: bool) {
    let mut textarea = TextArea::new(single, "");

    textarea.set_text(text);
    textarea.move_end();
    tab.input.edit = Some((edit, textarea));
}

fn open_status_menu(tab: &mut Tab, pr: &gh::PullRequest) -> Act {
    let opts: Vec<(&'static str, Op)> = match pr.state {
        State::Open => vec![("convert to draft", Op::Draft), ("close pull request", Op::Close)],
        State::Draft => vec![("ready for review", Op::Ready), ("close pull request", Op::Close)],
        State::Closed => vec![("reopen pull request", Op::Reopen)],
        State::Merged => return Act::Toast("merged: 0 status changes".into()),
    };

    if !pr.can_edit {
        return Act::Toast("0 rights to change this pull request".into());
    }

    tab.input.menu = Some(Menu::Status(0, opts));

    Act::None
}

fn open_comment(tab: &mut Tab, pr: &gh::PullRequest, i: usize, comment: usize) -> Act {
    let Some(thread) = pr.threads.get(i) else {
        return Act::None;
    };

    let Some(entry) = (if comment == 0 {
        Some(&thread.root)
    } else {
        thread.replies.get(comment - 1)
    }) else {
        return Act::None;
    };

    if entry.mine {
        tab.input.menu = Some(Menu::Mine {
            cur: 0,
            thread: i,
            comment,
            armed: false,
        });
    } else if entry.kind == Kind::Line {
        edit_tab(tab, Edit::Reply(i, thread.root.id), "", false);
    } else {
        edit_tab(tab, Edit::Comment, &gh::quote(&entry.author, &entry.body), false);
        tab.focus = Some(Stop::NewComment);
    }

    Act::None
}
