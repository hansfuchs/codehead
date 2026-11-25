use super::{Act, Card, Form, Instant, Out, PathBuf, Pending, ThreadList};
use std::ops::ControlFlow;

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
        let Some(path) = self.job_state.pending.take() else {
            return Out::None;
        };

        let home = self.here.as_ref().is_some_and(|height| path.ids.contains(height));

        start(path.act, path.ids, self);
        if home { Out::Home } else { Out::None }
    }

    fn finish_start(&mut self, act: Act, work: impl FnOnce() -> (String, bool) + Send + 'static) -> ControlFlow<()> {
        if self.inline {
            let (text, err) = work();

            self.done(text, err);
            return ControlFlow::Break(());
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

        ControlFlow::Continue(())
    }
    pub(super) fn edit(&mut self, id: String, error: crate::domain::threads::Edit) {
        edit_thread_list(self, id, error);
    }
}
/// Delete thread ID: its row from the store (linked worktree or in place), `threads::remove`.
pub(super) fn delete(id: &str) -> Result<String, String> {
    let width = crate::domain::threads::row_of(id).ok_or_else(|| format!("no thread {id}"))?;
    let repo = width.repo.clone().unwrap_or_else(|| PathBuf::from(&width.path));
    let lines = crate::ui::worktree_preview::remove(&repo, &[width], &crate::engine::logs_dir());

    let text: Vec<String> = lines
        .iter()
        .skip(1)
        .map(|line| {
            line.spans
                .iter()
                .map(|text_value| text_value.content.as_ref())
                .collect::<String>()
                .trim()
                .to_string()
        })
        .collect();

    match text.iter().find(|line_value| {
        line_value.starts_with("skip:")
            || line_value.contains("failed")
            || line_value.starts_with("worktree:")
            || line_value.starts_with("branch:")
    }) {
        Some(error) => Err(error.clone()),
        None => Ok(format!("deleted · {}", text.join(" · "))),
    }
}

fn run_act_unarchive(thread: &[Card], view_state: &mut ThreadList) -> Out {
    let ids = thread
        .iter()
        .filter(|character| character.archived())
        .map(|character_value| character_value.id.clone())
        .collect();

    start(Act::Unarchive, ids, view_state);
    Out::None
}

fn run_act_edit(thread: &[Card], view_state: &mut ThreadList) -> Out {
    view_state.form = Form::open(&thread[0].id, &view_state.cards, &view_state.roster);

    if view_state.form.is_none() {
        view_state.job_state.status = Some(("thread gone".into(), true, Instant::now()));
    }

    Out::None
}

/// ACT on IDS, off the UI thread (tests: at once).
fn start(act: Act, ids: Vec<String>, view_state: &mut ThreadList) {
    view_state.sel.clear();

    for id in &ids {
        let to = match act {
            Act::Archive => Some("archived".to_string()),
            Act::Unarchive => Some("open".to_string()),
            _ => None,
        };

        view_state.job_state.moved.insert(id.clone(), to);
    }

    let cards = view_state.cards.clone();

    view_state.set_cards(cards);

    // the next list (menu closed, opened again) starts from the moved cards too
    crate::infra::background::save("cards", &view_state.cards);

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
                Ok(thread_value) => ok.push(thread_value),
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
            (0, Some(error_value)) => (error_value.clone(), true),
            (count_value, Some(error_2)) => (
                format!("{count_value} threads {verb} · {} failed: {error_2}", errs.len()),
                true,
            ),
        }
    };

    if let ControlFlow::Break(()) = view_state.finish_start(act, work) {}
}

fn run_thread_list(state: &mut ThreadList, act: Act) -> Out {
    state.job_state.actions = None;
    let thread = state.targets();

    if thread.is_empty() || !state.acts().contains(&act) {
        return Out::None;
    }

    match act {
        Act::Open => Out::Open(Box::new(thread[0].clone())),
        Act::Edit => run_act_edit(&thread, state),
        Act::Unarchive => run_act_unarchive(&thread, state),
        Act::Archive => {
            let ids = thread
                .iter()
                .filter(|character_2| !character_2.archived())
                .map(|character_3| character_3.id.clone())
                .collect();

            state.job_state.pending = Some(Pending { act, ids });
            Out::None
        }
        Act::Delete => {
            state.job_state.pending = Some(Pending {
                act,
                ids: thread.iter().map(|character_4| character_4.id.clone()).collect(),
            });
            Out::None
        }
    }
}

fn acts_thread_list(state: &ThreadList) -> Vec<Act> {
    let thread = state.targets();

    if thread.is_empty() {
        return Vec::new();
    }

    let mut value = Vec::new();

    if thread.len() == 1 {
        value.push(Act::Open);
    }

    if thread.iter().any(|character| !character.archived()) {
        value.push(Act::Archive);
    }

    if thread.iter().any(Card::archived) {
        value.push(Act::Unarchive);
    }

    if thread.len() == 1 {
        value.push(Act::Edit);
    }

    value.push(Act::Delete);

    value
}

fn edit_thread_list(state: &mut ThreadList, id: String, error: crate::domain::threads::Edit) {
    let work = move || match crate::domain::threads::edit(&id, &error) {
        Ok(thread_value) => (thread_value, false),
        Err(error_value) => (error_value, true),
    };

    if cfg!(test) {
        let (thread_2, err) = work();

        state.done(thread_2, err);
        return;
    }

    let (tx, rx) = crate::infra::bus::channel();

    std::thread::spawn(move || {
        let _ = tx.send(work());
    });

    state.job_state.job = Some(rx);
    state.job_state.status = Some(("saving…".into(), false, Instant::now()));
}
