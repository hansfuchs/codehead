use super::{Agent, AgentForm, HashMap, Line, PathBuf, Rect, Worktree};

pub(crate) struct Config {
    pub notes: PathBuf,
    pub repo: Option<PathBuf>,
    pub cwd: PathBuf,
    /// Default agent: takes over threads of deleted agents, preselected for new threads.
    pub agent: String,
    /// Agents preselected for a new thread (-a, comma-separated).
    pub picks: Vec<String>,
    pub branch: Option<String>,
    pub prefix: String,
    pub copy_mode: bool,
    pub logs: PathBuf,
    /// Dir of the task drafts.
    pub drafts: PathBuf,
}

/// Where the new thread works.
#[derive(Clone, Debug)]
pub(crate) enum Target {
    /// New worktree on this branch.
    Branch(String),
    /// Existing branch: the checkout that has it. Else the main checkout: SWITCH → `git switch`
    /// to it first (STASH: a switch that local changes block → `git stash`, then again), else as is.
    Checkout { branch: String, switch: bool, stash: bool },
    /// New branch in the main checkout (git switch -c), 0 worktrees.
    NewBranch(String),
    /// Existing worktree without a thread.
    Worktree { path: String, branch: String },
    /// No git repo: the current directory itself.
    InPlace,
    /// Dir of a Claude Code session, in place.
    Dir(String),
}

pub(crate) enum Outcome {
    Quit,
    Copy(Vec<PathBuf>),
    /// AGENTS: first starts the thread, the others join it.
    /// CLONE: a home repo to clone first (url, path).
    /// FLOW: the slug of the flow the thread follows, the step it starts at.
    /// FLOWS: (agent, flow slug, start step) per agent with a flow.
    /// PROJECTS: the owner's projects (None: auto, the agents pick). `TITLE_SET`: the owner wrote
    /// the title.
    New {
        message: String,
        target: Target,
        agents: Vec<String>,
        clone: Option<(String, PathBuf)>,
        flows: Vec<(String, String, usize)>,
        projects: Option<Vec<String>>,
        title_set: bool,
    },
    Resume(Box<Worktree>),
}

/// $EDITOR on: a note, the compose prompt, the sprite of the open agent form.
pub(crate) enum Suspend {
    Note(PathBuf),
    Prompt,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Tab {
    /// The compose screen (`Screen::Compose`).
    New,
    /// Sub-screen of task: enter on the notes field.
    Notes,
    Threads,
    /// Projects of a repo: their briefs (projtab.rs).
    Projects,
    /// The owner's pull requests (mergetab.rs).
    Merge,
    Agents,
    /// Flows and their steps (flows.rs).
    Flows,
    Settings,
}

impl Tab {
    pub(super) fn next(self) -> Tab {
        match self {
            Tab::New | Tab::Notes => Tab::Threads,
            Tab::Threads => Tab::Projects,
            Tab::Projects => Tab::Merge,
            Tab::Merge => Tab::Agents,
            Tab::Agents => Tab::Flows,
            Tab::Flows => Tab::Settings,
            Tab::Settings => Tab::New,
        }
    }
    pub(super) fn prev(self) -> Tab {
        match self {
            Tab::New | Tab::Notes => Tab::Settings,
            Tab::Threads => Tab::New,
            Tab::Projects => Tab::Threads,
            Tab::Merge => Tab::Projects,
            Tab::Agents => Tab::Merge,
            Tab::Flows => Tab::Agents,
            Tab::Settings => Tab::Flows,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Screen {
    Pick,
    Compose,
}

/// Where a new task works (git repo only), the "where" row of the task screen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Where {
    /// The branch of the current directory's checkout: works there, 0 checkout.
    Current,
    Branch(String),
    NewBranch,
    NewWorktree,
}

/// The open where dropdown: cursor (a row of [`App::where_rows`]), 1st row in view, typed filter.
#[derive(Default)]
pub(super) struct WhereMenu {
    pub(super) cursor: usize,
    pub(super) offset: usize,
    pub(super) query: String,
}

/// The open flow dropdown of AGENT's card: the flows (0: 0 flow, I: the I-th flow), the 2nd
/// dropdown of the flow under the cursor: its steps (the step the agent starts at).
#[derive(Default)]
pub(super) struct FlowMenu {
    pub(super) agent: String,
    pub(super) cursor: usize,
    pub(super) offset: usize,
    /// The steps dropdown: cursor, 1st row in view.
    pub(super) sub: Option<(usize, usize)>,
}

/// The where row's choices for a repo: the choices, branches checked out elsewhere, the repo's
/// branch, the branch and top dir of the current directory.
#[derive(Default)]
pub(super) struct Choices {
    pub(super) wheres: Vec<Where>,
    pub(super) checkouts: HashMap<String, String>,
    pub(super) repo_branch: String,
    pub(super) here_branch: String,
    pub(super) here_dir: String,
}

/// A region of the start screen: tab / shift+tab walk them.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Region {
    /// The main tabs row.
    Tabs,
    /// The settings' sub-tabs row.
    SubTabs,
    Content,
    /// The prompt under a PR (merge tab).
    Input,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Field {
    /// The drafts row above the prompt: "view drafts (n)" (only with 1+ drafts), "save as draft".
    Drafts,
    Title,
    /// The repo row of "where" (enter: the repo picker).
    Repo,
    /// The projects row of "where" (←→ a project, space: on / off; auto: the agents pick).
    Projects,
    /// The branch row of "where" (enter: the where dropdown).
    Where,
    /// Checkboxes below the where row: checkout, stash.
    Opts,
    Agents,
    Notes,
    Prompt,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Act {
    Filter,
    ThreadFilter,
    /// An action of the thread list (archive, unarchive, edit).
    Thread(crate::features::thread_list::Act),
    AgentFilter,
    DeleteNote,
    EditNote,
    CopyPaths,
    ClearSelection,
    CollapseAll,
    ExpandAll,
    Reload,
    AddAlias,
    DeleteThreads,
    NewAgent,
    EditAgent,
    DeleteAgent,
    Start,
    EditPrompt,
    OpenNotes,
    SaveDraft,
    Reset,
    Quit,
}

/// A row of the start screen's context menu: a command of the screen, the flow of the agents
/// card under the cursor.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum StartCtx {
    Run(Act),
    Flow,
}

#[derive(Clone, Copy)]
pub(super) struct Cmd {
    pub(super) key: &'static str,
    pub(super) label: &'static str,
    pub(super) act: Act,
}

pub(super) enum ConfirmAction {
    Note(String),
    Threads(Vec<Worktree>),
    Agent(Agent),
    /// Esc on an agent form with edits: yes drops them, no: back to the form.
    DiscardAgent(Box<AgentForm>),
    /// The menu command "reset" of the task: where row, notes, context back to the start.
    ResetTask,
}

pub(super) enum Modal {
    Confirm {
        title: String,
        question: String,
        yes: bool,
        action: ConfirmAction,
    },
    Info {
        title: String,
        lines: Vec<Line<'static>>,
        scroll: u16,
    },
    Agent(Box<AgentForm>),
}

#[derive(Default)]
pub(super) struct Regions {
    pub(super) tabs: Vec<(Rect, Tab)>,
    pub(super) list: Rect,
    pub(super) preview: Rect,
    pub(super) menu_rows: Vec<(Rect, usize)>,
    pub(super) buttons: Vec<(Rect, bool)>,
    pub(super) notes_label: Rect,
    pub(super) where_row: Rect,
    /// The repo row, its chip (anchor of the repo picker), the open picker's area.
    pub(super) repo_row: Rect,
    pub(super) repo_chip: Rect,
    pub(super) proj_row: Rect,
    pub(super) repo_pick: Rect,
    /// The open flow dropdown: its area, rows (area, 0 = 0 flow / I = flow); the steps dropdown:
    /// its rows (area, step).
    pub(super) flow_menu: Rect,
    pub(super) flow_rows: Vec<(Rect, usize)>,
    pub(super) flow_steps: Vec<(Rect, usize)>,
    /// The where chip (anchor of the where dropdown), the rows of the open dropdown (area, choice).
    pub(super) where_chip: Rect,
    pub(super) where_rows: Vec<(Rect, usize)>,
    /// The project chip (anchor of the project dropdown), the rows of the open dropdown (area,
    /// 0 = auto / I = the I-th project).
    pub(super) proj_chip: Rect,
    pub(super) proj_rows: Vec<(Rect, usize)>,
    /// Rows of the open harness / model dropdown of the agent form (area, row).
    pub(super) form_rows: Vec<(Rect, usize)>,
    pub(super) task_agents: Rect,
    /// Checkboxes of the opts row: (area, 0 checkout / 1 stash).
    pub(super) opts: Vec<(Rect, usize)>,
    /// "view drafts (n)" field, and the rows of its open dropdown (area, draft).
    pub(super) drafts: Rect,
    /// "save as draft" button, right of "view drafts (n)".
    pub(super) save_draft: Rect,
    pub(super) draft_rows: Vec<(Rect, usize)>,
}

/// The result of a 1-off action that ran off the UI thread.
pub(super) enum Job {
    /// Threads deleted: the report.
    Threads(Vec<Line<'static>>),
    /// An agent deleted: the report.
    AgentDeleted(Vec<Line<'static>>),
    /// An agent saved: its live sessions restarted (the toast's text).
    Restarted(String),
}
