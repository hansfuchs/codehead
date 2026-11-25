use super::{
    Agent, AgentSnapshot, Choices, Cmd, Command, Config, Draft, Field, FlowMenu, HashMap, HashSet, Instant, Job, Line,
    Mention, Modal, Notes, OutStyle, Outcome, Paster, PathBuf, Position, Previews, Reads, Rect, Regions, Row, Screen,
    Session, StartCtx, Suspend, Tab, Target, TextArea, Use, Where, WhereMenu, Worktree, background,
};

pub(crate) struct App {
    pub cfg: Config,
    /// The merge tab: the owner's pull requests, 1 open.
    pub(super) merge_tab: crate::screens::merge::merge_tab::MergeTab,
    /// The flows tab.
    pub(super) flows_tab: crate::features::flows::FlowsTab,
    pub(super) flows_area: Rect,
    pub(super) merge_area: Rect,
    /// The new-thread view was open at the last tick (its windows warmed up: `engine::warm`).
    pub(super) warmed: bool,
    pub(super) tab: Tab,
    pub(super) settings: Option<crate::features::settings::Settings>,
    pub(super) tabs_focus: bool,
    /// 1-off actions off the UI thread (thread delete, agent delete, session restarts): their
    /// results, 1 key per job.
    pub(super) jobs: Reads<u64, Job>,
    pub(super) job_n: u64,
    /// codehead.toml of the settings tab (tests: a temp file, never the real one).
    pub(super) settings_path: PathBuf,
    pub outcome: Option<Outcome>,
    pub suspend: Option<Suspend>,
    /// A key or paste (not esc) in the settings tab since it opened: its path suggestions came
    /// from typing, so their esc does not count for esc esc.
    pub(super) settings_typed: bool,
    pub(super) compose: ComposeState,
    pub(super) note_list: NoteListState,
    pub(super) agent_list: AgentListState,
    pub(super) thread_list: ThreadListState,
    pub(super) repository: RepositoryState,
    pub(super) flow_selection: FlowSelectionState,
    pub(super) project_selection: ProjectSelectionState,
    pub(super) model_catalog: ModelCatalogState,
    pub(super) draft_selection: DraftSelectionState,
    pub(super) overlays: OverlaysState,
    pub(super) preview: PreviewState,
}

pub(super) struct ComposeState {
    /// Context: last responses (Claude Code sessions, threads) that go into the thread message.
    pub(super) carry: Vec<Session>,
    pub(super) picks: Vec<String>,
    pub(super) pick_cursor: usize,
    pub(super) pick_offset: usize,
    pub(super) pick_places: Vec<(usize, u16, usize)>,
    pub(super) screen: Screen,
    pub(super) title: TextArea,
    pub(super) prompt: TextArea,
    pub(super) field: Field,
    pub(super) target: Option<Target>,
    /// Prompt pastes: images, long texts (files, listed in the message).
    pub(super) paster: Paster,
    pub(super) files: Vec<(String, PathBuf)>,
    pub(super) mention: Mention,
    /// `>` dropdown of the prompt: commands (mention a file / note / thread / artifact).
    pub(super) command: Command,
    /// Path dropdown of the prompt: the entries of the directory a typed path names.
    pub(super) paths: crate::ui::paths::Paths,
    /// `#` of the prompt: the shortcuts.
    pub(super) shortcuts: crate::ui::shortcuts::Pick,
}

pub(super) struct NoteListState {
    pub(super) notes: Notes,
    pub(super) collapsed: HashSet<String>,
    pub(super) selected: Vec<String>,
    pub(super) rows: Vec<Row>,
    pub(super) cursor: usize,
    pub(super) offset: usize,
    pub(super) filter: Option<String>,
    pub(super) filter_editing: bool,
    /// Input of the notes filter (undo / redo, cursor); FILTER holds its text.
    pub(super) filter_ta: TextArea,
    pub(super) note_reads: Reads<(), Notes>,
}

pub(super) struct AgentListState {
    pub(super) agent_reads: Reads<(), AgentSnapshot>,
    pub(super) agents: Vec<Agent>,
    pub(super) usage: Option<HashMap<String, Vec<Use>>>,
    pub(super) acursor: usize,
    /// Agents tab: first virtual row of the card grid in view.
    pub(super) aoffset: usize,
    /// Agents tab filter (fuzzy: name, harness, model). Some = row shows text.
    pub(super) afilter: Option<String>,
    pub(super) afilter_editing: bool,
    pub(super) afilter_ta: TextArea,
    /// Agents tab, last draw: every card (agent, x, virtual row) for ↑↓←→, visible cards for clicks.
    pub(super) aplace: Vec<(usize, u16, usize)>,
    pub(super) acards: Vec<(Rect, usize)>,
    pub(super) apreview: HashMap<String, Vec<String>>,
    pub(super) styles: Vec<OutStyle>,
    pub(super) global_style: Option<String>,
    /// Agent cards per row (last draw), for ↑↓.
    /// Global effort (~/.claude/settings.json effortLevel), for the cards.
    pub(super) effort: String,
}

pub(super) struct ThreadListState {
    /// Threads tab rows: every thread (preview data), worktrees without a thread.
    pub(super) worktrees: Option<Vec<Worktree>>,
    /// Threads tab: the thread list (cards, repo + view tabs, filter, actions); its views
    /// `worktrees` / `claude` show this screen's rows (cursor, 1st row in view).
    pub(super) tlist: crate::features::thread_list::ThreadList,
    pub(super) tcursor: usize,
    pub(super) toffset: usize,
    pub(super) tpreview: HashMap<String, Vec<Line<'static>>>,
    pub(super) thread_reads: Reads<(), Vec<Worktree>>,
    pub(super) session_reads: Reads<(), Vec<Session>>,
    pub(super) detail_reads: Reads<String, Vec<Line<'static>>>,
    pub(super) extra_reads: Reads<String, Vec<String>>,
    /// Claude Code sessions: the 3rd section of the threads tab, below the worktrees.
    pub(super) sessions: Vec<Session>,
    /// Data version of the threads tab: up with each read that brings new rows (they dissolve in).
    pub(super) threads_version: u64,
    pub(super) threads_fx: crate::ui::dissolve::Dissolve,
}

pub(super) struct RepositoryState {
    /// Choices of the where row: branches (default branch first), new branch, new worktree.
    pub(super) wheres: Vec<Where>,
    /// The home repo's repos (the picker shows the ones without a clone), the repo of the where
    /// row (None: 0 git repo), the open repo picker, the dir codehead started in.
    pub(super) repos: Vec<crate::infra::home::Repo>,
    pub(super) repo: Option<crate::infra::home::Repo>,
    pub(super) repo_pick: Option<crate::screens::start_screen::repo_pick::Picker>,
    /// The where row's branches per repo: off the UI thread (git runs).
    pub(super) where_reads: background::Reads<PathBuf, Choices>,
    pub(super) start_cwd: PathBuf,
    pub(super) where_i: usize,
    /// Where dropdown (enter on the where row): open with its cursor, scroll, filter.
    pub(super) where_menu: Option<WhereMenu>,
    /// Checkouts by branch, and the main checkout's branch (read once: the where row draws often).
    pub(super) checkouts: HashMap<String, String>,
    pub(super) repo_branch: String,
    /// Branch + top dir of the current directory's checkout ("current branch").
    pub(super) here_branch: String,
    pub(super) here_dir: String,
    /// Opts row: checkout the picked branch, stash conflicting changes; the box under the cursor.
    pub(super) checkout: bool,
    pub(super) stash: bool,
    pub(super) opt_i: usize,
}

pub(super) struct FlowSelectionState {
    /// The flow per agent of the new thread (`f` on its card): agent → (flow slug, start step);
    /// "" = 0 flow picked. 0 entry: the repo's last flow for the lead, else 0 flow.
    pub(super) agent_flows: std::collections::BTreeMap<String, (String, usize)>,
    /// The repo's last flow: the lead's until a pick.
    pub(super) flow_default: Option<String>,
    pub(super) flow_menu: Option<FlowMenu>,
    pub(super) flow_lib: crate::engine::flows::Library,
}

pub(super) struct ProjectSelectionState {
    pub(super) projects_tab: crate::features::projects::ProjectsTab,
    pub(super) projects_area: Rect,
    /// The new form's projects row: the repo's projects (repo, projects dir, its mtime: read
    /// again when a brief is made / saved / deleted), the picks (None: auto), the open dropdown.
    pub(super) proj_cache: Option<(
        PathBuf,
        PathBuf,
        Option<std::time::SystemTime>,
        Vec<crate::domain::projects::Project>,
    )>,
    pub(super) proj_picks: Option<Vec<String>>,
    pub(super) proj_menu: Option<WhereMenu>,
}

pub(super) struct ModelCatalogState {
    /// Model lists by harness (agent form; `<data dir>/models.json`), the reads under way, the
    /// last read error by harness, the read itself (tests: a fake, 0 harness spawned).
    pub(super) models: Option<HashMap<String, crate::infra::models::Entry>>,
    pub(super) model_reads: Reads<String, Result<crate::infra::models::Entry, String>>,
    pub(super) model_errors: HashMap<String, String>,
    pub(super) probe: fn(&str) -> Result<Vec<crate::infra::models::Model>, String>,
}

pub(super) struct DraftSelectionState {
    /// This task's draft file (id), and its content at the last write.
    pub(super) draft_id: String,
    pub(super) draft_saved: String,
    /// The draft on screen was saved on purpose (ctrl+s): its file stays kept.
    pub(super) draft_kept: bool,
    /// The other drafts (oldest first), and the open drafts dropdown: cursor, 1st row in view.
    pub(super) drafts: Vec<Draft>,
    pub(super) drafts_menu: Option<(usize, usize)>,
    /// Button of the drafts row under the cursor: 0 "view drafts (n)", 1 "save as draft".
    pub(super) draft_btn: usize,
    /// Drafts dropdown: a 1st d on this row (a 2nd d deletes the draft).
    pub(super) draft_armed: Option<usize>,
}

pub(super) struct OverlaysState {
    pub(super) modal: Option<Modal>,
    /// F2 menu (over every screen and modal), and the commands of its quick tab.
    pub(super) menu: Option<crate::screens::quick_menu::Menu>,
    /// The context menu (ctrl+.): the actions of this screen / field.
    pub(super) ctx: Option<crate::ui::context_menu::Menu<StartCtx>>,
    pub(super) menu_cmds: Vec<Cmd>,
    pub(super) menu_help: Vec<(&'static str, &'static str)>,
    pub(super) toast: Option<(String, Instant, bool)>,
    /// The notifications dropdown (ctrl+n).
    pub(super) notes_panel: Option<crate::features::notifications::Panel>,
    /// Time of the last plain esc, and whether it came on the start screen (a main tab, 0 local
    /// controls). A 2nd esc within `DOUBLE_ESC`: start screen → quit, else → close local controls.
    pub(super) last_esc: Option<(Instant, bool)>,
    /// Time of the last ctrl+c that did not quit (unsent text: a 2nd within 1s quits).
    pub(super) last_ctrl_c: Option<Instant>,
}

pub(super) struct PreviewState {
    /// Last mouse position: the clickable item under it gets the hover bg.
    pub(super) hover: Option<Position>,
    /// The last drawn frame: ctrl+click reads the URL under the mouse from it.
    pub(super) last_frame: ratatui::buffer::Buffer,
    /// Mouse selection in the preview (a thread's chat, a note, an agent); KEY: what it showed.
    pub(super) psel: crate::ui::screen_select::ScreenSel,
    pub(super) psel_key: String,
    pub(super) previews: Previews,
    pub(super) pscroll: u16,
    pub(super) regions: Regions,
}
