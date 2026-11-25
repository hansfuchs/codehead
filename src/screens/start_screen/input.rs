use super::{
    Agent, App, DOUBLE_ESC, Field, Instant, KeyCode, KeyEvent, KeyModifiers, MAIN_TABS, Outcome, Region, Screen, Tab,
};

impl App {
    pub(crate) fn key(&mut self, key: KeyEvent) {
        self.key_inner(key);
        self.backup();
    }

    pub(super) fn key_inner(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

        // notifications (ctrl+n): over everything, the menu too
        if let Some(done) = key_overlays(self, key) {
            return done;
        }

        // @ and > dropdowns of the prompt: their keys first (esc closes them, 0 double esc)
        if key_prompt_controls(self, key) {
            return;
        }

        // a modal (confirm, agent form) takes its own Esc: 0 double esc over it
        if let Some(done) = key_escape(self, key, ctrl) {
            return done;
        }

        // F1-F4 (alt+1..4 too): the main tabs from every field (ctrl / alt+←→: word jumps); tab: the next field
        key_tabs(self, key, ctrl);
    }

    /// The regions of the screen in tab order: the tabs row, the settings' sub-tabs, the content,
    /// a PR's prompt (merge tab).
    pub(super) fn regions(&self) -> Vec<Region> {
        let mut regions = vec![Region::Tabs];

        if self.compose.screen == Screen::Pick && self.tab == Tab::Settings {
            regions.push(Region::SubTabs);
        }

        regions.push(Region::Content);

        if self.compose.screen == Screen::Pick && self.tab == Tab::Merge && self.merge_tab.is_detail() {
            regions.push(Region::Input);
        }

        regions
    }

    pub(super) fn region(&self) -> Region {
        if self.tabs_focus {
            Region::Tabs
        } else if self.compose.screen == Screen::Pick
            && self.tab == Tab::Settings
            && self.settings.as_ref().is_some_and(|settings| settings.bar)
        {
            Region::SubTabs
        } else if self.compose.screen == Screen::Pick && self.tab == Tab::Merge && self.merge_tab.input_focus {
            Region::Input
        } else {
            Region::Content
        }
    }

    /// Tab (BACK: shift+tab): the keys to the next / previous region. False: tab is the
    /// content's own here (the notes list, a flow being edited).
    pub(super) fn cycle(&mut self, back: bool) -> bool {
        if self.compose.screen == Screen::Pick
            && (self.tab == Tab::Notes || (self.tab == Tab::Flows && self.flows_tab.typing()))
        {
            return false;
        }

        let regions = self.regions();
        let at = regions.iter().position(|region| *region == self.region()).unwrap_or(0) as isize;
        let to = regions[(at + if back { -1 } else { 1 }).rem_euclid(regions.len() as isize) as usize];
        let settings = self.compose.screen == Screen::Pick && self.tab == Tab::Settings;

        if let Some(settings) = self.settings.as_mut().filter(|_| settings) {
            focus_settings_region(to, settings);
        }

        self.apply_settings();
        self.merge_tab.input_focus = to == Region::Input;
        self.tabs_focus = to == Region::Tabs;

        if to == Region::Content && self.compose.screen == Screen::Compose && self.compose.field == Field::Drafts {
            self.compose.field = self.fields()[0];
        }

        true
    }

    /// The start screen: 1 of its main tabs (task, threads, agents, settings), 0 local controls
    /// for an Esc to close (modal, filter input, thread filter, the notes list of the task).
    pub(super) fn at_start(&self) -> bool {
        let notes = self.compose.screen == Screen::Pick && self.tab == Tab::Notes;
        let pr_open = self.compose.screen == Screen::Pick
            && ((self.tab == Tab::Merge && (self.merge_tab.is_detail() || self.merge_tab.typing()))
                || (self.tab == Tab::Flows && self.flows_tab.typing())
                || (self.tab == Tab::Projects && self.project_selection.projects_tab.typing()));
        let threads = self.compose.screen == Screen::Pick && self.tab == Tab::Threads;
        let thread_controls = threads
            && (self.thread_list.tlist.modal()
                || !self.thread_list.tlist.query().is_empty()
                || !self.thread_list.tlist.sel.is_empty());

        self.overlays.modal.is_none()
            && self.draft_selection.drafts_menu.is_none()
            && self.repository.where_menu.is_none()
            && self.project_selection.proj_menu.is_none()
            && self.flow_selection.flow_menu.is_none()
            && self.repository.repo_pick.is_none()
            && !notes
            && !pr_open
            && !self.note_list.filter_editing
            && !self.agent_list.afilter_editing
            && !thread_controls
            && !(self.compose.screen == Screen::Pick
                && self.tab == Tab::Agents
                && self
                    .agent_list
                    .afilter
                    .as_deref()
                    .is_some_and(|query| !query.is_empty()))
    }

    /// Close local controls without changing the current tab.
    pub(super) fn dismiss_controls(&mut self) {
        self.overlays.modal = None;
        self.draft_selection.drafts_menu = None;
        self.repository.where_menu = None;
        self.project_selection.proj_menu = None;
        self.flow_selection.flow_menu = None;
        self.repository.repo_pick = None;
        self.note_list.filter_editing = false;
        self.thread_list.tlist.filtering = false;
        self.agent_list.afilter_editing = false;
        self.tabs_focus = false;
    }
}

fn focus_settings_region(to: Region, settings: &mut crate::features::settings::Settings) {
    settings.commit();
    settings.bar = to == Region::SubTabs;

    if to == Region::Content {
        settings.focus_input();
    }
}

fn key_prompt_controls(app: &mut App, key: KeyEvent) -> bool {
    if app.mention_on() {
        // skills: of the picked agents' harnesses (0 picked: of every agent's)
        let picked = |agent: &&Agent| app.compose.picks.is_empty() || app.compose.picks.contains(&agent.name);

        let mut harnesses: Vec<String> = app
            .agent_list
            .agents
            .iter()
            .filter(picked)
            .map(|agent| agent.harness.clone())
            .collect();

        harnesses.sort();
        harnesses.dedup();
        app.compose.command.harnesses = harnesses;

        if app
            .compose
            .mention
            .key(key, &mut app.compose.prompt, &app.agent_list.agents)
            || app.compose.command.key(key, &mut app.compose.prompt)
            || app.compose.shortcuts.key(key, &mut app.compose.prompt)
            || app.compose.paths.key(key, &mut app.compose.prompt)
        {
            app.overlays.last_esc = None;
            return true;
        }
    }

    false
}

fn key_escape(app: &mut App, key: KeyEvent, ctrl: bool) -> Option<()> {
    if key.code == KeyCode::Esc && key.modifiers.is_empty() && app.overlays.modal.is_none() {
        if double_escape(app) {
            return Some(());
        }

        app.overlays.last_esc = Some((Instant::now(), app.at_start()));
    } else {
        app.overlays.last_esc = None;
    }

    // settings: Esc closes path suggestions. Typed ones: 0 double esc. Ones a path field
    // shows on focus by itself: the esc still counts for esc esc.
    if key.code == KeyCode::Esc
        && app.overlays.modal.is_none()
        && !app.tabs_focus
        && app.compose.screen == Screen::Pick
        && app.tab == Tab::Settings
        && app
            .settings
            .as_mut()
            .is_some_and(|settings| settings.suggestions_key(key))
    {
        if app.settings_typed {
            app.overlays.last_esc = None;
        }

        return Some(());
    }

    if app.compose.screen == Screen::Pick && app.tab == Tab::Settings && key.code != KeyCode::Esc {
        app.settings_typed = true;
    }

    if app.overlays.modal.is_some() {
        app.modal_key(key);
        return Some(());
    }

    if ctrl && matches!(key.code, KeyCode::Char('c')) {
        return {
            app.ctrl_c();
            Some(())
        };
    }

    None
}

fn quit(app: &mut App) {
    app.outcome = Some(Outcome::Quit);
}

fn key_tabs(app: &mut App, key: KeyEvent, ctrl: bool) {
    let main_tab = match key.code {
        KeyCode::F(number @ 1..=7) => Some(usize::from(number - 1)),
        // alt+1-7, ctrl+1-7 (kitty protocol), as the views of a thread
        KeyCode::Char(number @ '1'..='7') if key.modifiers.contains(KeyModifiers::ALT) != ctrl => {
            Some(usize::from(number as u8 - b'1'))
        }
        _ => None,
    };

    if !app.cfg.copy_mode
        && let Some(i) = main_tab
    {
        let to = MAIN_TABS[i];

        app.switch_tab(to);

        // the keys: in the tab's content; settings: on its tabs row (0 path dropdown at entry)
        if to != Tab::Settings {
            app.tabs_focus = false;
        }

        return;
    }

    // tab / shift+tab: the next / previous region (the tabs row, the tab's content and its
    // parts); arrows walk the fields inside
    if matches!(key.code, KeyCode::Tab | KeyCode::BackTab)
        && !ctrl
        && !key.modifiers.contains(KeyModifiers::ALT)
        && !app.cfg.copy_mode
        && app.cycle(key.code == KeyCode::BackTab)
    {
        return;
    }

    match app.compose.screen {
        Screen::Compose if app.tabs_focus => app.tabs_key(key),
        Screen::Compose => app.compose_key(key),
        Screen::Pick => {
            if app.tab == Tab::Settings && !app.tabs_focus {
                app.settings_key(key);
                return;
            }

            if app.tab == Tab::Flows && !app.tabs_focus {
                let output = app.flows_tab.key(key);

                app.flows_out(output);
                return;
            }

            if app.tab == Tab::Projects && !app.tabs_focus {
                let output = app.project_selection.projects_tab.key(key);

                app.projects_out(output);
                return;
            }

            if app.tab != Tab::Merge && app.preview_key(key) {
                return;
            }

            if app.tabs_focus {
                app.tabs_key(key);
            } else {
                app.pick_content_key(key);
            }
        }
    }
}

fn key_overlays(app: &mut App, key: KeyEvent) -> Option<()> {
    key_global_overlays(app, key).or_else(|| key_local_overlays(app, key))
}

fn key_global_overlays(app: &mut App, key: KeyEvent) -> Option<()> {
    if let Some(panel) = app.overlays.notes_panel.as_mut() {
        let pick = panel.key(key);

        return {
            app.notes_pick(pick);
            Some(())
        };
    }

    if crate::features::notifications::is_key(&key) {
        app.overlays.menu = None;
        app.overlays.notes_panel = Some(crate::features::notifications::Panel::open(None));
        return Some(());
    }

    // F2 menu: over everything, its keys first (1 esc closes it)
    if let Some(menu) = app.overlays.menu.as_mut() {
        app.overlays.last_esc = None;
        let action = menu.key(key);

        return {
            app.menu_action(action);
            Some(())
        };
    }

    if crate::infra::config::menu_key().matches(&key) {
        app.overlays.last_esc = None;
        return {
            app.open_menu();
            Some(())
        };
    }

    // the context menu (ctrl+.): its keys first; the thread list has its own (actions)
    if let Some(ctx) = app.overlays.ctx.as_mut() {
        app.overlays.last_esc = None;
        let out = ctx.key(key);

        return {
            app.ctx_out(out);
            Some(())
        };
    }

    let threads = app.compose.screen == Screen::Pick && app.tab == Tab::Threads && !app.tabs_focus;

    if crate::ui::context_menu::is_key(&key) && !threads && app.overlays.modal.is_none() {
        app.overlays.last_esc = None;
        return {
            app.open_ctx();
            Some(())
        };
    }

    None
}

fn key_local_overlays(app: &mut App, key: KeyEvent) -> Option<()> {
    // drafts dropdown: its keys first (esc: back to the task, 0 double esc)
    if app.draft_selection.drafts_menu.is_some() && app.overlays.modal.is_none() {
        app.overlays.last_esc = None;
        return {
            app.drafts_key(key);
            Some(())
        };
    }

    // where dropdown: its keys first (esc closes it, 0 double esc)
    if app.repository.where_menu.is_some() && app.overlays.modal.is_none() {
        app.overlays.last_esc = None;
        return {
            app.where_key(key);
            Some(())
        };
    }

    if app.project_selection.proj_menu.is_some() && app.overlays.modal.is_none() {
        app.overlays.last_esc = None;
        return {
            app.proj_key(key);
            Some(())
        };
    }

    // flow dropdown (+ its steps): its keys first (esc closes 1 level, 0 double esc)
    if app.flow_selection.flow_menu.is_some() && app.overlays.modal.is_none() {
        app.overlays.last_esc = None;
        return {
            app.flow_menu_key(key);
            Some(())
        };
    }

    // repo picker: its keys first (esc: filter off, then closed; 0 double esc)
    if app.repository.repo_pick.is_some() && app.overlays.modal.is_none() {
        app.overlays.last_esc = None;
        return {
            app.repo_pick_key(key);
            Some(())
        };
    }

    // the thread list's form / question / dropdown / filter: its keys first (esc closes it, 0
    // double esc)
    if app.compose.screen == Screen::Pick
        && app.tab == Tab::Threads
        && app.overlays.modal.is_none()
        && app.thread_list.tlist.modal()
    {
        // its esc: a 2nd esc soon after closes the other controls, 0 quit
        app.overlays.last_esc = (key.code == KeyCode::Esc).then(|| (Instant::now(), false));
        return {
            app.threads_key(key);
            Some(())
        };
    }

    None
}

fn double_escape(app: &mut App) -> bool {
    let Some((last, from_start)) = app.overlays.last_esc.take() else {
        return false;
    };

    if last.elapsed() >= DOUBLE_ESC {
        return false;
    }

    if from_start {
        quit(app);
    } else {
        app.dismiss_controls();
    }

    true
}
