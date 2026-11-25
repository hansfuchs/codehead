use super::app_state::{
    AgentListState, ComposeState, DraftSelectionState, FlowSelectionState, ModelCatalogState, NoteListState,
    OverlaysState, PreviewState, ProjectSelectionState, RepositoryState, ThreadListState,
};
use super::{
    Agent, Choices, Command, Config, Field, HashMap, HashSet, Mention, Notes, OutStyle, Paster, PathBuf, Previews,
    Reads, Rect, Regions, Screen, TextArea, Where, agents, background, drafts, quick_choices, styles, where_choices,
};

impl ComposeState {
    pub(super) fn new(cfg: &Config, title: TextArea, prompt: TextArea, picks: Vec<String>, paster: Paster) -> Self {
        ComposeState {
            paster,
            files: Vec::new(),
            mention: Mention::default(),
            command: Command::new(&cfg.cwd),
            paths: crate::ui::paths::Paths::new(&cfg.cwd),
            shortcuts: crate::ui::shortcuts::Pick::default(),
            carry: Vec::new(),
            picks,
            pick_cursor: 0,
            pick_offset: 0,
            pick_places: Vec::new(),
            screen: if cfg.copy_mode { Screen::Pick } else { Screen::Compose },
            title,
            prompt,
            field: Field::Prompt,
            target: None,
        }
    }
}

impl NoteListState {
    pub(super) fn new(notes: Notes) -> Self {
        NoteListState {
            notes,
            collapsed: HashSet::new(),
            selected: Vec::new(),
            rows: Vec::new(),
            cursor: 0,
            offset: 0,
            filter: None,
            filter_editing: false,
            filter_ta: TextArea::new(true, ""),
            note_reads: Reads::default(),
        }
    }
}

impl AgentListState {
    pub(super) fn new(agents: Vec<Agent>, styles: Vec<OutStyle>, global_style: Option<String>) -> Self {
        AgentListState {
            effort: agents::effort_name(),
            agent_reads: Reads::default(),
            agents,
            usage: background::snapshot("usage"),
            acursor: 0,
            aoffset: 0,
            afilter: None,
            afilter_editing: false,
            afilter_ta: TextArea::new(true, ""),
            aplace: Vec::new(),
            acards: Vec::new(),
            apreview: HashMap::new(),
            styles,
            global_style,
        }
    }
}

impl ThreadListState {
    pub(super) fn new(cfg: &Config) -> Self {
        ThreadListState {
            threads_version: 0,
            threads_fx: crate::ui::dissolve::Dissolve::default(),
            worktrees: Some(
                background::snapshot(&format!(
                    "threads-{}",
                    background::scope(cfg.repo.as_deref().unwrap_or(&cfg.cwd))
                ))
                .unwrap_or_default(),
            ),
            tlist: crate::features::thread_list::ThreadList::new(
                crate::features::thread_list::Mode::Manage,
                None,
                cfg.repo.clone(),
                Vec::new(),
            ),
            tcursor: 0,
            toffset: 0,
            tpreview: HashMap::new(),
            thread_reads: Reads::default(),
            session_reads: Reads::default(),
            detail_reads: Reads::default(),
            extra_reads: Reads::default(),
            sessions: background::snapshot("sessions").unwrap_or_default(),
        }
    }
}

impl RepositoryState {
    pub(super) fn new(
        cfg: &Config,
        choices: Choices,
        repo: Option<crate::infra::home::Repo>,
        repos: Vec<crate::infra::home::Repo>,
    ) -> Self {
        let Choices {
            wheres,
            checkouts,
            repo_branch,
            here_branch,
            here_dir,
        } = choices;
        let start_cwd = cfg.cwd.clone();

        let where_i = if cfg.branch.is_some() {
            wheres
                .iter()
                .position(|choice| *choice == Where::NewWorktree)
                .unwrap_or(0)
        } else {
            0
        };

        RepositoryState {
            repos,
            repo,
            repo_pick: None,
            where_reads: background::Reads::default(),
            start_cwd,
            wheres,
            where_i,
            where_menu: None,
            checkouts,
            repo_branch,
            here_branch,
            here_dir,
            checkout: true,
            stash: false,
            opt_i: 0,
        }
    }
}

impl FlowSelectionState {
    pub(super) fn new() -> Self {
        FlowSelectionState {
            agent_flows: std::collections::BTreeMap::default(),
            flow_default: None,
            flow_menu: None,
            flow_lib: crate::engine::flows::Library::default(),
        }
    }
}

impl ProjectSelectionState {
    pub(super) fn new() -> Self {
        ProjectSelectionState {
            projects_tab: crate::features::projects::ProjectsTab::default(),
            projects_area: Rect::default(),
            proj_cache: None,
            proj_picks: None,
            proj_menu: None,
        }
    }
}

impl ModelCatalogState {
    pub(super) fn new() -> Self {
        ModelCatalogState {
            models: None,
            model_reads: Reads::default(),
            model_errors: HashMap::new(),
            probe: if cfg!(test) {
                |_| Err("tests: 0 probe".into())
            } else {
                crate::infra::models::read
            },
        }
    }
}

impl DraftSelectionState {
    pub(super) fn new() -> Self {
        DraftSelectionState {
            draft_id: drafts::new_id(),
            draft_saved: String::new(),
            draft_kept: false,
            drafts: Vec::new(),
            drafts_menu: None,
            draft_btn: 0,
            draft_armed: None,
        }
    }
}

impl OverlaysState {
    pub(super) fn new() -> Self {
        OverlaysState {
            modal: None,
            menu: None,
            ctx: None,
            menu_cmds: Vec::new(),
            menu_help: Vec::new(),
            toast: None,
            notes_panel: None,
            last_esc: None,
            last_ctrl_c: None,
        }
    }
}

impl PreviewState {
    pub(super) fn new() -> Self {
        PreviewState {
            hover: None,
            last_frame: ratatui::buffer::Buffer::default(),
            psel: crate::ui::screen_select::ScreenSel::default(),
            psel_key: String::new(),
            previews: Previews::new(),
            pscroll: 0,
            regions: Regions::default(),
        }
    }
}

impl ComposeState {
    pub(super) fn load(cfg: &Config, title: &str, agents: &[Agent]) -> Self {
        let mut input = TextArea::new(true, "what the thread is about (also names the worktree)");

        input.set_text(title);
        let prompt = TextArea::new(false, "free text for the agents, sent below the list of notes");
        let mut picks: Vec<String> = cfg
            .picks
            .iter()
            .filter(|name| agents.iter().any(|agent| &agent.name == *name))
            .cloned()
            .collect();

        if picks.is_empty() && agents.iter().any(|agent| agent.name == cfg.agent) {
            picks.push(cfg.agent.clone());
        }

        let home = PathBuf::from(std::env::var("HOME").unwrap_or_default());
        let paster = Paster::new(
            home.join(".cache/codehead/paste")
                .join(format!("new-{}", crate::engine::now_ms() / 1000)),
        );

        Self::new(cfg, input, prompt, picks, paster)
    }
}

impl NoteListState {
    pub(super) fn load(cfg: &Config) -> Self {
        let notes = if cfg!(test) {
            Notes::load(&cfg.notes)
        } else {
            Notes::empty(&cfg.notes)
        };

        Self::new(notes)
    }
}

impl AgentListState {
    pub(super) fn load() -> (Self, Vec<String>) {
        let styles = styles::load(&styles::dir());
        let global_style = styles::global_name();
        let mut agents = agents::roster_only();
        let synced = agents::sync(&agents, &styles, global_style.as_deref());

        if !synced.is_empty() {
            agents = agents::roster_only();
        }

        (Self::new(agents, styles, global_style), synced)
    }
}

impl RepositoryState {
    pub(super) fn load(cfg: &Config) -> Self {
        let choices = cfg
            .repo
            .as_deref()
            .map(|repo| load_branch(cfg, repo))
            .unwrap_or_default();
        let repo = cfg.repo.as_ref().map(|path| crate::infra::home::Repo {
            name: path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default(),
            path: path.clone(),
            url: None,
        });

        let repos = if cfg!(test) {
            Vec::new()
        } else {
            crate::infra::home::repos()
        };

        Self::new(cfg, choices, repo, repos)
    }
}

fn load_branch(cfg: &Config, repo: &std::path::Path) -> Choices {
    if cfg!(test) {
        where_choices(repo, &cfg.cwd)
    } else {
        quick_choices(repo, &cfg.cwd)
    }
}
