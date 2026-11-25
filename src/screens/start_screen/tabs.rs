use super::{App, KeyCode, KeyEvent, KeyModifiers, Screen, Tab, styles};

impl App {
    /// The tab the header shows as active.
    pub(super) fn cur_tab(&self) -> Tab {
        if self.compose.screen == Screen::Compose {
            Tab::New
        } else {
            self.tab
        }
    }

    pub(super) fn switch_tab(&mut self, tab: Tab) {
        self.repository.where_menu = None;
        self.project_selection.proj_menu = None;
        self.flow_selection.flow_menu = None;
        self.repository.repo_pick = None;

        // leaving the settings: their last edit applies
        if tab != Tab::Settings && self.compose.screen == Screen::Pick && self.tab == Tab::Settings {
            switch_tab_text(self);
        }

        if tab == Tab::New {
            self.compose.screen = Screen::Compose;
            self.load_task_flows();
            return;
        }

        self.compose.screen = Screen::Pick;

        if tab == Tab::Threads {
            self.refresh_threads();
        }

        if tab == Tab::Agents {
            self.refresh_agents();
        }

        if tab == Tab::Merge {
            self.merge_tab.ensure();
        }

        if tab == Tab::Flows {
            self.flows_tab.enter();
        }

        if tab == Tab::Projects {
            self.enter_projects();
        }

        if tab == Tab::Settings {
            self.settings_typed = false;
            self.tabs_focus = true;
        }

        if tab == Tab::Settings && self.settings.is_none() {
            self.settings = Some(crate::features::settings::Settings::new(self.settings_path.clone()));
        }

        self.tab = tab;
        self.note_list.filter_editing = false;
        self.preview.pscroll = 0;
    }

    pub(super) fn settings_action(&mut self, action: crate::features::settings::Action) {
        use crate::features::settings::Action;

        match action {
            Action::Header => self.tabs_focus = true,
            Action::None => {}
        }

        self.apply_settings();
    }

    /// Settings the form wrote to codehead.toml: live now.
    pub(super) fn apply_settings(&mut self) {
        let Some(file) = self
            .settings
            .as_mut()
            .and_then(crate::features::settings::Settings::take_applied)
        else {
            return;
        };

        {
            apply_settings_notes(self, file);
        }
    }

    pub(super) fn settings_key(&mut self, key: KeyEvent) {
        let action = self
            .settings
            .as_mut()
            .map_or(crate::features::settings::Action::None, |text| text.key(key));

        self.settings_action(action);
    }

    pub(super) fn preview_key(&mut self, key: KeyEvent) -> bool {
        if (self.note_list.filter_editing || self.thread_list.tlist.filtering || self.agent_list.afilter_editing)
            && !matches!(key.code, KeyCode::PageUp | KeyCode::PageDown)
        {
            return false;
        }

        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        let half = (self.preview.regions.preview.height / 2).max(1);

        match key.code {
            KeyCode::PageDown => self.preview.pscroll = self.preview.pscroll.saturating_add(half * 2),
            KeyCode::PageUp => self.preview.pscroll = self.preview.pscroll.saturating_sub(half * 2),
            KeyCode::Down if shift => self.preview.pscroll = self.preview.pscroll.saturating_add(1),
            KeyCode::Up if shift => self.preview.pscroll = self.preview.pscroll.saturating_sub(1),
            _ => return false,
        }

        true
    }

    pub(super) fn tabs_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Right | KeyCode::Char('l') => self.switch_tab(self.cur_tab().next()),
            KeyCode::Left | KeyCode::Char('h') => self.switch_tab(self.cur_tab().prev()),
            KeyCode::Down | KeyCode::Enter | KeyCode::Char('j') => {
                tabs_key_settings(self, key);
            }
            // esc: the top level already (esc esc quits); q: 0 quit (menu → quit codehead)
            KeyCode::Up | KeyCode::Esc => {}
            // any other key: the tab's own (its hotkeys, e.g. ctrl+f: filter), focus to the content.
            // Settings: 0 (a letter there would edit a field)
            _ if self.compose.screen == Screen::Compose => {
                self.tabs_focus = false;
                self.compose.field = self.fields()[0];
                self.compose_key(key);
            }
            _ if self.tab != Tab::Settings => {
                self.tabs_focus = false;
                self.pick_content_key(key);
            }
            _ => {}
        }
    }

    /// A key for the content of the current tab of the pick screen (0 tabs row focus).
    pub(super) fn pick_content_key(&mut self, key: KeyEvent) {
        match self.tab {
            Tab::Notes => self.notes_key(key),
            Tab::Threads => self.threads_key(key),
            Tab::Merge => {
                let output = self.merge_tab.key(key);

                self.merge_out(output);
            }
            Tab::Agents | Tab::New => self.agents_key(key),
            Tab::Settings => self.settings_key(key),
            Tab::Flows => {
                let output_value = self.flows_tab.key(key);

                self.flows_out(output_value);
            }
            Tab::Projects => {
                let output_2 = self.project_selection.projects_tab.key(key);

                self.projects_out(output_2);
            }
        }
    }
}

fn tabs_key_settings(app_state: &mut App, key: KeyEvent) {
    app_state.tabs_focus = false;

    if app_state.compose.screen == Screen::Compose {
        app_state.compose.field = app_state.fields()[0];
    } else if app_state.tab == Tab::Settings
        && let Some(settings) = &mut app_state.settings
    {
        settings.focus_input();
    }

    // the threads tab: its repo tabs first (↓↓: the view tabs, the list)
    if app_state.compose.screen == Screen::Pick && app_state.tab == Tab::Threads && key.code == KeyCode::Down {
        app_state.thread_list.tlist.focus = crate::features::thread_list::Focus::Repos;
    }
}

fn apply_settings_notes(app_state: &mut App, file: crate::infra::config::File) {
    // tests: 0 change to the process-wide config other tests read
    if !cfg!(test) {
        crate::infra::config::apply(file);
    }

    app_state.cfg.prefix = crate::infra::config::prefix();
    app_state.cfg.agent = crate::domain::agents::default_agent(&app_state.agent_list.agents).unwrap_or_default();
    let notes = crate::infra::config::notes();

    if app_state.cfg.notes != notes {
        app_state.cfg.notes = notes;
        app_state.note_list.selected.clear();
        app_state.reload_notes();
    }

    app_state.agent_list.styles = styles::load(&styles::dir());
    app_state.agent_list.global_style = styles::global_name();
}

fn switch_tab_text(app_state: &mut App) {
    if let Some(text) = app_state.settings.as_mut() {
        text.commit();
    }

    app_state.apply_settings();
}
