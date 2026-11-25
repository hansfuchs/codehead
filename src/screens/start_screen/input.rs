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
        if let Some(value) = key_overlays(self, key) {
            return value;
        }

        // @ and > dropdowns of the prompt: their keys first (esc closes them, 0 double esc)
        if key_prompt_controls(self, key) {
            return;
        }

        // a modal (confirm, agent form) takes its own Esc: 0 double esc over it
        if let Some(value) = key_escape(self, key, ctrl) {
            return value;
        }

        // F1-F4 (alt+1..4 too): the main tabs from every field (ctrl / alt+←→: word jumps); tab: the next field
        key_tabs(self, key, ctrl);
    }

    /// The regions of the screen in tab order: the tabs row, the settings' sub-tabs, the content,
    /// a PR's prompt (merge tab).
    pub(super) fn regions(&self) -> Vec<Region> {
        let mut result = vec![Region::Tabs];

        if self.compose.screen == Screen::Pick && self.tab == Tab::Settings {
            result.push(Region::SubTabs);
        }

        result.push(Region::Content);

        if self.compose.screen == Screen::Pick && self.tab == Tab::Merge && self.merge_tab.is_detail() {
            result.push(Region::Input);
        }

        result
    }

    pub(super) fn region(&self) -> Region {
        if self.tabs_focus {
            Region::Tabs
        } else if self.compose.screen == Screen::Pick
            && self.tab == Tab::Settings
            && self.settings.as_ref().is_some_and(|text| text.bar)
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

        let rs = self.regions();
        let at = rs.iter().position(|result| *result == self.region()).unwrap_or(0) as isize;
        let to = rs[(at + if back { -1 } else { 1 }).rem_euclid(rs.len() as isize) as usize];
        let settings = self.compose.screen == Screen::Pick && self.tab == Tab::Settings;

        if let Some(text) = self.settings.as_mut().filter(|_| settings) {
            cycle_body(to, text);
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

fn cycle_body(to: Region, text: &mut crate::features::settings::Settings) {
    text.commit();
    text.bar = to == Region::SubTabs;

    if to == Region::Content {
        text.focus_input();
    }
}

fn key_prompt_controls(app_state: &mut App, key: KeyEvent) -> bool {
    if app_state.mention_on() {
        // skills: of the picked agents' harnesses (0 picked: of every agent's)
        let picked = |agent_value: &&Agent| {
            app_state.compose.picks.is_empty() || app_state.compose.picks.contains(&agent_value.name)
        };

        let mut harnesses: Vec<String> = app_state
            .agent_list
            .agents
            .iter()
            .filter(picked)
            .map(|agent_2| agent_2.harness.clone())
            .collect();

        harnesses.sort();
        harnesses.dedup();
        app_state.compose.command.harnesses = harnesses;

        if app_state
            .compose
            .mention
            .key(key, &mut app_state.compose.prompt, &app_state.agent_list.agents)
            || app_state.compose.command.key(key, &mut app_state.compose.prompt)
            || app_state.compose.shortcuts.key(key, &mut app_state.compose.prompt)
            || app_state.compose.paths.key(key, &mut app_state.compose.prompt)
        {
            app_state.overlays.last_esc = None;
            return true;
        }
    }

    false
}

fn key_escape(app_state: &mut App, key: KeyEvent, ctrl: bool) -> Option<()> {
    if key.code == KeyCode::Esc && key.modifiers.is_empty() && app_state.overlays.modal.is_none() {
        if double_escape(app_state) {
            return Some(());
        }

        app_state.overlays.last_esc = Some((Instant::now(), app_state.at_start()));
    } else {
        app_state.overlays.last_esc = None;
    }

    if key.code == KeyCode::Esc
        && app_state.overlays.modal.is_none()
        && !app_state.tabs_focus
        && app_state.compose.screen == Screen::Pick
        && app_state.tab == Tab::Settings
        && app_state
            .settings
            .as_mut()
            .is_some_and(|text| text.suggestions_key(key))
    {
        if app_state.settings_typed {
            app_state.overlays.last_esc = None;
        }

        return Some(());
    }

    if app_state.compose.screen == Screen::Pick && app_state.tab == Tab::Settings && key.code != KeyCode::Esc {
        app_state.settings_typed = true;
    }

    if app_state.overlays.modal.is_some() {
        app_state.modal_key(key);
        return Some(());
    }

    // settings: Esc closes path suggestions. Typed ones: 0 double esc. Ones a path field
    // shows on focus by itself: the esc still counts for esc esc.

    if ctrl && matches!(key.code, KeyCode::Char('c')) {
        return {
            app_state.ctrl_c();
            Some(())
        };
    }

    None
}

fn key_escape_body(app_state: &mut App) {
    app_state.outcome = Some(Outcome::Quit);
}

fn key_tabs(app_state: &mut App, key: KeyEvent, ctrl: bool) {
    let main_tab = match key.code {
        KeyCode::F(count @ 1..=7) => Some(usize::from(count - 1)),
        // alt+1-7, ctrl+1-7 (kitty protocol), as the views of a thread
        KeyCode::Char(character @ '1'..='7') if key.modifiers.contains(KeyModifiers::ALT) != ctrl => {
            Some(usize::from(character as u8 - b'1'))
        }
        _ => None,
    };

    if !app_state.cfg.copy_mode
        && let Some(i) = main_tab
    {
        let to = MAIN_TABS[i];

        app_state.switch_tab(to);

        // the keys: in the tab's content; settings: on its tabs row (0 path dropdown at entry)
        if to != Tab::Settings {
            app_state.tabs_focus = false;
        }

        return;
    }

    // tab / shift+tab: the next / previous region (the tabs row, the tab's content and its
    // parts); arrows walk the fields inside
    if matches!(key.code, KeyCode::Tab | KeyCode::BackTab)
        && !ctrl
        && !key.modifiers.contains(KeyModifiers::ALT)
        && !app_state.cfg.copy_mode
        && app_state.cycle(key.code == KeyCode::BackTab)
    {
        return;
    }

    match app_state.compose.screen {
        Screen::Compose if app_state.tabs_focus => app_state.tabs_key(key),
        Screen::Compose => app_state.compose_key(key),
        Screen::Pick => {
            if app_state.tab == Tab::Settings && !app_state.tabs_focus {
                app_state.settings_key(key);
                return;
            }

            if app_state.tab == Tab::Flows && !app_state.tabs_focus {
                let output = app_state.flows_tab.key(key);

                app_state.flows_out(output);
                return;
            }

            if app_state.tab == Tab::Projects && !app_state.tabs_focus {
                let output_value = app_state.project_selection.projects_tab.key(key);

                app_state.projects_out(output_value);
                return;
            }

            if app_state.tab != Tab::Merge && app_state.preview_key(key) {
                return;
            }

            if app_state.tabs_focus {
                app_state.tabs_key(key);
            } else {
                app_state.pick_content_key(key);
            }
        }
    }
}

fn key_overlays(app_state: &mut App, key: KeyEvent) -> Option<()> {
    key_global_overlays(app_state, key).or_else(|| key_local_overlays(app_state, key))
}

fn key_global_overlays(app_state: &mut App, key: KeyEvent) -> Option<()> {
    if let Some(path) = app_state.overlays.notes_panel.as_mut() {
        let pick = path.key(key);

        return {
            app_state.notes_pick(pick);
            Some(())
        };
    }

    if crate::features::notifications::is_key(&key) {
        app_state.overlays.menu = None;
        app_state.overlays.notes_panel = Some(crate::features::notifications::Panel::open(None));
        return Some(());
    }

    if let Some(message) = app_state.overlays.menu.as_mut() {
        app_state.overlays.last_esc = None;
        let agent = message.key(key);

        return {
            app_state.menu_action(agent);
            Some(())
        };
    }

    if crate::infra::config::menu_key().matches(&key) {
        app_state.overlays.last_esc = None;
        return {
            app_state.open_menu();
            Some(())
        };
    }

    if let Some(message_value) = app_state.overlays.ctx.as_mut() {
        app_state.overlays.last_esc = None;
        let out = message_value.key(key);

        return {
            app_state.ctx_out(out);
            Some(())
        };
    }

    let threads = app_state.compose.screen == Screen::Pick && app_state.tab == Tab::Threads && !app_state.tabs_focus;

    if crate::ui::context_menu::is_key(&key) && !threads && app_state.overlays.modal.is_none() {
        app_state.overlays.last_esc = None;
        return {
            app_state.open_ctx();
            Some(())
        };
    }

    None
}

fn key_local_overlays(app_state: &mut App, key: KeyEvent) -> Option<()> {
    if app_state.draft_selection.drafts_menu.is_some() && app_state.overlays.modal.is_none() {
        app_state.overlays.last_esc = None;
        return {
            app_state.drafts_key(key);
            Some(())
        };
    }

    if app_state.repository.where_menu.is_some() && app_state.overlays.modal.is_none() {
        app_state.overlays.last_esc = None;
        return {
            app_state.where_key(key);
            Some(())
        };
    }

    if app_state.project_selection.proj_menu.is_some() && app_state.overlays.modal.is_none() {
        app_state.overlays.last_esc = None;
        return {
            app_state.proj_key(key);
            Some(())
        };
    }

    if app_state.flow_selection.flow_menu.is_some() && app_state.overlays.modal.is_none() {
        app_state.overlays.last_esc = None;
        return {
            app_state.flow_menu_key(key);
            Some(())
        };
    }

    if app_state.repository.repo_pick.is_some() && app_state.overlays.modal.is_none() {
        app_state.overlays.last_esc = None;
        return {
            app_state.repo_pick_key(key);
            Some(())
        };
    }

    // F2 menu: over everything, its keys first (1 esc closes it)

    // the context menu (ctrl+.): its keys first; the thread list has its own (actions)

    // drafts dropdown: its keys first (esc: back to the task, 0 double esc)

    // where dropdown: its keys first (esc closes it, 0 double esc)

    // flow dropdown (+ its steps): its keys first (esc closes 1 level, 0 double esc)

    // repo picker: its keys first (esc: filter off, then closed; 0 double esc)

    // the thread list's form / question / dropdown / filter: its keys first (esc closes it, 0
    // double esc)
    if app_state.compose.screen == Screen::Pick
        && app_state.tab == Tab::Threads
        && app_state.overlays.modal.is_none()
        && app_state.thread_list.tlist.modal()
    {
        // its esc: a 2nd esc soon after closes the other controls, 0 quit
        app_state.overlays.last_esc = (key.code == KeyCode::Esc).then(|| (Instant::now(), false));
        return {
            app_state.threads_key(key);
            Some(())
        };
    }

    None
}

fn double_escape(app_state: &mut App) -> bool {
    let Some((last, from_start)) = app_state.overlays.last_esc.take() else {
        return false;
    };

    if last.elapsed() >= DOUBLE_ESC {
        return false;
    }

    if from_start {
        key_escape_body(app_state);
    } else {
        app_state.dismiss_controls();
    }

    true
}
