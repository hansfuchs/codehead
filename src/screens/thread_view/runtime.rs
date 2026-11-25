use super::subscription;
use super::{Backend, Duration, Exit, Instant, Msg, Path, Peek, Receiver, Terminal, View, store, subscribe, take_pr};

pub(crate) fn run<B: Backend>(
    term: &mut Terminal<B>,
    id: &str,
    cwd: &Path,
    title: &str,
    note: Option<&str>,
    first: &str,
) -> Result<Exit, String> {
    run_with(term, id, cwd, title, note, first, None).map(|exit| exit.unwrap_or(Exit::Back))
}

/// Perf suite: the view of thread ID up to its 1st frame on TERM, as `run_with` opens it.
#[cfg(test)]
pub(crate) fn open_1st_frame<B: Backend>(term: &mut Terminal<B>, id: &str, cwd: &Path) -> Result<(), String> {
    let (tx, _rx) = crate::infra::bus::channel();
    let _sub = subscribe(id, tx.clone());
    let mut view = View::new(id, cwd, "", tx);

    view.save_prompt();
    crate::ui::thread_card::mark_seen_now(id);

    term.draw(|frame| view.draw(frame))
        .map(|_| ())
        .map_err(|error| error.to_string())
}

/// EXIT of a sub-thread's view (MAIN: the thread it runs for): back (esc esc) goes to MAIN, not
/// to the start screen.
pub(super) fn to_main(exit: Exit, main: Option<&str>) -> Exit {
    match (exit, main.and_then(store::load)) {
        (Exit::Back, Some(thread)) => Exit::Open {
            id: thread.id.clone(),
            dir: thread.work_dir(),
            title: thread.title.clone(),
            note: String::new(),
        },
        (exit, _) => exit,
    }
}

pub(super) struct Runtime {
    pub(super) id: String,
    pub(super) main: Option<String>,
    pub(super) kickoff: Option<Instant>,
    pub(super) view: View,
    pub(super) messages: Receiver<Msg>,
    pub(super) topics: Receiver<crate::infra::bus::Topic>,
    pub(super) opening: Option<Instant>,
    pub(super) drawn: Instant,
    pub(super) dirty: bool,
    pub(super) wake: bool,
    pub(super) flows_due: bool,
    pub(super) notes_version: u64,
    pub(super) count_second: u64,
    _subscription: subscription::Unsub,
}

pub(super) enum Step {
    Next,
    Exit(Option<Exit>),
}

pub(crate) fn run_with<B: Backend>(
    term: &mut Terminal<B>,
    id: &str,
    cwd: &Path,
    title: &str,
    note: Option<&str>,
    _first: &str,
    mut hook: Option<&mut dyn FnMut(&Peek) -> bool>,
) -> Result<Option<Exit>, String> {
    let mut runtime = Runtime::new(id, cwd, title, note)?;

    loop {
        match runtime.step(term, &mut hook)? {
            Step::Next => {}
            Step::Exit(exit) => return Ok(exit),
        }
    }
}

impl Runtime {
    fn new(id: &str, cwd: &Path, title: &str, note: Option<&str>) -> Result<Self, String> {
        let thread = store::load(id).ok_or_else(|| format!("no thread {id}"))?;
        let kickoff = thread.kickoff.then(Instant::now);
        let (tx, messages) = crate::infra::bus::channel();
        let subscription = subscribe(id, tx.clone());

        crate::engine::trace("view new …");
        let opened = Instant::now();
        let mut view = View::new(id, cwd, title, tx);

        crate::engine::trace("view new");
        prepare_view(&mut view, id, note);

        Ok(Self {
            id: id.to_string(),
            main: thread.of,
            kickoff,
            view,
            messages,
            topics: crate::infra::bus::subscribe(),
            opening: Some(opened),
            drawn: Instant::now().checked_sub(Duration::from_secs(1)).unwrap(),
            dirty: true,
            wake: true,
            flows_due: true,
            notes_version: crate::domain::notifications::version() + crate::infra::bus::version(),
            count_second: 0,
            _subscription: subscription,
        })
    }

    fn step<B: Backend>(
        &mut self,
        term: &mut Terminal<B>,
        hook: &mut Option<&mut dyn FnMut(&Peek) -> bool>,
    ) -> Result<Step, String> {
        self.notifications_changed();
        let every = self.frame_interval();

        self.draw_frame(term, every)?;

        if self.hook_done(hook) {
            return Ok(Step::Exit(None));
        }

        if self.read_keys(every)? {
            return Ok(Step::Exit(Some(self.exit())));
        }

        self.outside(term);

        if let Some(exit) = self.subthread_exit() {
            return Ok(Step::Exit(Some(exit)));
        }

        if self.view.handoff_due() || self.view.fork_due() {
            return Ok(Step::Exit(Some(self.exit())));
        }

        let pane = self.view.panes.pane;
        let _watch = crate::engine::watch(move || format!("view: tick {pane:?}"));

        self.poll();

        while let Ok(message) = self.messages.try_recv() {
            self.message(message);
        }

        self.refresh();

        Ok(Step::Next)
    }

    fn hook_done(&mut self, hook: &mut Option<&mut dyn FnMut(&Peek) -> bool>) -> bool {
        let Some(hook) = hook.as_mut() else {
            return false;
        };

        hook(&Peek {
            pane: self.view.panes.pane,
            menu: self.view.menu.is_some(),
            ide: std::mem::take(&mut self.view.ide.tutor_ide),
        })
    }

    fn exit(&mut self) -> Exit {
        crate::ui::thread_card::mark_seen_now(&self.id);
        to_main(std::mem::replace(&mut self.view.exit, Exit::Back), self.main.as_deref())
    }

    fn subthread_exit(&self) -> Option<Exit> {
        let main = self.main.as_deref()?;

        if store::thread_dir(&self.id).join("thread.json").is_file() {
            return None;
        }

        let thread = store::load(main)?;

        Some(Exit::Open {
            id: thread.id.clone(),
            dir: thread.work_dir(),
            title: thread.title.clone(),
            note: "the sub-thread ended: its result is in the stream".into(),
        })
    }
}

fn prepare_view(view: &mut View, id: &str, note: Option<&str>) {
    if let Some(pr) = take_pr(id) {
        view.show_pr(&pr);
    }

    if let Some(draft) = crate::domain::handoff::take_draft(id) {
        view.prompt.input.set_text(&draft);
        view.prompt.input.move_end();
    }

    if let Some(note) = note.filter(|note| !note.is_empty()) {
        view.toast(note.to_string());
    }

    view.save_prompt();
    crate::ui::thread_card::mark_seen_now(id);
}
