use super::{
    Act, AgentForm, App, ConfirmAction, HashSet, Job, KeyCode, KeyEvent, KeyModifiers, Modal, Outcome, Suspend, Tab,
    Worktree, agents, restart_note, trash,
};
use std::fmt::Write as _;
use std::ops::ControlFlow;

impl App {
    pub(super) fn run(&mut self, act: Act) {
        match act {
            Act::AddAlias => action_add_alias(self),
            Act::AgentFilter => action_agent_filter(self),
            Act::Thread(agent) => action_thread(self, agent),
            Act::ThreadFilter => action_thread_filter(self),
            Act::Filter => action_filter(self),
            Act::DeleteNote => {
                action_delete_note(self);
            }
            Act::EditNote => action_edit_note(self),
            Act::CopyPaths => {
                run_paths(self);
            }
            Act::ClearSelection => self.note_list.selected.clear(),
            Act::CollapseAll => action_collapse_all(self),
            Act::ExpandAll => {
                self.note_list.collapsed.clear();
                self.rebuild();
            }
            Act::Reload => reload_screen(self),
            // threads: the thread list asks and deletes; a worktree without thread: here
            Act::DeleteThreads if self.thread_list.tlist.view.cards() => action_delete_threads_rows(self),
            Act::DeleteThreads => {
                action_delete_threads(self);
            }
            Act::NewAgent => action_new_agent(self),
            Act::EditAgent => action_edit_agent(self),
            Act::DeleteAgent => {
                action_delete_agent(self);
            }
            Act::Start => self.start(),
            Act::EditPrompt => self.suspend = Some(Suspend::Prompt),
            Act::OpenNotes => self.open_notes(),
            Act::SaveDraft => self.save_draft(),
            // asked first: the reset drops picked notes and context
            Act::Reset => action_reset(self),
            Act::Quit => self.outcome = Some(Outcome::Quit),
        }
    }

    /// No to a confirm: a discard of the agent form goes back to the form.
    pub(super) fn declined(&mut self, action: ConfirmAction) {
        if let ConfirmAction::DiscardAgent(form) = action {
            self.overlays.modal = Some(Modal::Agent(form));
        }
    }

    pub(super) fn confirmed(&mut self, action: ConfirmAction) {
        match action {
            ConfirmAction::DiscardAgent(_) => {}
            ConfirmAction::ResetTask => {
                self.compose.target = None;
                self.compose.carry.clear();
                self.note_list.selected.clear();
                self.toast("reset: where row, 0 notes, 0 context", false);
            }
            ConfirmAction::Note(path) => match trash(&self.note_list.notes.abs(&path)) {
                Ok(how) => {
                    self.note_list
                        .selected
                        .retain(|text| !(text == &path || (path.ends_with('/') && text.starts_with(&path))));
                    self.reload_notes();
                    self.toast(format!("{path}: {how}"), false);
                }
                Err(error) => self.toast(format!("{path}: {error}"), true),
            },
            // teardown scripts, git, host stops: seconds; the report comes as a modal
            ConfirmAction::Threads(items) => {
                let repo = self.cfg.repo.clone().unwrap_or_else(|| self.cfg.cwd.clone());
                let logs = self.cfg.logs.clone();

                self.toast(format!("deleting {} threads…", items.len()), false);
                self.job(move || Job::Threads(crate::ui::worktree_preview::remove(&repo, &items, &logs)));
            }
            ConfirmAction::Agent(agent) => if let ControlFlow::Break(()) = confirmed_fallback(self, agent) {},
        }
    }

    pub(super) fn save_agent(&mut self, mut form: Box<AgentForm>) {
        let spec = form.spec(&self.agent_list.styles, self.agent_list.global_style.as_deref());

        if spec.name.is_empty() {
            form.error = Some("name required".into());
            form.field = 0;
            self.overlays.modal = Some(Modal::Agent(form));
            return;
        }

        let res = match &form.edit {
            Some(old) => agents::edit(old, &spec),
            None => agents::create(&spec),
        }
        .and_then(|()| agents::set_effort(form.effort_level()));

        match res {
            Ok(()) => {
                save_agent_old_name(self, &form, &spec);
            }
            Err(error) => {
                form.error = Some(error.lines().last().unwrap_or("failed").to_string());
                self.overlays.modal = Some(Modal::Agent(form));
            }
        }
    }
}

fn action_add_alias(app_state: &mut App) {
    if let Some(text_value) = app_state.settings.as_mut() {
        text_value.form.add_alias();
    }
}

fn action_agent_filter(app_state: &mut App) {
    app_state.switch_tab(Tab::Agents);
    app_state.tabs_focus = false;

    if app_state.agent_list.afilter.is_none() {
        app_state.agent_list.afilter = Some(String::new());
    }

    if app_state.agent_list.afilter_ta.text() != app_state.agent_list.afilter.as_deref().unwrap_or("") {
        app_state
            .agent_list
            .afilter_ta
            .set_text(app_state.agent_list.afilter.as_deref().unwrap_or(""));
    }

    app_state.agent_list.afilter_ta.move_end();
    app_state.agent_list.afilter_editing = true;
}

fn action_thread(app_state: &mut App, agent: crate::features::thread_list::Act) {
    app_state.switch_tab(Tab::Threads);
    app_state.tabs_focus = false;
    app_state.thread_list.tlist.focus = crate::features::thread_list::Focus::List;

    if let crate::features::thread_list::Out::Open(character) = app_state.thread_list.tlist.run(agent) {
        app_state.open_card(&character);
    }
}

fn action_thread_filter(app_state: &mut App) {
    app_state.switch_tab(Tab::Threads);
    app_state.tabs_focus = false;
    app_state.thread_list.tlist.focus = crate::features::thread_list::Focus::List;
    app_state.thread_list.tlist.filter.move_end();
    app_state.thread_list.tlist.filtering = true;
}

fn action_filter(app_state: &mut App) {
    app_state.open_notes();

    if app_state.note_list.filter.is_none() {
        app_state.note_list.filter = Some(String::new());
        app_state.note_list.cursor = 0;
        app_state.rebuild();
    }

    if app_state.note_list.filter_ta.text() != app_state.note_list.filter.as_deref().unwrap_or("") {
        app_state
            .note_list
            .filter_ta
            .set_text(app_state.note_list.filter.as_deref().unwrap_or(""));
    }

    app_state.note_list.filter_ta.move_end();
    app_state.note_list.filter_editing = true;
}

fn action_delete_note(app_state: &mut App) {
    let Some(row) = app_state.current() else {
        return;
    };

    let path = row.path().to_string();

    let question = if row.is_dir() {
        format!(
            "delete folder {path} with {} notes?",
            app_state.note_list.notes.inside(&path).len()
        )
    } else {
        format!("delete {path}?")
    };

    app_state.overlays.modal = Some(Modal::Confirm {
        title: "delete note".into(),
        question,
        yes: false,
        action: ConfirmAction::Note(path),
    });
}

fn action_edit_note(app_state: &mut App) {
    match app_state.current() {
        Some(result) if !result.is_dir() => {
            app_state.suspend = Some(Suspend::Note(app_state.note_list.notes.abs(result.path())));
        }
        _ => app_state.toast("move to a note first", true),
    }
}

fn action_collapse_all(app_state: &mut App) {
    app_state.note_list.collapsed = app_state
        .note_list
        .notes
        .tree(&HashSet::new())
        .iter()
        .filter(|result_2| result_2.is_dir())
        .map(|result_3| result_3.path().to_string())
        .collect();
    app_state.rebuild();
}

fn action_delete_threads_rows(app_state: &mut App) {
    app_state.switch_tab(Tab::Threads);
    app_state.tabs_focus = false;
    app_state
        .thread_list
        .tlist
        .run(crate::features::thread_list::Act::Delete);
}

fn action_delete_threads(app_state: &mut App) {
    let items: Vec<Worktree> = app_state
        .current_wt()
        .filter(|width| width.thread.is_none())
        .into_iter()
        .collect();

    if items.is_empty() {
        return;
    }

    let question = format!(
        "delete {} thread(s) + worktree(s)? uncommitted files → git stash",
        items.len()
    );

    app_state.overlays.modal = Some(Modal::Confirm {
        title: "delete threads".into(),
        question,
        yes: false,
        action: ConfirmAction::Threads(items),
    });
}

fn action_new_agent(app_state: &mut App) {
    let directory = app_state.default_style();

    app_state.overlays.modal = Some(Modal::Agent(Box::new(AgentForm::new(
        None,
        &app_state.agent_list.styles,
        directory.as_deref(),
    ))));
}

fn action_edit_agent(app_state: &mut App) {
    if let Some(agent_value) = app_state.current_agent().cloned() {
        let directory_value = app_state.default_style();

        app_state.overlays.modal = Some(Modal::Agent(Box::new(AgentForm::new(
            Some(agent_value),
            &app_state.agent_list.styles,
            directory_value.as_deref(),
        ))));
    }
}

fn action_delete_agent(app_state: &mut App) {
    let Some(agent_2) = app_state.current_agent().cloned() else {
        return;
    };

    if agent_2.name == app_state.cfg.agent {
        app_state.toast(
            format!(
                "{} is the default agent: threads of deleted agents go there",
                agent_2.name
            ),
            true,
        );
        return;
    }

    if !app_state
        .agent_list
        .agents
        .iter()
        .any(|x| x.name == app_state.cfg.agent)
    {
        app_state.toast(
            format!("default agent {} not found: 0 agents to take over", app_state.cfg.agent),
            true,
        );
        return;
    }

    let uses = app_state.uses(&agent_2);
    let live = uses.iter().filter(|user| !user.live.is_empty()).count();
    let question = format!(
        "delete {}? {} threads move to {} ({live} live: {} takes over now)",
        agent_2.name,
        uses.len(),
        app_state.cfg.agent,
        app_state.cfg.agent
    );

    app_state.overlays.modal = Some(Modal::Confirm {
        title: "delete agent".into(),
        question,
        yes: false,
        action: ConfirmAction::Agent(agent_2),
    });
}

fn action_reset(app_state: &mut App) {
    app_state.overlays.modal = Some(Modal::Confirm {
        title: "reset".into(),
        question: "reset the where row, drop the picked notes and context?".into(),
        yes: false,
        action: ConfirmAction::ResetTask,
    });
}

fn run_paths(app_state: &mut App) {
    let paths: Vec<String> = if app_state.note_list.selected.is_empty() {
        app_state
            .current()
            .map(|result_value| app_state.note_list.notes.inside(result_value.path()))
            .unwrap_or_default()
    } else {
        app_state.note_list.selected.clone()
    };

    let text = paths.iter().fold(String::new(), |mut output, path| {
        let _ = writeln!(output, "{}", app_state.note_list.notes.abs(path).display());

        output
    });

    if paths.is_empty() {
        app_state.toast("0 notes to copy", true);
    } else if crate::infra::clip::copy(&text) {
        app_state.toast(format!("copied {} path(s)", paths.len()), false);
    } else {
        app_state.toast("0 clipboard tools found (wl-copy, xclip, xsel)", true);
    }
}

fn confirmed_fallback(app_state: &mut App, agent: agents::Agent) -> ControlFlow<()> {
    let Some(fallback) = app_state
        .agent_list
        .agents
        .iter()
        .find(|x| x.name == app_state.cfg.agent)
        .cloned()
    else {
        return ControlFlow::Break(());
    };

    let uses = app_state.uses(&agent);
    let logs = app_state.cfg.logs.clone();

    app_state.toast(format!("deleting {}…", agent.name), false);
    app_state.job(move || Job::AgentDeleted(crate::ui::agent_actions::delete(&agent, &fallback, &uses, &logs)));

    ControlFlow::Continue(())
}

fn save_agent_old_name(app_state: &mut App, form: &AgentForm, spec: &agents::Spec) {
    let old_name = form.edit.as_ref().map(|agent| agent.name.clone());

    if let Some(old) = &old_name
        && let Some(path_value) = app_state.compose.picks.iter_mut().find(|path| *path == old)
    {
        path_value.clone_from(&spec.name);
    }

    app_state.load_agents();

    if let Some(i) = app_state
        .agent_list
        .agents
        .iter()
        .position(|agent_value| agent_value.name == spec.name)
    {
        app_state.agent_list.acursor = i;
    }

    let msg = format!("{} {}", spec.name, if old_name.is_some() { "saved" } else { "created" });

    app_state.toast(msg.clone(), false);

    if let Some(old) = form.edit.clone()
        && let Some(new) = agents::roster_only()
            .into_iter()
            .find(|agent_2| agent_2.name == spec.name)
        && agents::profile_changed(&old, &new)
    {
        save_agent_old_name_logs(app_state, msg, old, new);
    }
}

fn save_agent_old_name_logs(app_state: &mut App, msg: String, old: agents::Agent, new: agents::Agent) {
    let logs = app_state.cfg.logs.clone();
    let head = msg;

    app_state.job(move || {
        let uses = agents::usage().remove(&new.name).unwrap_or_default();
        let result = agents::restart(&new, old.harness == new.harness, &uses, &logs);

        Job::Restarted(format!("{head}{}", restart_note(&result)))
    });
}

fn run_body(app_state: &mut App) {
    app_state.settings_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL));
}

fn reload_screen(app_state: &mut App) {
    match app_state.tab {
        Tab::Threads => app_state.load_threads(),
        Tab::Merge => {
            let key = app_state.merge_tab.key(KeyEvent::from(KeyCode::Char('r')));

            return app_state.merge_out(key);
        }
        Tab::Agents => app_state.load_agents(),
        Tab::Notes | Tab::New => app_state.reload_notes(),
        Tab::Settings => {
            run_body(app_state);
            return;
        }
        Tab::Flows => app_state.flows_tab.enter(),
        Tab::Projects => app_state.enter_projects(),
    }

    app_state.toast("reloaded", false);
}
