use super::{
    Act, AgentForm, App, ConfirmAction, HashSet, Job, KeyCode, KeyEvent, KeyModifiers, Modal, Outcome, Suspend, Tab,
    Worktree, agents, restart_note, trash,
};
use std::fmt::Write as _;

impl App {
    pub(super) fn run(&mut self, act: Act) {
        match act {
            Act::AddAlias => action_add_alias(self),
            Act::AgentFilter => action_agent_filter(self),
            Act::Thread(act) => action_thread(self, act),
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
                        .retain(|note| !(note == &path || (path.ends_with('/') && note.starts_with(&path))));
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
            ConfirmAction::Agent(agent) => delete_agent_to_fallback(self, agent),
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
                after_agent_saved(self, &form, &spec);
            }
            Err(error) => {
                form.error = Some(error.lines().last().unwrap_or("failed").to_string());
                self.overlays.modal = Some(Modal::Agent(form));
            }
        }
    }
}

fn action_add_alias(app: &mut App) {
    if let Some(settings) = app.settings.as_mut() {
        settings.form.add_alias();
    }
}

fn action_agent_filter(app: &mut App) {
    app.switch_tab(Tab::Agents);
    app.tabs_focus = false;

    if app.agent_list.afilter.is_none() {
        app.agent_list.afilter = Some(String::new());
    }

    if app.agent_list.afilter_ta.text() != app.agent_list.afilter.as_deref().unwrap_or("") {
        app.agent_list
            .afilter_ta
            .set_text(app.agent_list.afilter.as_deref().unwrap_or(""));
    }

    app.agent_list.afilter_ta.move_end();
    app.agent_list.afilter_editing = true;
}

fn action_thread(app: &mut App, act: crate::features::thread_list::Act) {
    app.switch_tab(Tab::Threads);
    app.tabs_focus = false;
    app.thread_list.tlist.focus = crate::features::thread_list::Focus::List;

    if let crate::features::thread_list::Out::Open(card) = app.thread_list.tlist.run(act) {
        app.open_card(&card);
    }
}

fn action_thread_filter(app: &mut App) {
    app.switch_tab(Tab::Threads);
    app.tabs_focus = false;
    app.thread_list.tlist.focus = crate::features::thread_list::Focus::List;
    app.thread_list.tlist.filter.move_end();
    app.thread_list.tlist.filtering = true;
}

fn action_filter(app: &mut App) {
    app.open_notes();

    if app.note_list.filter.is_none() {
        app.note_list.filter = Some(String::new());
        app.note_list.cursor = 0;
        app.rebuild();
    }

    if app.note_list.filter_ta.text() != app.note_list.filter.as_deref().unwrap_or("") {
        app.note_list
            .filter_ta
            .set_text(app.note_list.filter.as_deref().unwrap_or(""));
    }

    app.note_list.filter_ta.move_end();
    app.note_list.filter_editing = true;
}

fn action_delete_note(app: &mut App) {
    let Some(row) = app.current() else {
        return;
    };

    let path = row.path().to_string();

    let question = if row.is_dir() {
        format!(
            "delete folder {path} with {} notes?",
            app.note_list.notes.inside(&path).len()
        )
    } else {
        format!("delete {path}?")
    };

    app.overlays.modal = Some(Modal::Confirm {
        title: "delete note".into(),
        question,
        yes: false,
        action: ConfirmAction::Note(path),
    });
}

fn action_edit_note(app: &mut App) {
    match app.current() {
        Some(row) if !row.is_dir() => {
            app.suspend = Some(Suspend::Note(app.note_list.notes.abs(row.path())));
        }
        _ => app.toast("move to a note first", true),
    }
}

fn action_collapse_all(app: &mut App) {
    app.note_list.collapsed = app
        .note_list
        .notes
        .tree(&HashSet::new())
        .iter()
        .filter(|row| row.is_dir())
        .map(|row| row.path().to_string())
        .collect();
    app.rebuild();
}

fn action_delete_threads_rows(app: &mut App) {
    app.switch_tab(Tab::Threads);
    app.tabs_focus = false;
    app.thread_list.tlist.run(crate::features::thread_list::Act::Delete);
}

fn action_delete_threads(app: &mut App) {
    let items: Vec<Worktree> = app
        .current_wt()
        .filter(|worktree| worktree.thread.is_none())
        .into_iter()
        .collect();

    if items.is_empty() {
        return;
    }

    let question = format!(
        "delete {} thread(s) + worktree(s)? uncommitted files → git stash",
        items.len()
    );

    app.overlays.modal = Some(Modal::Confirm {
        title: "delete threads".into(),
        question,
        yes: false,
        action: ConfirmAction::Threads(items),
    });
}

fn action_new_agent(app: &mut App) {
    let default_style = app.default_style();

    app.overlays.modal = Some(Modal::Agent(Box::new(AgentForm::new(
        None,
        &app.agent_list.styles,
        default_style.as_deref(),
    ))));
}

fn action_edit_agent(app: &mut App) {
    if let Some(agent) = app.current_agent().cloned() {
        let default_style = app.default_style();

        app.overlays.modal = Some(Modal::Agent(Box::new(AgentForm::new(
            Some(agent),
            &app.agent_list.styles,
            default_style.as_deref(),
        ))));
    }
}

fn action_delete_agent(app: &mut App) {
    let Some(agent) = app.current_agent().cloned() else {
        return;
    };

    if agent.name == app.cfg.agent {
        app.toast(
            format!(
                "{} is the default agent: threads of deleted agents go there",
                agent.name
            ),
            true,
        );
        return;
    }

    if !app.agent_list.agents.iter().any(|x| x.name == app.cfg.agent) {
        app.toast(
            format!("default agent {} not found: 0 agents to take over", app.cfg.agent),
            true,
        );
        return;
    }

    let uses = app.uses(&agent);
    let live = uses.iter().filter(|entry| !entry.live.is_empty()).count();
    let question = format!(
        "delete {}? {} threads move to {} ({live} live: {} takes over now)",
        agent.name,
        uses.len(),
        app.cfg.agent,
        app.cfg.agent
    );

    app.overlays.modal = Some(Modal::Confirm {
        title: "delete agent".into(),
        question,
        yes: false,
        action: ConfirmAction::Agent(agent),
    });
}

fn action_reset(app: &mut App) {
    app.overlays.modal = Some(Modal::Confirm {
        title: "reset".into(),
        question: "reset the where row, drop the picked notes and context?".into(),
        yes: false,
        action: ConfirmAction::ResetTask,
    });
}

fn run_paths(app: &mut App) {
    let paths: Vec<String> = if app.note_list.selected.is_empty() {
        app.current()
            .map(|row| app.note_list.notes.inside(row.path()))
            .unwrap_or_default()
    } else {
        app.note_list.selected.clone()
    };

    let text = paths.iter().fold(String::new(), |mut output, path| {
        let _ = writeln!(output, "{}", app.note_list.notes.abs(path).display());

        output
    });

    if paths.is_empty() {
        app.toast("0 notes to copy", true);
    } else if crate::infra::clip::copy(&text) {
        app.toast(format!("copied {} path(s)", paths.len()), false);
    } else {
        app.toast("0 clipboard tools found (wl-copy, xclip, xsel)", true);
    }
}

fn delete_agent_to_fallback(app: &mut App, agent: agents::Agent) {
    let Some(fallback) = app.agent_list.agents.iter().find(|x| x.name == app.cfg.agent).cloned() else {
        return;
    };

    let uses = app.uses(&agent);
    let logs = app.cfg.logs.clone();

    app.toast(format!("deleting {}…", agent.name), false);
    app.job(move || Job::AgentDeleted(crate::ui::agent_actions::delete(&agent, &fallback, &uses, &logs)));
}

fn after_agent_saved(app: &mut App, form: &AgentForm, spec: &agents::Spec) {
    let old_name = form.edit.as_ref().map(|agent| agent.name.clone());

    if let Some(old) = &old_name
        && let Some(pick) = app.compose.picks.iter_mut().find(|pick| *pick == old)
    {
        pick.clone_from(&spec.name);
    }

    app.load_agents();

    if let Some(i) = app.agent_list.agents.iter().position(|agent| agent.name == spec.name) {
        app.agent_list.acursor = i;
    }

    let msg = format!("{} {}", spec.name, if old_name.is_some() { "saved" } else { "created" });

    app.toast(msg.clone(), false);

    if let Some(old) = form.edit.clone()
        && let Some(new) = agents::roster_only().into_iter().find(|agent| agent.name == spec.name)
        && agents::profile_changed(&old, &new)
    {
        restart_changed_agent(app, msg, old, new);
    }
}

fn restart_changed_agent(app: &mut App, msg: String, old: agents::Agent, new: agents::Agent) {
    let logs = app.cfg.logs.clone();
    let head = msg;

    app.job(move || {
        let uses = agents::usage().remove(&new.name).unwrap_or_default();
        let result = agents::restart(&new, old.harness == new.harness, &uses, &logs);

        Job::Restarted(format!("{head}{}", restart_note(&result)))
    });
}

fn reload_settings(app: &mut App) {
    app.settings_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL));
}

fn reload_screen(app: &mut App) {
    match app.tab {
        Tab::Threads => app.load_threads(),
        Tab::Merge => {
            let key = app.merge_tab.key(KeyEvent::from(KeyCode::Char('r')));

            return app.merge_out(key);
        }
        Tab::Agents => app.load_agents(),
        Tab::Notes | Tab::New => app.reload_notes(),
        Tab::Settings => {
            reload_settings(app);
            return;
        }
        Tab::Flows => app.flows_tab.enter(),
        Tab::Projects => app.enter_projects(),
    }

    app.toast("reloaded", false);
}
