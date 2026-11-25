use super::{Duration, Instant, Pane, Path, PathBuf, Rect, SPLIT_MIN, View, json, store};

impl View {
    /// Switch to view PANE of the thread (the owner's pick: the view stops following the width).
    pub(super) fn set_pane(&mut self, pane: Pane) {
        self.layout.auto_pane = false;
        self.switch_pane(pane);
    }

    /// The view follows the width of AREA: wide (and a git checkout): changes + chat, the
    /// prompt has the keys; narrow: the chat.
    pub(super) fn follow_width(&mut self, area: Rect) {
        if !self.layout.auto_pane || !self.layout.split {
            return;
        }

        if self.checkout_state.checkouts.is_empty() {
            self.load_checkouts();
        }

        let wide = area.width >= SPLIT_MIN && !self.checkout_state.checkouts.is_empty();
        let want = if wide { Pane::Changes } else { Pane::Chat };

        if self.panes.pane != want && matches!(self.panes.pane, Pane::Chat | Pane::Changes) {
            self.switch_pane(want);

            // typing goes to the prompt; alt (or esc) gives the keys to the changes
            self.panes.prompt_focus = wide;
        }
    }

    /// Switch to view PANE. The changes view: git read at once, keys to the view.
    pub(super) fn switch_pane(&mut self, pane: Pane) {
        if self.panes.pane == pane {
            return;
        }

        // the search bar shows in the chat only: another view gets the keys
        if pane != Pane::Chat {
            self.find.focus = false;
        }

        self.panes.pane = pane;
        self.panes.prompt_focus = false;
        self.checkout_state.repo_focus = false;
        self.stream.dirty = true;

        if pane == Pane::Files {
            self.panes.files.refresh(true);
        }

        if pane == Pane::Changes {
            self.load_checkouts();
            self.panes.changes.refresh(true);
        }

        if pane == Pane::Merge {
            self.merge_sources(true);
        }
    }

    /// The merge tab's sources: the thread's checkouts (their repos' dirs), the PR links of its
    /// log (NOW: even when read less than 60 s ago).
    pub(super) fn merge_sources(&mut self, now: bool) {
        if !now
            && self
                .merge_state
                .merge_sources_at
                .is_some_and(|at| at.elapsed() < Duration::from_secs(60))
        {
            return;
        }

        self.merge_state.merge_sources_at = Some(Instant::now());
        let id = self.id.clone();

        let read = move || read_merge_sources(&id);

        // tests: at once
        if cfg!(test) {
            let sources = read();

            self.apply_merge_sources(sources);
            return;
        }

        if !self.merge_state.merge_reads.waiting(&()) {
            self.merge_state.merge_reads.request((), read);
        }
    }

    pub(super) fn apply_merge_sources(
        &mut self,
        (dirs, sources): (Vec<(String, PathBuf)>, crate::screens::merge::Sources),
    ) {
        for (repo, dir) in dirs {
            self.merge_state.merge.set_dir(&repo, dir);
        }

        self.merge_state.merge.set_sources(sources);
    }

    /// A finished sources read: in the merge tab. True: 1 came.
    pub(super) fn poll_merge_sources(&mut self) -> bool {
        let reads = self.merge_state.merge_reads.poll();
        let any = !reads.is_empty();

        for ((), sources) in reads {
            self.apply_merge_sources(sources);
        }

        any
    }

    /// The thread's checkouts (more of them: an agent edited in another repo / worktree).
    pub(super) fn load_checkouts(&mut self) {
        if let Some(thread) = store::load(&self.id) {
            set_checkouts(self, thread);
        }

        self.checkout_state.checkout = self
            .checkout_state
            .checkout
            .min(self.checkout_state.checkouts.len().saturating_sub(1));

        // 1 repo left: 0 repo tabs to step through
        if self.checkout_state.checkouts.len() < 2 {
            self.checkout_state.repo_focus = false;
        }

        // the changes view on the checkout picked (the view opened on another dir: a work dir
        // without git, the dir of the thread before)
        if let Some(dir) = self
            .checkout_state
            .checkouts
            .get(self.checkout_state.checkout)
            .filter(|dir| **dir != self.panes.changes.dir())
            .cloned()
        {
            self.panes.changes = crate::features::changes::View::new(&self.id, &dir);
            self.changes_agents();
            self.panes.changes.refresh(true);
            self.stream.dirty = true;
        }
    }

    /// The repo STEP tabs right (-1: left) of the picked one, wrapping.
    pub(super) fn step_checkout(&mut self, step: isize) {
        self.load_checkouts();
        let count = self.checkout_state.checkouts.len();

        self.checkout_state.checkout_tabs.i = self.checkout_state.checkout;

        if self.checkout_state.checkout_tabs.step(step, count) {
            self.pick_checkout(self.checkout_state.checkout_tabs.i);
        }
    }

    /// The changes view on checkout I.
    pub(super) fn pick_checkout(&mut self, i: usize) {
        if i == self.checkout_state.checkout || i >= self.checkout_state.checkouts.len() {
            return;
        }

        self.checkout_state.checkout = i;
        self.checkout_state.checkout_picked = true;
        let dir = self.checkout_state.checkouts[i].clone();
        let old = std::mem::replace(
            &mut self.panes.changes,
            crate::features::changes::View::new(&self.id, &dir),
        );

        self.panes.changes.focus_like(&old);
        self.changes_agents();
        self.panes.changes.refresh(true);
        self.stream.dirty = true;
    }

    /// Show PR in the merge tab.
    pub(crate) fn show_pr(&mut self, pr: &crate::infra::gh::PrRef) {
        self.set_pane(Pane::Merge);
        self.merge_state.merge.show(pr);
    }

    pub(super) fn merge_act(&mut self, act: crate::screens::merge::Act) {
        match act {
            crate::screens::merge::Act::None => {}
            crate::screens::merge::Act::Toast(toast) => self.toast(toast),
            crate::screens::merge::Act::Open(file, line) => self.open_at(&file, line),
            crate::screens::merge::Act::Prompt(text) => {
                self.stream.back = 0;
                let to = self.default_to().join(", ");

                self.send_prompt(&text);
                self.toast(format!("sent to {to}"));
            }
        }
    }

    pub(super) fn changes_act(&mut self, act: crate::features::changes::Act) {
        match act {
            crate::features::changes::Act::None => {}
            crate::features::changes::Act::Toast(toast) => self.toast(toast),
            crate::features::changes::Act::Open(file, line) => self.open_at(&file, line),
            crate::features::changes::Act::ToggleChat => {
                self.mini_chat_state.mini_chat = !self.mini_chat_state.mini_chat;
                self.stream.dirty = true;
            }
            // the ship section's agent steps: a sub-thread (the lead), this thread waits
            crate::features::changes::Act::Sub(task) => {
                self.stream.back = 0;
                self.start_sub(task);
            }
            crate::features::changes::Act::Send(text) => {
                self.stream.back = 0;
                let to = self.default_to().join(", ");

                self.send_prompt(&text);
                self.toast(format!("review comments sent to {to}"));
            }
            // an @agent in a comment / reply: to that agent now, the comment's thread along
            crate::features::changes::Act::Mention { to, text, context } => {
                self.stream.back = 0;
                self.bring_front(&to);
                self.prompt.pending.push((text.clone(), Instant::now(), None));
                self.host(
                    "send",
                    json!({"op": "prompt", "text": text, "to": to, "context": context}),
                );
                self.toast(format!("comment sent to {}", to.join(", ")));
            }
            crate::features::changes::Act::Checkout(root) => {
                self.load_checkouts();

                match self
                    .checkout_state
                    .checkouts
                    .iter()
                    .position(|dir| crate::features::changes::same_dir(dir, &root))
                {
                    Some(i) => self.pick_checkout(i),
                    None => self.toast(format!("{}: 0 checkout of this thread", root.display())),
                }
            }
        }
    }

    pub(super) fn files_act(&mut self, act: crate::features::files::Act) {
        match act {
            crate::features::files::Act::None => {}
            crate::features::files::Act::Toast(toast) => self.toast(toast),
            crate::features::files::Act::Open(path) => {
                let file = path.strip_prefix(&self.cwd).map_or_else(
                    |_| path.display().to_string(),
                    |relative| relative.display().to_string(),
                );

                self.open_at(&file, 1);
            }
            crate::features::files::Act::Yazi(path) => self.panes.yazi = Some(path),
        }
    }

    /// yazi's pick (the chooser file): its paths into the prompt.
    pub(super) fn yazi_picked(&mut self, paths: &[String]) {
        if paths.is_empty() {
            return;
        }

        let shown: Vec<String> = paths
            .iter()
            .map(|path| {
                Path::new(path)
                    .strip_prefix(&self.cwd)
                    .map_or_else(|_| path.clone(), |relative| relative.display().to_string())
            })
            .collect();

        self.prompt.input.insert(&format!("{} ", shown.join(" ")));
        self.toast(format!("yazi: {} path(s) in the prompt", shown.len()));
    }

    /// The IDE (setting `editor`) at <FILE:LINE> of the worktree (VS Code: the window that has it open).
    pub(super) fn open_at(&mut self, file: &str, line: u32) {
        let cwd = self.cwd.clone();

        self.open_editor(
            |key| crate::infra::editor::args_at(key, &cwd, file, line),
            Some((file, line)),
            format!("{file}:{line}"),
        );
    }

    /// portless: read this thread's live URLs every 5 s (on a thread). 0 portless, 0 worktree: 0.
    pub(super) fn poll_urls(&mut self) {
        if let Some(rx) = &self.urls_state.urls_read
            && let Ok(urls) = rx.try_recv()
        {
            self.urls_state.urls_read = None;
            self.urls_state.urls = urls;
        }

        if cfg!(test)
            || self.urls_state.urls_read.is_some()
            || self
                .urls_state
                .urls_at
                .is_some_and(|at| at.elapsed() < Duration::from_secs(5))
        {
            return;
        }

        self.urls_state.urls_at = Some(Instant::now());

        let Some(prefix) = store::load(&self.id)
            .filter(|thread| thread.worktree.is_some())
            .and_then(|thread| thread.branch)
            .map(|branch| crate::infra::portless::prefix(&branch))
        else {
            return;
        };

        if !crate::infra::portless::active() {
            return;
        }

        let (tx, rx) = crate::infra::bus::channel();

        std::thread::spawn(move || {
            let _ = tx.send(crate::infra::portless::urls_of(
                &crate::infra::portless::routes(),
                &prefix,
            ));
        });

        self.urls_state.urls_read = Some(rx);
    }
}

fn read_merge_sources(id: &str) -> (Vec<(String, PathBuf)>, crate::screens::merge::Sources) {
    let Some(thread) = store::load(id) else {
        return (Vec::new(), crate::screens::merge::Sources::default());
    };

    let checkouts: Vec<(PathBuf, Option<String>)> = thread
        .all_checkouts()
        .into_iter()
        .map(|checkout| (PathBuf::from(checkout.dir), checkout.branch))
        .collect();
    let dirs = checkouts
        .iter()
        .filter_map(|(dir, _)| crate::infra::gh::repo_of_dir(dir).map(|repo| (repo, dir.clone())))
        .collect();
    let log = std::fs::read_to_string(store::events_path(id)).unwrap_or_default();

    (
        dirs,
        crate::screens::merge::Sources {
            checkouts,
            links: crate::infra::gh::pr_urls(&log),
        },
    )
}

fn set_checkouts(view: &mut View, thread: store::Thread) {
    let thread = crate::engine::checkouts::prune(&thread).unwrap_or(thread);
    let mut all: Vec<store::Checkout> = thread
        .all_checkouts()
        .into_iter()
        .filter(|checkout| Path::new(&checkout.dir).is_dir())
        .collect();

    // a work dir without git (codehead started outside of a repo), agents in a repo: the
    // changes of the repos only
    if all
        .iter()
        .any(|checkout| crate::domain::threads::repo_root(Path::new(&checkout.dir)).is_some())
    {
        all.retain(|checkout| crate::domain::threads::repo_root(Path::new(&checkout.dir)).is_some());
    }

    let name = |checkout: &store::Checkout| {
        let dir = Path::new(&checkout.dir);
        let root = crate::domain::threads::repo_root(dir).unwrap_or_else(|| dir.to_path_buf());

        root.file_name().map_or_else(
            || checkout.dir.clone(),
            |file_name| file_name.to_string_lossy().into_owned(),
        )
    };

    let names: Vec<String> = all.iter().map(name).collect();

    // 2 checkouts of 1 repo: the branch tells them apart
    view.checkout_state.checkout_labels = all
        .iter()
        .zip(&names)
        .map(|(checkout, name)| match &checkout.branch {
            Some(branch) if names.iter().filter(|other| *other == name).count() > 1 => {
                format!("{name} · {branch}")
            }
            _ => name.clone(),
        })
        .collect();

    // an agent moved the work to a worktree of the work dir's repo (`git worktree add`):
    // the newest such worktree is the thread's work, unless the owner picked a checkout
    // only from the main checkout to a linked worktree: a thread in its own worktree stays
    // there when an agent edits the main checkout (git writes a worktree's gitdir as a
    // real path: /var → /private/var on macOS)
    let work = thread.work_dir();
    let canon = |path: &Path| std::fs::canonicalize(path).ok();
    let real = |path: &Path| crate::domain::threads::repo_root(path).and_then(|root| canon(&root));
    let main = |path: &Path| real(path).is_some_and(|root| Some(root) == canon(path));
    let repo = real(&work);

    // (a submodule's root follows to the superproject too: linked worktrees only)
    let moved = main(&work)
        .then(|| {
            all.iter().rposition(|checkout| {
                crate::domain::threads::linked_worktree(Path::new(&checkout.dir))
                    && real(Path::new(&checkout.dir)) == repo
            })
        })
        .flatten();

    if !view.checkout_state.checkout_picked {
        view.checkout_state.checkout = moved.unwrap_or(0);
    }

    view.checkout_state.checkouts = all.into_iter().map(|checkout| PathBuf::from(checkout.dir)).collect();
}
