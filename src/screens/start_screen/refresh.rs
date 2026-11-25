use super::{
    App, Duration, HashMap, HashSet, Job, Modal, Notes, PathBuf, Screen, Tab, agents, background, claude, threads,
};

impl App {
    pub(super) fn rebuild(&mut self) {
        let keep = self
            .note_list
            .rows
            .get(self.note_list.cursor)
            .map(|result| result.path().to_string());

        self.note_list.rows = match &self.note_list.filter {
            Some(query) => self.note_list.notes.search(query),
            None => self.note_list.notes.tree(&self.note_list.collapsed),
        };

        if let Some(path) = keep
            && let Some(i) = self
                .note_list
                .rows
                .iter()
                .position(|result_value| result_value.path() == path)
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
            let j = run();

            self.job_done(j);
            return;
        }

        self.job_n += 1;
        self.jobs.request(self.job_n, run);
    }

    pub(super) fn job_done(&mut self, j: Job) {
        match j {
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
            Job::Restarted(msg) => {
                self.agent_list.usage = None;
                self.load_agents();
                self.toast(msg, false);
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

fn poll_controls(app_state: &mut App, dirty: &mut bool) {
    for (repo, character) in app_state.repository.where_reads.poll() {
        if app_state.cfg.repo.as_ref() != Some(&repo)
            || app_state
                .repository
                .repo
                .as_ref()
                .is_some_and(|result| result.url.is_some())
        {
            continue;
        }

        let picked = app_state.repository.wheres.get(app_state.repository.where_i).cloned();

        app_state.repository.wheres = character.wheres;
        app_state.repository.checkouts = character.checkouts;
        app_state.repository.repo_branch = character.repo_branch;
        app_state.repository.here_branch = character.here_branch;
        app_state.repository.here_dir = character.here_dir;
        app_state.repository.where_i = picked
            .and_then(|path| app_state.repository.wheres.iter().position(|width| *width == path))
            .unwrap_or(0);
        *dirty = true;
    }

    // the new-thread view opens: the plan windows start now, the task can take its time
    if app_state.compose.screen == Screen::Pick && app_state.tab == Tab::Merge {
        *dirty |= app_state.merge_tab.poll();

        if let Some(count) = app_state.merge_tab.take_note() {
            app_state.toast(count, false);
            *dirty = true;
        }
    }

    let compose = app_state.compose.screen == Screen::Compose && !app_state.cfg.copy_mode;

    if compose && !app_state.warmed {
        crate::engine::warm::warm();
    }

    app_state.warmed = compose;

    // avatars on screen (agents tab, agent form, picker, menu): a frame every 150 ms
    if matches!(app_state.overlays.modal, Some(Modal::Agent(_)))
        || app_state.cur_tab() == Tab::Agents
        || app_state.overlays.menu.is_some()
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
    if let Some(text) = app_state.settings.as_mut() {
        *dirty |= text.poll();
    }
}

fn poll_thread_list(app_state: &mut App, dirty: &mut bool) -> bool {
    let shown = app_state.compose.screen == Screen::Pick && app_state.tab == Tab::Threads;
    let busy = app_state.thread_list.tlist.waiting();

    app_state.thread_list.tlist.poll(shown);
    let menu_job = app_state
        .overlays
        .menu
        .as_mut()
        .is_some_and(|message| message.list.take_changed());

    if app_state.thread_list.tlist.take_changed() || menu_job {
        app_state.load_threads();
        app_state.thread_list.tlist.reload();
    }

    *dirty |= busy || app_state.thread_list.tlist.waiting();

    shown
}

fn poll_thread_rows(app_state: &mut App, dirty: &mut bool, shown: bool) {
    if app_state.thread_list.tlist.roster.len() != app_state.agent_list.agents.len() {
        app_state.thread_list.tlist.roster = app_state
            .agent_list
            .agents
            .iter()
            .map(|agent| agent.name.clone())
            .collect();
    }

    let keep = if app_state.thread_list.tlist.view.cards() {
        None
    } else {
        app_state
            .current_wt()
            .map(|width_value| width_value.key())
            .or_else(|| app_state.current_session().map(crate::infra::claude::Session::key))
    };

    let mut changed = false;

    for ((), rows) in app_state.thread_list.thread_reads.poll() {
        if app_state.thread_list.worktrees.as_ref() == Some(&rows) {
            continue;
        }

        app_state.thread_list.threads_version += 1;
        app_state.thread_list.worktrees = Some(rows);
        changed = true;
    }

    for ((), rows) in app_state.thread_list.session_reads.poll() {
        if app_state.thread_list.sessions == rows {
            continue;
        }

        app_state.thread_list.threads_version += 1;
        app_state.thread_list.sessions = rows;
        changed = true;
    }

    *dirty |= changed;

    if changed || (shown && app_state.thread_list.tlist.extra[1].0 != app_state.thread_list.sessions.len()) {
        // the thread list's tabs: the counts and repos of this screen's rows
        let wts = app_state.thread_list.worktrees.as_deref().unwrap_or(&[]);
        let orphans: Vec<Option<PathBuf>> = wts
            .iter()
            .filter(|width_2| width_2.thread.is_none())
            .map(|width_3| width_3.repo.clone())
            .collect();
        let sessions: Vec<Option<PathBuf>> = app_state
            .thread_list
            .sessions
            .iter()
            .map(|text_value| text_value.repo.clone())
            .collect();

        app_state.thread_list.tlist.extra = [(orphans.len(), orphans), (sessions.len(), sessions)];
    }

    if changed {
        poll_thread_rows_vis(app_state, keep);
    }
}

fn poll_thread_rows_vis(app_state: &mut App, keep: Option<String>) {
    let vis = app_state.tvisible();
    let nw = app_state.n_worktrees();

    app_state.thread_list.tcursor = keep
        .and_then(|key| poll_thread_rows_vis_body(app_state, &vis, nw, &key))
        .unwrap_or_else(|| app_state.thread_list.tcursor.min(vis.len().saturating_sub(1)));
}

fn poll_thread_rows_vis_body(app_state: &mut App, vis: &[usize], nw: usize, key: &str) -> Option<usize> {
    vis.iter().position(|&i| {
        if i < nw {
            app_state.thread_list.worktrees.as_ref().unwrap()[i].key() == key
        } else {
            app_state.thread_list.sessions[i - nw].key() == key
        }
    })
}

fn poll_agent_jobs(app_state: &mut App, dirty: &mut bool) {
    for (_, j) in app_state.jobs.poll() {
        *dirty = true;
        app_state.job_done(j);
    }

    for ((), (rows, effort, usage)) in app_state.agent_list.agent_reads.poll() {
        *dirty = true;
        let keep = app_state.current_agent().map(|agent_value| agent_value.name.clone());

        app_state.agent_list.agents = rows;
        app_state.agent_list.effort = effort;
        app_state.agent_list.usage = Some(usage);

        app_state.agent_list.acursor = keep
            .and_then(|name| {
                app_state
                    .agent_list
                    .agents
                    .iter()
                    .position(|agent_2| agent_2.name == name)
            })
            .unwrap_or_else(|| {
                app_state
                    .agent_list
                    .acursor
                    .min(app_state.agent_list.agents.len().saturating_sub(1))
            });

        let names: HashSet<String> = app_state
            .agent_list
            .agents
            .iter()
            .map(|agent_3| agent_3.name.clone())
            .collect();

        app_state.compose.picks.retain(|path_value| names.contains(path_value));

        if app_state.compose.picks.is_empty() && names.contains(&app_state.cfg.agent) {
            app_state.compose.picks.push(app_state.cfg.agent.clone());
        }
    }
}

fn poll_previews(app_state: &mut App, dirty: &mut bool) {
    let (details, extras) = (
        app_state.thread_list.detail_reads.poll(),
        app_state.thread_list.extra_reads.poll(),
    );

    *dirty |= !details.is_empty() || !extras.is_empty();
    app_state.thread_list.tpreview.extend(details);
    app_state.agent_list.apreview.extend(extras);

    for ((), notes) in app_state.note_list.note_reads.poll() {
        *dirty = true;
        app_state.note_list.notes = notes;
        app_state
            .note_list
            .selected
            .retain(|text_2| app_state.note_list.notes.files.contains(text_2));
        app_state.rebuild();
    }

    match app_state.cur_tab() {
        Tab::Threads => app_state.refresh_threads(),
        Tab::Agents => app_state.refresh_agents(),
        Tab::Notes => {
            let root = app_state.cfg.notes.clone();

            app_state.note_list.note_reads.request((), move || Notes::load(&root));
        }
        _ => {}
    }
}

fn poll_models(app_state: &mut App, dirty: &mut bool) {
    for (height, res) in app_state.model_catalog.model_reads.poll() {
        *dirty = true;

        match res {
            Ok(error) => {
                app_state.model_catalog.model_errors.remove(&height);
                app_state
                    .model_catalog
                    .models
                    .get_or_insert_with(HashMap::new)
                    .insert(height, error);
            }
            Err(error_value) => {
                app_state.model_catalog.model_errors.insert(height, error_value);
            }
        }
    }

    if app_state
        .overlays
        .toast
        .as_ref()
        .is_some_and(|thread| thread.1.elapsed() > Duration::from_secs(3))
    {
        app_state.overlays.toast = None;
        *dirty = true;
    }
}
