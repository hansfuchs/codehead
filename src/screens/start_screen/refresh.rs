use super::{
    App, Duration, HashMap, HashSet, Job, Modal, Notes, PathBuf, Screen, Tab, agents, background, claude, threads,
};

impl App {
    pub(super) fn rebuild(&mut self) {
        let keep = self
            .note_list
            .rows
            .get(self.note_list.cursor)
            .map(|row| row.path().to_string());

        self.note_list.rows = match &self.note_list.filter {
            Some(query) => self.note_list.notes.search(query),
            None => self.note_list.notes.tree(&self.note_list.collapsed),
        };

        if let Some(path) = keep
            && let Some(i) = self.note_list.rows.iter().position(|row| row.path() == path)
        {
            self.note_list.cursor = i;
        }

        self.note_list.cursor = self.note_list.cursor.min(self.note_list.rows.len().saturating_sub(1));
    }

    pub(crate) fn reload_notes(&mut self) {
        self.note_list.note_reads.invalidate();
        let root = self.cfg.notes.clone();

        self.note_list.note_reads.request((), move || Notes::load(&root));
    }

    /// Return immediately with retained UI state. Fresh data arrives later.
    pub(crate) fn return_from_thread(&mut self) {
        self.outcome = None;
        self.overlays.last_esc = None;
        self.overlays.last_ctrl_c = None;
        self.preview.hover = None;
        self.thread_list.thread_reads.invalidate();
        self.thread_list.session_reads.invalidate();
        self.agent_list.agent_reads.invalidate();
        self.refresh_threads();
        self.refresh_agents();
    }

    pub(super) fn load_threads(&mut self) {
        self.thread_list.thread_reads.invalidate();
        self.thread_list.session_reads.invalidate();
        self.thread_list.detail_reads.invalidate();
        self.refresh_threads();
    }

    pub(super) fn refresh_threads(&mut self) {
        // tests: the reads give back the rows the test set (0 reads of the machine's threads)
        if cfg!(test) {
            let (rows, sessions) = (
                self.thread_list.worktrees.clone().unwrap_or_default(),
                self.thread_list.sessions.clone(),
            );

            self.thread_list.thread_reads.request((), move || rows);
            self.thread_list.session_reads.request((), move || sessions);
            return;
        }

        let repo = self.cfg.repo.clone();
        let name = format!(
            "threads-{}",
            background::scope(repo.as_deref().unwrap_or(&self.cfg.cwd))
        );

        self.thread_list.thread_reads.request((), move || {
            let rows = threads::load(repo.as_deref());

            background::save(&name, &rows);
            rows
        });

        self.thread_list.session_reads.request((), || {
            let rows = claude::load(40);

            background::save("sessions", &rows);
            rows
        });
    }

    pub(super) fn load_agents(&mut self) {
        self.agent_list.agent_reads.invalidate();
        self.thread_list.extra_reads.invalidate();
        self.refresh_agents();
    }

    /// Run 1-off action RUN off the UI thread; its Job comes back in `tick` (tests: at once).
    pub(super) fn job(&mut self, run: impl FnOnce() -> Job + Send + 'static) {
        if cfg!(test) {
            let job = run();

            self.job_done(job);
            return;
        }

        self.job_n += 1;
        self.jobs.request(self.job_n, run);
    }

    pub(super) fn job_done(&mut self, job: Job) {
        match job {
            Job::Threads(lines) => {
                self.load_threads();
                self.overlays.modal = Some(Modal::Info {
                    title: "delete threads".into(),
                    lines,
                    scroll: 0,
                });
            }
            Job::AgentDeleted(lines) => {
                self.load_agents();
                self.thread_list.worktrees = None;
                self.overlays.modal = Some(Modal::Info {
                    title: "delete agent".into(),
                    lines,
                    scroll: 0,
                });
            }
            Job::Restarted(message) => {
                self.agent_list.usage = None;
                self.load_agents();
                self.toast(message, false);
            }
        }
    }

    pub(super) fn refresh_agents(&mut self) {
        self.agent_list.agent_reads.request((), || {
            let rows = agents::load();
            let effort = agents::effort_name();
            let usage = agents::usage();

            background::save("usage", &usage);
            (rows, effort, usage)
        });
    }

    /// Background reads in. True: something on screen may change (a redraw is due).
    pub(crate) fn tick(&mut self) -> bool {
        let mut dirty = self.preview.previews.poll();

        // the where row's branches: the pick stays on its choice
        poll_controls(self, &mut dirty);

        // the thread list: its reads, its jobs (archive, delete, edit: this screen's rows again),
        // the menu's jobs too
        let shown = poll_thread_list(self, &mut dirty);

        poll_thread_rows(self, &mut dirty, shown);

        poll_agent_jobs(self, &mut dirty);

        poll_previews(self, &mut dirty);

        poll_models(self, &mut dirty);

        // the menu animates (working waves) and reads in the background; the > dropdown reads too
        dirty || self.overlays.menu.is_some() || self.compose.command.busy()
    }
}

fn poll_controls(app: &mut App, dirty: &mut bool) {
    for (repo, branches) in app.repository.where_reads.poll() {
        if app.cfg.repo.as_ref() != Some(&repo) || app.repository.repo.as_ref().is_some_and(|repo| repo.url.is_some()) {
            continue;
        }

        let picked = app.repository.wheres.get(app.repository.where_i).cloned();

        app.repository.wheres = branches.wheres;
        app.repository.checkouts = branches.checkouts;
        app.repository.repo_branch = branches.repo_branch;
        app.repository.here_branch = branches.here_branch;
        app.repository.here_dir = branches.here_dir;
        app.repository.where_i = picked
            .and_then(|old| app.repository.wheres.iter().position(|choice| *choice == old))
            .unwrap_or(0);
        *dirty = true;
    }

    // the new-thread view opens: the plan windows start now, the task can take its time
    if app.compose.screen == Screen::Pick && app.tab == Tab::Merge {
        *dirty |= app.merge_tab.poll();

        if let Some(note) = app.merge_tab.take_note() {
            app.toast(note, false);
            *dirty = true;
        }
    }

    let compose = app.compose.screen == Screen::Compose && !app.cfg.copy_mode;

    if compose && !app.warmed {
        crate::engine::warm::warm();
    }

    app.warmed = compose;

    // avatars on screen (agents tab, agent form, picker, menu): a frame every 150 ms
    if matches!(app.overlays.modal, Some(Modal::Agent(_)))
        || app.cur_tab() == Tab::Agents
        || app.overlays.menu.is_some()
    {
        use std::sync::atomic::{AtomicU64, Ordering};
        static LAST: AtomicU64 = AtomicU64::new(0);
        let ms = (crate::domain::avatar::now() * 1000.0) as u64;

        if ms.saturating_sub(LAST.load(Ordering::Relaxed)) >= 150 {
            LAST.store(ms, Ordering::Relaxed);
            *dirty = true;
        }
    }

    // the settings' MCP tab: harness lists, probes, sign-ins in the background
    if let Some(settings) = app.settings.as_mut() {
        *dirty |= settings.poll();
    }
}

fn poll_thread_list(app: &mut App, dirty: &mut bool) -> bool {
    let shown = app.compose.screen == Screen::Pick && app.tab == Tab::Threads;
    let busy = app.thread_list.tlist.waiting();

    app.thread_list.tlist.poll(shown);
    let menu_job = app.overlays.menu.as_mut().is_some_and(|menu| menu.list.take_changed());

    if app.thread_list.tlist.take_changed() || menu_job {
        app.load_threads();
        app.thread_list.tlist.reload();
    }

    *dirty |= busy || app.thread_list.tlist.waiting();

    shown
}

fn poll_thread_rows(app: &mut App, dirty: &mut bool, shown: bool) {
    if app.thread_list.tlist.roster.len() != app.agent_list.agents.len() {
        app.thread_list.tlist.roster = app.agent_list.agents.iter().map(|agent| agent.name.clone()).collect();
    }

    let keep = if app.thread_list.tlist.view.cards() {
        None
    } else {
        app.current_wt()
            .map(|worktree| worktree.key())
            .or_else(|| app.current_session().map(crate::infra::claude::Session::key))
    };

    let mut changed = false;

    for ((), rows) in app.thread_list.thread_reads.poll() {
        if app.thread_list.worktrees.as_ref() == Some(&rows) {
            continue;
        }

        app.thread_list.threads_version += 1;
        app.thread_list.worktrees = Some(rows);
        changed = true;
    }

    for ((), rows) in app.thread_list.session_reads.poll() {
        if app.thread_list.sessions == rows {
            continue;
        }

        app.thread_list.threads_version += 1;
        app.thread_list.sessions = rows;
        changed = true;
    }

    *dirty |= changed;

    if changed || (shown && app.thread_list.tlist.extra[1].0 != app.thread_list.sessions.len()) {
        // the thread list's tabs: the counts and repos of this screen's rows
        let worktrees = app.thread_list.worktrees.as_deref().unwrap_or(&[]);
        let orphans: Vec<Option<PathBuf>> = worktrees
            .iter()
            .filter(|worktree| worktree.thread.is_none())
            .map(|worktree| worktree.repo.clone())
            .collect();
        let sessions: Vec<Option<PathBuf>> = app
            .thread_list
            .sessions
            .iter()
            .map(|session| session.repo.clone())
            .collect();

        app.thread_list.tlist.extra = [(orphans.len(), orphans), (sessions.len(), sessions)];
    }

    if changed {
        restore_thread_cursor(app, keep);
    }
}

fn restore_thread_cursor(app: &mut App, keep: Option<String>) {
    let vis = app.tvisible();
    let nw = app.n_worktrees();

    app.thread_list.tcursor = keep
        .and_then(|key| visible_position_of(app, &vis, nw, &key))
        .unwrap_or_else(|| app.thread_list.tcursor.min(vis.len().saturating_sub(1)));
}

fn visible_position_of(app: &mut App, vis: &[usize], nw: usize, key: &str) -> Option<usize> {
    vis.iter().position(|&i| {
        if i < nw {
            app.thread_list.worktrees.as_ref().unwrap()[i].key() == key
        } else {
            app.thread_list.sessions[i - nw].key() == key
        }
    })
}

fn poll_agent_jobs(app: &mut App, dirty: &mut bool) {
    for (_, job) in app.jobs.poll() {
        *dirty = true;
        app.job_done(job);
    }

    for ((), (rows, effort, usage)) in app.agent_list.agent_reads.poll() {
        *dirty = true;
        let keep = app.current_agent().map(|agent| agent.name.clone());

        app.agent_list.agents = rows;
        app.agent_list.effort = effort;
        app.agent_list.usage = Some(usage);

        app.agent_list.acursor = keep
            .and_then(|name| app.agent_list.agents.iter().position(|agent| agent.name == name))
            .unwrap_or_else(|| {
                app.agent_list
                    .acursor
                    .min(app.agent_list.agents.len().saturating_sub(1))
            });

        let names: HashSet<String> = app.agent_list.agents.iter().map(|agent| agent.name.clone()).collect();

        app.compose.picks.retain(|pick| names.contains(pick));

        if app.compose.picks.is_empty() && names.contains(&app.cfg.agent) {
            app.compose.picks.push(app.cfg.agent.clone());
        }
    }
}

fn poll_previews(app: &mut App, dirty: &mut bool) {
    let (details, extras) = (app.thread_list.detail_reads.poll(), app.thread_list.extra_reads.poll());

    *dirty |= !details.is_empty() || !extras.is_empty();
    app.thread_list.tpreview.extend(details);
    app.agent_list.apreview.extend(extras);

    for ((), notes) in app.note_list.note_reads.poll() {
        *dirty = true;
        app.note_list.notes = notes;
        app.note_list
            .selected
            .retain(|file| app.note_list.notes.files.contains(file));
        app.rebuild();
    }

    match app.cur_tab() {
        Tab::Threads => app.refresh_threads(),
        Tab::Agents => app.refresh_agents(),
        Tab::Notes => {
            let root = app.cfg.notes.clone();

            app.note_list.note_reads.request((), move || Notes::load(&root));
        }
        _ => {}
    }
}

fn poll_models(app: &mut App, dirty: &mut bool) {
    for (harness, result) in app.model_catalog.model_reads.poll() {
        *dirty = true;

        match result {
            Ok(models) => {
                app.model_catalog.model_errors.remove(&harness);
                app.model_catalog
                    .models
                    .get_or_insert_with(HashMap::new)
                    .insert(harness, models);
            }
            Err(error) => {
                app.model_catalog.model_errors.insert(harness, error);
            }
        }
    }

    if app
        .overlays
        .toast
        .as_ref()
        .is_some_and(|toast| toast.1.elapsed() > Duration::from_secs(3))
    {
        app.overlays.toast = None;
        *dirty = true;
    }
}
