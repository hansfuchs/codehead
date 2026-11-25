use super::app_state::{
    AgentListState, App, ComposeState, DraftSelectionState, FlowSelectionState, ModelCatalogState, NoteListState,
    OverlaysState, PreviewState, ProjectSelectionState, RepositoryState, ThreadListState,
};
use super::{Config, PathBuf, Reads, Rect, Tab, drafts, where_choices};

impl App {
    pub(crate) fn new(cfg: Config, title: &str) -> App {
        let (agent_list, synced) = AgentListState::load();
        let mut app = App {
            warmed: false,
            merge_tab: crate::screens::merge::merge_tab::MergeTab::default(),
            flows_tab: crate::features::flows::FlowsTab::new(Some(cfg.cwd.clone())),
            flows_area: Rect::default(),
            merge_area: Rect::default(),
            tab: if cfg.copy_mode { Tab::Notes } else { Tab::New },
            settings: None,
            tabs_focus: false,
            jobs: Reads::default(),
            job_n: 0,
            settings_path: settings_path(),
            outcome: None,
            suspend: None,
            settings_typed: false,
            compose: ComposeState::load(&cfg, title, &agent_list.agents),
            note_list: NoteListState::load(&cfg),
            agent_list,
            thread_list: ThreadListState::new(&cfg),
            repository: RepositoryState::load(&cfg),
            flow_selection: FlowSelectionState::new(),
            project_selection: ProjectSelectionState::new(),
            model_catalog: ModelCatalogState::new(),
            draft_selection: DraftSelectionState::new(),
            overlays: OverlaysState::new(),
            preview: PreviewState::new(),
            cfg,
        };

        finish_setup(&mut app, &synced);
        app
    }

    /// Style for new agents: the global outputStyle, else mecha, else none.
    pub(super) fn default_style(&self) -> Option<String> {
        let group = self.agent_list.global_style.as_deref();

        self.agent_list
            .styles
            .iter()
            .find(|text| Some(text.name.as_str()) == group)
            .or_else(|| {
                self.agent_list
                    .styles
                    .iter()
                    .find(|text_value| text_value.id == "mecha")
            })
            .map(|text_2| text_2.id.clone())
    }
}

fn settings_path() -> PathBuf {
    if cfg!(test) {
        std::env::temp_dir().join(format!(
            "codehead-settings-app-{}-{:?}.toml",
            std::process::id(),
            std::thread::current().id()
        ))
    } else {
        crate::infra::config::path()
    }
}

fn finish_setup(app_state: &mut App, synced: &[String]) {
    app_state.rebuild();
    app_state.draft_selection.draft_saved = app_state.draft().content();
    app_state.draft_selection.drafts = drafts::load(&app_state.cfg.drafts, &app_state.draft_selection.draft_id);

    if !synced.is_empty() {
        app_state.toast(
            format!("prompts synced with output-styles: {}", synced.join(", ")),
            false,
        );
    }

    app_state.refresh_threads();
    app_state.refresh_agents();

    if cfg!(test) {
        return;
    }

    app_state.reload_notes();

    if let Some(repo) = app_state.cfg.repo.clone() {
        let cwd = app_state.cfg.cwd.clone();

        app_state
            .repository
            .where_reads
            .request(repo.clone(), move || where_choices(&repo, &cwd));
    }
}
