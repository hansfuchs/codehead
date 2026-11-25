use super::{Act, Card, Form, Instant, Out, PathBuf, Pending, ThreadList};

impl ThreadList {
    /// The actions on the targets.
    pub(super) fn acts(&self) -> Vec<Act> {
        acts_thread_list(self)
    }
    pub(crate) fn run(&mut self, act: Act) -> Out {
        run_thread_list(self, act)
    }

    /// The yes to a question: its job starts. The thread in view among its threads: home.
    pub(super) fn confirm(&mut self) -> Out {
        let Some(pending) = self.job_state.pending.take() else {
            return Out::None;
        };

        let home = self.here.as_ref().is_some_and(|here| pending.ids.contains(here));

        start(pending.act, pending.ids, self);
        if home { Out::Home } else { Out::None }
    }

    fn spawn_job(&mut self, act: Act, work: impl FnOnce() -> (String, bool) + Send + 'static) {
        if self.inline {
            let (text, err) = work();

            self.done(text, err);
            return;
        }

        let (tx, rx) = crate::infra::bus::channel();

        std::thread::spawn(move || {
            let _ = tx.send(work());
        });

        self.job_state.job = Some(rx);
        self.job_state.status = Some((
            format!(
                "{}…",
                match act {
                    Act::Archive => "archiving",
                    Act::Unarchive => "unarchiving",
                    _ => "deleting",
                }
            ),
            false,
            Instant::now(),
        ));
    }
    pub(super) fn edit(&mut self, id: String, edit: crate::domain::threads::Edit) {
        edit_thread_list(self, id, edit);
    }
}
/// Delete thread ID: its row from the store (linked worktree or in place), `threads::remove`.
pub(super) fn delete(id: &str) -> Result<String, String> {
    let row = crate::domain::threads::row_of(id).ok_or_else(|| format!("no thread {id}"))?;
    let repo = row.repo.clone().unwrap_or_else(|| PathBuf::from(&row.path));
    let lines = crate::ui::worktree_preview::remove(&repo, &[row], &crate::engine::logs_dir());

    let messages: Vec<String> = lines
        .iter()
        .skip(1)
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
                .trim()
                .to_string()
        })
        .collect();

    match messages.iter().find(|message| {
        message.starts_with("skip:")
            || message.contains("failed")
            || message.starts_with("worktree:")
            || message.starts_with("branch:")
    }) {
        Some(error) => Err(error.clone()),
        None => Ok(format!("deleted · {}", messages.join(" · "))),
    }
}

fn run_act_unarchive(targets: &[Card], thread_list: &mut ThreadList) -> Out {
    let ids = targets
        .iter()
        .filter(|card| card.archived())
        .map(|card| card.id.clone())
        .collect();

    start(Act::Unarchive, ids, thread_list);
    Out::None
}

fn run_act_edit(targets: &[Card], thread_list: &mut ThreadList) -> Out {
    thread_list.form = Form::open(&targets[0].id, &thread_list.cards, &thread_list.roster);

    if thread_list.form.is_none() {
        thread_list.job_state.status = Some(("thread gone".into(), true, Instant::now()));
    }

    Out::None
}

/// ACT on IDS, off the UI thread (tests: at once).
fn start(act: Act, ids: Vec<String>, thread_list: &mut ThreadList) {
    thread_list.sel.clear();

    for id in &ids {
        let to = match act {
            Act::Archive => Some("archived".to_string()),
            Act::Unarchive => Some("open".to_string()),
            _ => None,
        };

        thread_list.job_state.moved.insert(id.clone(), to);
    }

    let cards = thread_list.cards.clone();

    thread_list.set_cards(cards);

    // the next list (menu closed, opened again) starts from the moved cards too
    crate::infra::background::save("cards", &thread_list.cards);

    let work = move || {
        let mut ok = Vec::new();
        let mut errs = Vec::new();

        for id in &ids {
            let result = match act {
                Act::Archive => crate::domain::threads::archive(id),
                Act::Unarchive => crate::domain::threads::unarchive(id),
                Act::Delete => delete(id),
                _ => Ok(String::new()),
            };

            match result {
                Ok(message) => ok.push(message),
                Err(error) => errs.push(error),
            }
        }

        let verb = match act {
            Act::Archive => "archived",
            Act::Unarchive => "unarchived",
            _ => "deleted",
        };

        match (ok.len(), errs.first()) {
            (1, None) if ids.len() == 1 => (ok[0].clone(), false),
            (count, None) => (format!("{count} threads {verb}"), false),
            (0, Some(error)) => (error.clone(), true),
            (count, Some(error)) => (format!("{count} threads {verb} · {} failed: {error}", errs.len()), true),
        }
    };

    thread_list.spawn_job(act, work);
}

fn run_thread_list(state: &mut ThreadList, act: Act) -> Out {
    state.job_state.actions = None;
    let targets = state.targets();

    if targets.is_empty() || !state.acts().contains(&act) {
        return Out::None;
    }

    match act {
        Act::Open => Out::Open(Box::new(targets[0].clone())),
        Act::Edit => run_act_edit(&targets, state),
        Act::Unarchive => run_act_unarchive(&targets, state),
        Act::Archive => {
            let ids = targets
                .iter()
                .filter(|card| !card.archived())
                .map(|card| card.id.clone())
                .collect();

            state.job_state.pending = Some(Pending { act, ids });
            Out::None
        }
        Act::Delete => {
            state.job_state.pending = Some(Pending {
                act,
                ids: targets.iter().map(|card| card.id.clone()).collect(),
            });
            Out::None
        }
    }
}

fn acts_thread_list(state: &ThreadList) -> Vec<Act> {
    let targets = state.targets();

    if targets.is_empty() {
        return Vec::new();
    }

    let mut acts = Vec::new();

    if targets.len() == 1 {
        acts.push(Act::Open);
    }

    if targets.iter().any(|card| !card.archived()) {
        acts.push(Act::Archive);
    }

    if targets.iter().any(Card::archived) {
        acts.push(Act::Unarchive);
    }

    if targets.len() == 1 {
        acts.push(Act::Edit);
    }

    acts.push(Act::Delete);

    acts
}

fn edit_thread_list(state: &mut ThreadList, id: String, edit: crate::domain::threads::Edit) {
    let work = move || match crate::domain::threads::edit(&id, &edit) {
        Ok(message) => (message, false),
        Err(error) => (error, true),
    };

    if cfg!(test) {
        let (message, err) = work();

        state.done(message, err);
        return;
    }

    let (tx, rx) = crate::infra::bus::channel();

    std::thread::spawn(move || {
        let _ = tx.send(work());
    });

    state.job_state.job = Some(rx);
    state.job_state.status = Some(("saving…".into(), false, Instant::now()));
}
