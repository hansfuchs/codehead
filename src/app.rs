//! Screens: new (compose, the start) | threads | agents; notes = sub-screen of task. Modals: F2 menu, y/n confirm, info.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Constraint, Layout, Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph, Wrap};
use unicode_width::UnicodeWidthStr;

use crate::agents::{self, Agent, Use};
use crate::attach::{Pasted, Paster};
use crate::command::Command;
use crate::drafts::{self, Draft};
use crate::dropdown;
use crate::mention::Mention;
use crate::claude::{self, Session};
use crate::styles::{self, Style as OutStyle};
use crate::notes::{Notes, Row, slug};
use crate::preview::Previews;
use crate::sprites::{self, Sprite};
use crate::textarea::{Outcome as Input, TextArea};
use crate::threads::{self, Worktree};
use crate::theme;
use crate::background::{self, Reads};

/// Agent card rows: the sprite + 2 border rows.
use crate::cards::CARD_H;

/// Below this width the preview goes under the list (1/3 list, 2/3 preview).
const THIN: u16 = 110;

pub struct Config {
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
pub enum Target {
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

pub enum Outcome {
    Quit,
    Copy(Vec<PathBuf>),
    /// AGENTS: first starts the thread, the others join it.
    /// CLONE: a home repo to clone first (url, path).
    /// FLOW: the slug of the flow the thread follows, the step it starts at.
    /// FLOWS: (agent, flow slug, start step) per agent with a flow.
    New { message: String, target: Target, agents: Vec<String>, clone: Option<(String, PathBuf)>, flows: Vec<(String, String, usize)> },
    Resume(Box<Worktree>),
}

/// $EDITOR on: a note, the compose prompt, the sprite of the open agent form.
pub enum Suspend {
    Note(PathBuf),
    Prompt,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Tab {
    /// The compose screen (Screen::Compose).
    New,
    /// Sub-screen of task: enter on the notes field.
    Notes,
    Threads,
    /// The owner's pull requests (mergetab.rs).
    Merge,
    Agents,
    /// Flows and their steps (flows.rs).
    Flows,
    Settings,
}

impl Tab {
    fn next(self) -> Tab {
        match self {
            Tab::New | Tab::Notes => Tab::Threads,
            Tab::Threads => Tab::Merge,
            Tab::Merge => Tab::Agents,
            Tab::Agents => Tab::Flows,
            Tab::Flows => Tab::Settings,
            Tab::Settings => Tab::New,
        }
    }
    fn prev(self) -> Tab {
        match self {
            Tab::New | Tab::Notes => Tab::Settings,
            Tab::Threads => Tab::New,
            Tab::Merge => Tab::Threads,
            Tab::Agents => Tab::Merge,
            Tab::Flows => Tab::Agents,
            Tab::Settings => Tab::Flows,
        }
    }
}

/// The main tabs in order (F1-F6, alt+1-6).
const MAIN_TABS: [Tab; 6] = [Tab::New, Tab::Threads, Tab::Merge, Tab::Agents, Tab::Flows, Tab::Settings];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Screen {
    Pick,
    Compose,
}

/// Where a new task works (git repo only), the "where" row of the task screen.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Where {
    /// The branch of the current directory's checkout: works there, 0 checkout.
    Current,
    Branch(String),
    NewBranch,
    NewWorktree,
}

/// The open where dropdown: cursor (a row of [`App::where_rows`]), 1st row in view, typed filter.
#[derive(Default)]
struct WhereMenu {
    cursor: usize,
    offset: usize,
    query: String,
}

/// The open flow dropdown of AGENT's card: the flows (0: 0 flow, I: the I-th flow), the 2nd
/// dropdown of the flow under the cursor: its steps (the step the agent starts at).
#[derive(Default)]
struct FlowMenu {
    agent: String,
    cursor: usize,
    offset: usize,
    /// The steps dropdown: cursor, 1st row in view.
    sub: Option<(usize, usize)>,
}

/// The where row's choices for a repo: the choices, branches checked out elsewhere, the repo's
/// branch, the branch and top dir of the current directory.
#[derive(Default)]
struct Choices {
    wheres: Vec<Where>,
    checkouts: HashMap<String, String>,
    repo_branch: String,
    here_branch: String,
    here_dir: String,
}

/// Choices of REPO (CWD: the dir codehead runs in): current branch, new branch, new branch +
/// worktree, default branch (main, else master), the other branches (newest commit first).
fn where_choices(repo: &Path, cwd: &Path) -> Choices {
    // 2 git runs at once (~15 ms each): off the UI thread (`quick_choices` until then)
    let (bs, checkouts) = std::thread::scope(|s| {
        let checkouts = s.spawn(|| threads::checkouts(repo));
        (threads::branches(repo), checkouts.join().unwrap_or_default())
    });
    choices_of(repo, cwd, bs, checkouts)
}

/// The where choices by the file system only (0 git runs): current branch, new branch, new
/// branch + worktree. The branches come with `where_choices`.
fn quick_choices(repo: &Path, cwd: &Path) -> Choices {
    choices_of(repo, cwd, Vec::new(), HashMap::new())
}

fn choices_of(repo: &Path, cwd: &Path, bs: Vec<String>, checkouts: HashMap<String, String>) -> Choices {
    let here_branch = threads::current_branch(cwd);
    let repo_branch = threads::current_branch(repo);
    let here_dir = threads::toplevel(cwd).unwrap_or_else(|| repo.to_path_buf()).display().to_string();
    let default = ["main", "master"].into_iter().find(|d| bs.iter().any(|b| b == d)).map(String::from);
    let mut wheres = vec![Where::Current, Where::NewBranch, Where::NewWorktree];
    if let Some(d) = default.as_ref().filter(|d| **d != here_branch) {
        wheres.push(Where::Branch(d.clone()));
    }
    wheres.extend(bs.into_iter().filter(|b| *b != here_branch && Some(b) != default.as_ref()).map(Where::Branch));
    Choices { wheres, checkouts, repo_branch, here_branch, here_dir }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Field {
    /// The drafts row above the prompt: "view drafts (n)" (only with 1+ drafts), "save as draft".
    Drafts,
    Title,
    /// The repo row of "where" (enter: the repo picker).
    Repo,
    /// The branch row of "where" (enter: the where dropdown).
    Where,
    /// Checkboxes below the where row: checkout, stash.
    Opts,
    Agents,
    Notes,
    Prompt,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Act {
    Filter,
    ThreadFilter,
    /// An action of the thread list (archive, unarchive, edit).
    Thread(crate::threadlist::Act),
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

struct Cmd {
    key: &'static str,
    label: &'static str,
    act: Act,
}

enum ConfirmAction {
    Note(String),
    Threads(Vec<Worktree>),
    Agent(Agent),
    /// Esc on an agent form with edits: yes drops them, no: back to the form.
    DiscardAgent(Box<AgentForm>),
    /// The menu command "reset" of the task: where row, notes, context back to the start.
    ResetTask,
}

/// New / edit agent form. Fields: name, harness, model, effort, color, mode, style, sprite set, sprite, voice, system prompt, buttons.
struct AgentForm {
    edit: Option<Agent>,
    name: TextArea,
    /// Harness alone (`claude`), or harness/model for other harnesses (`codex/gpt-5.5`).
    harness: TextArea,
    /// Claude models: "default" first, then config::models() (+ the agent's own, if not listed).
    models: Vec<String>,
    model: usize,
    /// Index into agents::EFFORTS. Global: ~/.claude/settings.json `effortLevel`.
    effort: usize,
    voice: TextArea,
    prompt: TextArea,
    color: usize,
    mode: usize,
    /// Index into `styles`; 0 = none.
    style: usize,
    styles: Vec<(Option<String>, String)>,
    sprite: Sprite,
    /// The avatar picker, open over the form (enter on the avatar rows).
    picker: Option<crate::picker::Picker>,
    field: usize,
    save: bool,
    error: Option<String>,
    areas: [Rect; FORM_FIELDS - 1],
    /// `fingerprint` at open: Esc asks before it drops edits.
    orig: String,
    /// Harnesses of the harness dropdown: the installed ones (name, label, bin).
    harnesses: Vec<(String, String, String)>,
    /// Open dropdown of the harness / model field.
    pick: Option<FormPick>,
    /// codehead's MCP servers (settings › mcp servers): name, on for this agent; the cursor.
    mcp: Vec<(String, bool)>,
    mcp_cur: usize,
}

/// The open dropdown of the agent form: harness list, or the model list of HARNESS; cursor,
/// 1st row in view, typed filter.
struct FormPick {
    model: bool,
    harness: String,
    cursor: usize,
    offset: usize,
    query: String,
}

const F_NAME: usize = 0;
const F_HARNESS: usize = 1;
const F_MODEL: usize = 2;
const F_EFFORT: usize = 3;
const F_COLOR: usize = 4;
const F_MODE: usize = 5;
const F_STYLE: usize = 6;
const F_MCP: usize = 7;
const F_SET: usize = 8;
const F_SPRITE: usize = 9;
const F_VOICE: usize = 10;
const F_PROMPT: usize = 11;
const F_BUTTONS: usize = 12;
const FORM_FIELDS: usize = 13;

impl AgentForm {
    fn new(edit: Option<Agent>, styles: &[OutStyle], default_style: Option<&str>) -> AgentForm {
        let mut name = TextArea::new(true, "lowercase name, e.g. nova");
        let mut harness = TextArea::new(true, "claude · claude/opus · codex/gpt-5.5 · opencode · goose · cursor · pi · amp …");
        let mut voice = TextArea::new(false, "tone + persona, e.g. dry humour, calls the owner boss");
        let mut prompt = TextArea::new(false, "standing instructions: role, focus, rules (system prompt)");
        let mut opts: Vec<(Option<String>, String)> = vec![(None, "none".into())];
        opts.extend(styles.iter().map(|s| (Some(s.id.clone()), s.name.clone())));
        let (mut color, mut mode) = (4, 0);
        let mut models: Vec<String> = std::iter::once("default".to_string()).chain(crate::config::models()).collect();
        let mut model = 0;
        let effort = agents::effort().and_then(|e| agents::EFFORTS.iter().position(|x| *x == e)).unwrap_or(0);
        let profile = agents::parse_profile(edit.as_ref().and_then(|a| a.prompt.as_deref()), default_style);
        if let Some(a) = &edit {
            name.set_text(&a.name);
            if a.harness == "claude" {
                harness.set_text("claude");
                if let Some(m) = a.model.as_deref().filter(|m| !m.is_empty() && *m != "default") {
                    model = models.iter().position(|x| x == m).unwrap_or_else(|| {
                        models.push(m.to_string());
                        models.len() - 1
                    });
                }
            } else {
                harness.set_text(&a.harness_model());
            }
            color = a.color.as_deref().and_then(|c| agents::COLORS.iter().position(|x| *x == c)).unwrap_or(3);
            mode = agents::MODES.iter().position(|m| *m == a.mode).unwrap_or(0);
        } else {
            harness.set_text("claude");
        }
        voice.set_text(&profile.voice);
        prompt.set_text(&profile.prompt);
        let style = opts.iter().position(|(id, _)| *id == profile.style).unwrap_or(0);
        // saved, else what the agent tab shows (stable pick), else (new agent) a random regi
        let sprite = profile.sprite.clone().unwrap_or_else(|| match &edit {
            Some(a) => sprites::default_for(&a.name),
            None => sprites::random(crate::avatar::Cat::Regi),
        });
        let mut f = AgentForm {
            edit,
            name,
            harness,
            models,
            model,
            effort,
            voice,
            prompt,
            color,
            mode,
            style,
            styles: opts,
            picker: None,
            sprite,
            field: 0,
            save: true,
            error: None,
            areas: [Rect::default(); FORM_FIELDS - 1],
            orig: String::new(),
            harnesses: crate::engine::harnesses::installed().into_iter().map(|h| (h.name.to_string(), h.label.to_string(), h.bin.to_string())).collect(),
            pick: None,
            mcp: Vec::new(),
            mcp_cur: 0,
        };
        let own: Vec<String> = f.edit.as_ref().and_then(|a| crate::engine::roster::get(&a.name)).map(|a| a.mcp).unwrap_or_default();
        f.mcp = crate::engine::servers::all().into_iter().map(|s| (s.name.clone(), own.contains(&s.name))).collect();
        f.orig = f.fingerprint();
        f
    }

    /// The form's values in 1 string: Esc asks before it drops a form that differs from `orig`.
    fn fingerprint(&self) -> String {
        format!(
            "{}\x1f{}\x1f{}\x1f{}\x1f{}\x1f{}\x1f{}\x1f{}\x1f{}\x1f{}\x1f{:?}",
            self.name.text(),
            self.harness.text(),
            self.model,
            self.effort,
            self.voice.text(),
            self.prompt.text(),
            self.color,
            self.mode,
            self.style,
            self.sprite.id,
            self.mcp
        )
    }

    /// Edits since the form opened.
    fn dirty(&self) -> bool {
        self.fingerprint() != self.orig
    }

    fn text(&mut self) -> Option<&mut TextArea> {
        match self.field {
            F_NAME => Some(&mut self.name),
            F_HARNESS => Some(&mut self.harness),
            F_VOICE => Some(&mut self.voice),
            F_PROMPT => Some(&mut self.prompt),
            _ => None,
        }
    }

    fn multiline(&self) -> bool {
        matches!(self.field, F_VOICE | F_PROMPT)
    }

    /// Harness is claude (or empty = claude): the model row applies.
    fn is_claude(&self) -> bool {
        let h = self.harness.text();
        let h = h.trim();
        h.is_empty() || h == "claude"
    }

    /// The harness of the harness field: the part before `/` (empty: claude).
    fn harness_name(&self) -> String {
        let t = self.harness.text();
        let h = t.trim().split('/').next().unwrap_or_default().trim();
        if h.is_empty() { "claude".into() } else { h.to_string() }
    }

    /// The model: claude → the model row's, else the part after `/` (0: "default").
    fn model_id(&self) -> String {
        if self.is_claude() {
            return self.models[self.model].clone();
        }
        let t = self.harness.text();
        t.trim().split_once('/').map(|(_, m)| m.trim().to_string()).filter(|m| !m.is_empty()).unwrap_or_else(|| "default".into())
    }

    /// Harness NAME from the dropdown: the field holds it (same harness: its model stays).
    fn set_harness(&mut self, name: &str) {
        if name == self.harness_name() {
            return;
        }
        self.harness.load_text(name);
        if name == "claude" {
            self.model = 0;
        }
    }

    /// Model ID of the current harness from the dropdown: claude → the model row, else `harness/id`.
    fn set_model(&mut self, id: &str) {
        let h = self.harness_name();
        if h == "claude" {
            self.harness.load_text("claude");
            self.model = self.models.iter().position(|m| m == id).unwrap_or_else(|| {
                self.models.push(id.to_string());
                self.models.len() - 1
            });
        } else if id == "default" {
            self.harness.load_text(&h);
        } else {
            self.harness.load_text(&format!("{h}/{id}"));
        }
    }

    /// `claude`, `claude/opus`, or the harness text as typed (other harnesses).
    fn harness_model(&self) -> String {
        let h = self.harness.text().trim().to_string();
        if !self.is_claude() {
            return h;
        }
        match self.models[self.model].as_str() {
            "default" => "claude".into(),
            m => format!("claude/{m}"),
        }
    }

    /// None: effortLevel unset.
    fn effort_level(&self) -> Option<&'static str> {
        Some(agents::EFFORTS[self.effort]).filter(|e| *e != "default")
    }

    /// r: a random avatar of the category, the speed and eyes kept.
    fn random_sprite(&mut self) {
        let a = self.sprite.avatar;
        let r = sprites::random(a.cat).avatar;
        self.sprite = Sprite::of(crate::avatar::Avatar { idx: r.idx, ..a });
    }

    fn profile(&self) -> agents::Profile {
        agents::Profile {
            style: self.styles[self.style].0.clone(),
            voice: self.voice.text().trim().to_string(),
            prompt: self.prompt.text().trim().to_string(),
            sprite: Some(self.sprite.clone()),
        }
    }

    fn spec(&self, styles: &[OutStyle], global: Option<&str>) -> agents::Spec {
        let harness = self.harness_model();
        agents::Spec {
            name: self.name.text().trim().to_string(),
            prompt: agents::compose(&self.profile(), styles, if harness.is_empty() { "claude" } else { &harness }, global),
            harness,
            color: Some(agents::COLORS[self.color].to_string()),
            mode: agents::MODES[self.mode].to_string(),
            mcp: self.mcp.iter().filter(|(_, on)| *on).map(|(n, _)| n.clone()).collect(),
        }
    }

    /// What the base style does for this harness.
    fn style_note(&self, global: Option<&str>) -> String {
        let claude = self.harness.text().trim().split('/').next().is_none_or(|h| h.is_empty() || h == "claude");
        let (id, name) = &self.styles[self.style];
        match (id, claude, global) {
            (None, true, Some(g)) => format!("claude agents still get {g} from ~/.claude/settings.json"),
            (None, _, _) => "no base style".into(),
            (Some(_), true, Some(g)) if g == name => "= global outputStyle: claude applies it, 0 copies in the prompt".to_string(),
            (Some(_), _, _) => format!("{name} goes into the agent prompt"),
        }
    }
}

enum Modal {
    Confirm { title: String, question: String, yes: bool, action: ConfirmAction },
    Info { title: String, lines: Vec<Line<'static>>, scroll: u16 },
    Agent(Box<AgentForm>),
}

#[derive(Default)]
struct Regions {
    tabs: Vec<(Rect, Tab)>,
    list: Rect,
    preview: Rect,
    menu_rows: Vec<(Rect, usize)>,
    buttons: Vec<(Rect, bool)>,
    notes_label: Rect,
    where_row: Rect,
    /// The repo row, its chip (anchor of the repo picker), the open picker's area.
    repo_row: Rect,
    repo_chip: Rect,
    repo_pick: Rect,
    /// The open flow dropdown: its area, rows (area, 0 = 0 flow / I = flow); the steps dropdown:
    /// its rows (area, step).
    flow_menu: Rect,
    flow_rows: Vec<(Rect, usize)>,
    flow_steps: Vec<(Rect, usize)>,
    /// The where chip (anchor of the where dropdown), the rows of the open dropdown (area, choice).
    where_chip: Rect,
    where_rows: Vec<(Rect, usize)>,
    /// Rows of the open harness / model dropdown of the agent form (area, row).
    form_rows: Vec<(Rect, usize)>,
    task_agents: Rect,
    /// Checkboxes of the opts row: (area, 0 checkout / 1 stash).
    opts: Vec<(Rect, usize)>,
    /// "view drafts (n)" field, and the rows of its open dropdown (area, draft).
    drafts: Rect,
    /// "save as draft" button, right of "view drafts (n)".
    save_draft: Rect,
    draft_rows: Vec<(Rect, usize)>,
}

type AgentSnapshot = (Vec<Agent>, String, HashMap<String, Vec<Use>>);

/// The result of a 1-off action that ran off the UI thread.
enum Job {
    /// Threads deleted: the report.
    Threads(Vec<Line<'static>>),
    /// An agent deleted: the report.
    AgentDeleted(Vec<Line<'static>>),
    /// An agent saved: its live sessions restarted (the toast's text).
    Restarted(String),
}

pub struct App {
    pub cfg: Config,
    /// The merge tab: the owner's pull requests, 1 open.
    merge_tab: crate::mergetab::MergeTab,
    /// The flows tab.
    flows_tab: crate::flows::FlowsTab,
    flows_area: Rect,
    merge_area: Rect,
    /// The new-thread view was open at the last tick (its windows warmed up: engine::warm).
    warmed: bool,
    notes: Notes,
    collapsed: HashSet<String>,
    selected: Vec<String>,
    rows: Vec<Row>,
    cursor: usize,
    offset: usize,
    filter: Option<String>,
    filter_editing: bool,
    /// Last mouse position: the clickable item under it gets the hover bg.
    hover: Option<Position>,
    /// The last drawn frame: ctrl+click reads the URL under the mouse from it.
    last_frame: ratatui::buffer::Buffer,
    /// Mouse selection in the preview (a thread's chat, a note, an agent); KEY: what it showed.
    psel: crate::screensel::ScreenSel,
    psel_key: String,
    /// Input of the notes filter (undo / redo, cursor); FILTER holds its text.
    filter_ta: TextArea,
    tab: Tab,
    settings: Option<crate::settings::Settings>,
    tabs_focus: bool,
    /// Threads tab rows: every thread (preview data), worktrees without a thread.
    worktrees: Option<Vec<Worktree>>,
    /// Threads tab: the thread list (cards, repo + view tabs, filter, actions); its views
    /// `worktrees` / `claude` show this screen's rows (cursor, 1st row in view).
    tlist: crate::threadlist::ThreadList,
    tcursor: usize,
    toffset: usize,
    tpreview: HashMap<String, Vec<Line<'static>>>,
    thread_reads: Reads<(), Vec<Worktree>>,
    session_reads: Reads<(), Vec<Session>>,
    agent_reads: Reads<(), AgentSnapshot>,
    /// 1-off actions off the UI thread (thread delete, agent delete, session restarts): their
    /// results, 1 key per job.
    jobs: Reads<u64, Job>,
    job_n: u64,
    detail_reads: Reads<String, Vec<Line<'static>>>,
    extra_reads: Reads<String, Vec<String>>,
    note_reads: Reads<(), Notes>,
    /// Claude Code sessions: the 3rd section of the threads tab, below the worktrees.
    sessions: Vec<Session>,
    /// Context: last responses (Claude Code sessions, threads) that go into the thread message.
    carry: Vec<Session>,
    agents: Vec<Agent>,
    usage: Option<HashMap<String, Vec<Use>>>,
    /// codehead.toml of the settings tab (tests: a temp file, never the real one).
    settings_path: PathBuf,
    acursor: usize,
    /// Agents tab: first virtual row of the card grid in view.
    aoffset: usize,
    /// Agents tab filter (fuzzy: name, harness, model). Some = row shows text.
    afilter: Option<String>,
    afilter_editing: bool,
    afilter_ta: TextArea,
    /// Agents tab, last draw: every card (agent, x, virtual row) for ↑↓←→, visible cards for clicks.
    aplace: Vec<(usize, u16, usize)>,
    acards: Vec<(Rect, usize)>,
    apreview: HashMap<String, Vec<String>>,
    picks: Vec<String>,
    pick_cursor: usize,
    pick_offset: usize,
    pick_places: Vec<(usize, u16, usize)>,
    styles: Vec<OutStyle>,
    global_style: Option<String>,
    previews: Previews,
    pscroll: u16,
    screen: Screen,
    title: TextArea,
    prompt: TextArea,
    field: Field,
    target: Option<Target>,
    /// Choices of the where row: branches (default branch first), new branch, new worktree.
    wheres: Vec<Where>,
    /// The home repo's repos (the picker shows the ones without a clone), the repo of the where
    /// row (None: 0 git repo), the open repo picker, the dir codehead started in.
    repos: Vec<crate::home::Repo>,
    repo: Option<crate::home::Repo>,
    repo_pick: Option<crate::repopick::Picker>,
    /// The where row's branches per repo: off the UI thread (git runs).
    where_reads: background::Reads<PathBuf, Choices>,
    /// Data version of the threads tab: up with each read that brings new rows (they dissolve in).
    threads_version: u64,
    threads_fx: crate::dissolve::Dissolve,
    start_cwd: PathBuf,
    where_i: usize,
    /// Where dropdown (enter on the where row): open with its cursor, scroll, filter.
    where_menu: Option<WhereMenu>,
    /// Model lists by harness (agent form; `<data dir>/models.json`), the reads under way, the
    /// last read error by harness, the read itself (tests: a fake, 0 harness spawned).
    models: Option<HashMap<String, crate::models::Entry>>,
    model_reads: Reads<String, Result<crate::models::Entry, String>>,
    model_errors: HashMap<String, String>,
    probe: fn(&str) -> Result<Vec<crate::models::Model>, String>,
    /// Checkouts by branch, and the main checkout's branch (read once: the where row draws often).
    checkouts: HashMap<String, String>,
    repo_branch: String,
    /// Branch + top dir of the current directory's checkout ("current branch").
    here_branch: String,
    here_dir: String,
    /// Opts row: checkout the picked branch, stash conflicting changes; the box under the cursor.
    checkout: bool,
    stash: bool,
    opt_i: usize,
    /// The flow per agent of the new thread (`f` on its card): agent → (flow slug, start step);
    /// "" = 0 flow picked. 0 entry: the repo's last flow for the lead, else 0 flow.
    agent_flows: std::collections::BTreeMap<String, (String, usize)>,
    /// The repo's last flow: the lead's until a pick.
    flow_default: Option<String>,
    flow_menu: Option<FlowMenu>,
    flow_lib: crate::engine::flows::Library,
    /// Prompt pastes: images, long texts (files, listed in the message).
    paster: Paster,
    files: Vec<(String, PathBuf)>,
    mention: Mention,
    /// `>` dropdown of the prompt: commands (mention a file / note / thread / artifact).
    command: Command,
    /// Path dropdown of the prompt: the entries of the directory a typed path names.
    paths: crate::paths::Paths,
    /// `#` of the prompt: the shortcuts.
    shortcuts: crate::shortcuts::Pick,
    /// This task's draft file (id), and its content at the last write.
    draft_id: String,
    draft_saved: String,
    /// The draft on screen was saved on purpose (ctrl+s): its file stays kept.
    draft_kept: bool,
    /// The other drafts (oldest first), and the open drafts dropdown: cursor, 1st row in view.
    drafts: Vec<Draft>,
    drafts_menu: Option<(usize, usize)>,
    /// Button of the drafts row under the cursor: 0 "view drafts (n)", 1 "save as draft".
    draft_btn: usize,
    /// Agent cards per row (last draw), for ↑↓.
    /// Global effort (~/.claude/settings.json effortLevel), for the cards.
    effort: String,
    modal: Option<Modal>,
    /// F2 menu (over every screen and modal), and the commands of its quick tab.
    menu: Option<crate::menu::Menu>,
    menu_cmds: Vec<Cmd>,
    menu_help: Vec<(&'static str, &'static str)>,
    toast: Option<(String, Instant, bool)>,
    regions: Regions,
    pub outcome: Option<Outcome>,
    /// The notifications dropdown (ctrl+n).
    notes_panel: Option<crate::notify::Panel>,
    pub suspend: Option<Suspend>,
    /// Time of the last plain esc, and whether it came on the start screen (a main tab, 0 local
    /// controls). A 2nd esc within DOUBLE_ESC: start screen → quit, else → close local controls.
    last_esc: Option<(Instant, bool)>,
    /// A key or paste (not esc) in the settings tab since it opened: its path suggestions came
    /// from typing, so their esc does not count for esc esc.
    settings_typed: bool,
    /// Drafts dropdown: a 1st d on this row (a 2nd d deletes the draft).
    draft_armed: Option<usize>,
    /// Time of the last ctrl+c that did not quit (unsent text: a 2nd within 1s quits).
    last_ctrl_c: Option<Instant>,
}

/// 2 esc presses within this time: quit on the task tab, close local controls elsewhere.
const DOUBLE_ESC: Duration = Duration::from_millis(500);

/// Toast tail for a session restart after an agent edit.
fn restart_note(r: &agents::Restart) -> String {
    let mut parts = Vec::new();
    if r.resumed > 0 {
        parts.push(format!("{} sessions restarted (conversation kept)", r.resumed));
    }
    if r.stopped > 0 {
        parts.push(format!("{} sessions stopped (new harness: next message starts fresh)", r.stopped));
    }
    if !r.busy.is_empty() {
        parts.push(format!("{} busy: old profile until stopped ({})", r.busy.len(), r.busy.join(", ")));
    }
    if !r.failed.is_empty() {
        parts.push(format!("failed: {}", r.failed.join("; ")));
    }
    parts.iter().map(|p| format!(" · {p}")).collect()
}

/// Area of the (area, index) row at AT.
fn find_rows(v: &[(Rect, usize)], at: Position) -> Option<Rect> {
    v.iter().map(|(r, _)| *r).find(|r| r.contains(at))
}

/// Filter row ROW: the input cells after the `filter › ` label, before the count (COUNT cells).
fn filter_input(row: Rect, count: usize) -> Rect {
    let w = row.width.saturating_sub(9 + count as u16 + 1).max(1);
    Rect { x: row.x + 9, y: row.y, width: w, height: 1 }
}

/// Quick tab id of "keys of this screen".
const KEYS_ITEM: usize = 1000;

/// Threads tab row of a thread card (the menu opens it like the threads tab does).
fn worktree_of(c: &crate::nav::Card) -> Worktree {
    let path = c.dir.as_ref().map(|d| d.display().to_string()).unwrap_or_default();
    let thread = crate::threads::Thread {
        id: c.id.clone(),
        title: c.title.clone(),
        state: c.state.clone(),
        live: c.live,
        updated: c.activity / 1000,
        worktree: None,
        branch: (!c.branch.is_empty()).then(|| c.branch.clone()),
        dir: Some(path.clone()),
        busy: c.busy,
        needs: c.blocked.then(|| "question".to_string()),
        ..Default::default()
    };
    Worktree { path, branch: if c.branch.is_empty() { "in place".into() } else { c.branch.clone() }, thread: Some(thread), unpushed: 0, when: String::new(), linked: c.worktree, repo: c.repo.clone() }
}

fn key_name(k: &KeyEvent) -> String {
    let mut s = String::new();
    if k.modifiers.contains(KeyModifiers::CONTROL) {
        s.push_str("ctrl+");
    }
    if k.modifiers.contains(KeyModifiers::ALT) {
        s.push_str("alt+");
    }
    match k.code {
        // the case stays: D is not d (shift+d never deletes)
        KeyCode::Char(c) => s.push(c),
        KeyCode::Esc => s.push_str("esc"),
        KeyCode::Enter => s.push_str("enter"),
        _ => s.push('?'),
    }
    s
}

fn hint(key: &str, label: &str) -> Vec<Span<'static>> {
    vec![Span::styled(key.to_string(), theme::text()), Span::styled(format!(" {label}  "), theme::muted())]
}

fn trash(path: &std::path::Path) -> Result<&'static str, String> {
    let ok = |c: &mut std::process::Command| c.stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status().is_ok_and(|s| s.success());
    if ok(std::process::Command::new("gio").arg("trash").arg(path)) {
        return Ok("moved to trash");
    }
    if ok(std::process::Command::new("trash-put").arg(path)) {
        return Ok("moved to trash");
    }
    let r = if path.is_dir() { std::fs::remove_dir_all(path) } else { std::fs::remove_file(path) };
    r.map(|_| "deleted").map_err(|e| e.to_string())
}

/// Fuzzy match: Q's chars in order inside TEXT (case-insensitive). Higher = better:
/// adjacent matches and word starts score more. None: no match.
pub(crate) fn fuzzy(q: &str, text: &str) -> Option<i64> {
    let t: Vec<char> = text.to_lowercase().chars().collect();
    let mut score = 0i64;
    let mut ti = 0usize;
    let mut prev: Option<usize> = None;
    for qc in q.chars().filter(|c| !c.is_whitespace()) {
        let at = (ti..t.len()).find(|&i| t[i] == qc)?;
        score += 1;
        if prev.is_some_and(|p| p + 1 == at) {
            score += 5;
        }
        if at == 0 || !t[at - 1].is_alphanumeric() {
            score += 3;
        }
        prev = Some(at);
        ti = at + 1;
    }
    Some(score)
}

/// $HOME/x → ~/x.
fn tilde(p: &str) -> String {
    match std::env::var("HOME") {
        Ok(h) if !h.is_empty() && p.starts_with(&h) => format!("~{}", &p[h.len()..]),
        _ => p.to_string(),
    }
}

impl App {
    pub fn new(cfg: Config, title: &str) -> App {
        // the notes tree (20k+ entries in a big notes root): read in the background, after the
        // 1st frame; tests: at once
        let notes = if cfg!(test) { Notes::load(&cfg.notes) } else { Notes::empty(&cfg.notes) };
        let mut t = TextArea::new(true, "what the thread is about (also names the worktree)");
        t.set_text(title);
        let p = TextArea::new(false, "free text for the agents, sent below the list of notes");
        let styles = styles::load(&styles::dir());
        let global_style = styles::global_name();
        // Profile synchronization writes prompts: use current profiles for this step.
        let mut agents = agents::roster_only();
        let synced = agents::sync(&agents, &styles, global_style.as_deref());
        if !synced.is_empty() {
            agents = agents::roster_only();
        }
        let mut picks: Vec<String> = cfg.picks.iter().filter(|n| agents.iter().any(|a| &a.name == *n)).cloned().collect();
        if picks.is_empty() && agents.iter().any(|a| a.name == cfg.agent) {
            picks.push(cfg.agent.clone());
        }
        // where: current branch, new branch, new branch + worktree, default branch (main, else master),
        // the other branches (newest commit first)
        let Choices { wheres, checkouts, repo_branch, here_branch, here_dir } = cfg.repo.as_deref().map(|r| if cfg!(test) { where_choices(r, &cfg.cwd) } else { quick_choices(r, &cfg.cwd) }).unwrap_or_default();
        // the repo codehead started in; the home repo's repos for the picker
        let repo = cfg.repo.as_ref().map(|r| crate::home::Repo { name: r.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), path: r.clone(), url: None });
        let repos = if cfg!(test) { Vec::new() } else { crate::home::repos() };
        let start_cwd = cfg.cwd.clone();
        let home = PathBuf::from(std::env::var("HOME").unwrap_or_default());
        let paster = Paster::new(home.join(".cache/codehead/paste").join(format!("new-{}", (crate::engine::now_ms() / 1000))));
        // -b: new worktree on that branch
        let where_i = if cfg.branch.is_some() { wheres.iter().position(|w| *w == Where::NewWorktree).unwrap_or(0) } else { 0 };
        let mut app = App {
            repos,
            repo,
            repo_pick: None,
            where_reads: background::Reads::default(),
            threads_version: 0,
            threads_fx: Default::default(),
            start_cwd,
            wheres,
            where_i,
            where_menu: None,
            models: None,
            model_reads: Reads::default(),
            model_errors: HashMap::new(),
            probe: if cfg!(test) { |_| Err("tests: 0 probe".into()) } else { crate::models::read },
            checkouts,
            repo_branch,
            here_branch,
            here_dir,
            checkout: true,
            stash: false,
            opt_i: 0,
            agent_flows: Default::default(),
            flow_default: None,
            flow_menu: None,
            flow_lib: crate::engine::flows::Library::default(),
            paster,
            files: Vec::new(),
            mention: Mention::default(),
            command: Command::new(&cfg.cwd),
            paths: crate::paths::Paths::new(&cfg.cwd),
            shortcuts: crate::shortcuts::Pick::default(),
            draft_id: drafts::new_id(),
            draft_saved: String::new(),
            draft_kept: false,
            drafts: Vec::new(),
            drafts_menu: None,
            draft_btn: 0,
            effort: agents::effort_name(),
            notes,
            collapsed: HashSet::new(),
            selected: Vec::new(),
            rows: Vec::new(),
            cursor: 0,
            offset: 0,
            filter: None,
            filter_editing: false,
            filter_ta: TextArea::new(true, ""),
            hover: None,
            last_frame: ratatui::buffer::Buffer::default(),
            psel: Default::default(),
            psel_key: String::new(),
            warmed: false,
            merge_tab: Default::default(),
            flows_tab: crate::flows::FlowsTab::new(Some(cfg.cwd.clone())),
            flows_area: Rect::default(),
            merge_area: Rect::default(),
            tab: if cfg.copy_mode { Tab::Notes } else { Tab::New },
            settings: None,
            tabs_focus: false,
            worktrees: Some(background::snapshot(&format!("threads-{}", background::scope(cfg.repo.as_deref().unwrap_or(&cfg.cwd)))).unwrap_or_default()),
            tlist: crate::threadlist::ThreadList::new(crate::threadlist::Mode::Manage, None, cfg.repo.clone(), Vec::new()),
            tcursor: 0,
            toffset: 0,
            tpreview: HashMap::new(),
            thread_reads: Reads::default(),
            session_reads: Reads::default(),
            agent_reads: Reads::default(),
            jobs: Reads::default(),
            job_n: 0,
            detail_reads: Reads::default(),
            extra_reads: Reads::default(),
            note_reads: Reads::default(),
            sessions: background::snapshot("sessions").unwrap_or_default(),
            carry: Vec::new(),
            agents,
            usage: background::snapshot("usage"),
            // tests: a temp file per thread, never ~/.config/codehead.toml (changes apply on their own)
            settings_path: if cfg!(test) {
                std::env::temp_dir().join(format!("codehead-settings-app-{}-{:?}.toml", std::process::id(), std::thread::current().id()))
            } else {
                crate::config::path()
            },
            acursor: 0,
            aoffset: 0,
            afilter: None,
            afilter_editing: false,
            afilter_ta: TextArea::new(true, ""),
            aplace: Vec::new(),
            acards: Vec::new(),
            apreview: HashMap::new(),
            picks,
            pick_cursor: 0,
            pick_offset: 0,
            pick_places: Vec::new(),
            styles,
            global_style,
            previews: Previews::new(),
            pscroll: 0,
            screen: if cfg.copy_mode { Screen::Pick } else { Screen::Compose },
            title: t,
            prompt: p,
            field: Field::Prompt,
            target: None,
            modal: None,
            menu: None,
            menu_cmds: Vec::new(),
            menu_help: Vec::new(),
            toast: None,
            regions: Regions::default(),
            outcome: None,
            notes_panel: None,
            suspend: None,
            last_esc: None,
            settings_typed: false,
            draft_armed: None,
            last_ctrl_c: None,
            cfg,
        };
        app.rebuild();
        // what the task starts with (a title from the command line): not a draft yet
        app.draft_saved = app.draft().content();
        app.drafts = drafts::load(&app.cfg.drafts, &app.draft_id);
        if !synced.is_empty() {
            app.toast(format!("prompts synced with output-styles: {}", synced.join(", ")), false);
        }
        app.refresh_threads();
        app.refresh_agents();
        if !cfg!(test) {
            app.reload_notes();
            if let Some(r) = app.cfg.repo.clone() {
                let cwd = app.cfg.cwd.clone();
                app.where_reads.request(r.clone(), move || where_choices(&r, &cwd));
            }
        }
        app
    }

    /// Style for new agents: the global outputStyle, else mecha, else none.
    fn default_style(&self) -> Option<String> {
        let g = self.global_style.as_deref();
        self.styles.iter().find(|s| Some(s.name.as_str()) == g).or_else(|| self.styles.iter().find(|s| s.id == "mecha")).map(|s| s.id.clone())
    }

    // ---------- data ----------
    fn rebuild(&mut self) {
        let keep = self.rows.get(self.cursor).map(|r| r.path().to_string());
        self.rows = match &self.filter {
            Some(q) => self.notes.search(q),
            None => self.notes.tree(&self.collapsed),
        };
        if let Some(p) = keep
            && let Some(i) = self.rows.iter().position(|r| r.path() == p)
        {
            self.cursor = i;
        }
        self.cursor = self.cursor.min(self.rows.len().saturating_sub(1));
    }

    pub fn reload_notes(&mut self) {
        self.note_reads.invalidate();
        let root = self.cfg.notes.clone();
        self.note_reads.request((), move || Notes::load(&root));
    }

    /// Return immediately with retained UI state. Fresh data arrives later.
    pub fn return_from_thread(&mut self) {
        self.outcome = None;
        self.last_esc = None;
        self.last_ctrl_c = None;
        self.hover = None;
        self.thread_reads.invalidate();
        self.session_reads.invalidate();
        self.agent_reads.invalidate();
        self.refresh_threads();
        self.refresh_agents();
    }

    fn load_threads(&mut self) {
        self.thread_reads.invalidate();
        self.session_reads.invalidate();
        self.detail_reads.invalidate();
        self.refresh_threads();
    }

    fn refresh_threads(&mut self) {
        // tests: the reads give back the rows the test set (0 reads of the machine's threads)
        if cfg!(test) {
            let (rows, sessions) = (self.worktrees.clone().unwrap_or_default(), self.sessions.clone());
            self.thread_reads.request((), move || rows);
            self.session_reads.request((), move || sessions);
            return;
        }
        let repo = self.cfg.repo.clone();
        let name = format!("threads-{}", background::scope(repo.as_deref().unwrap_or(&self.cfg.cwd)));
        self.thread_reads.request((), move || {
            let rows = threads::load(repo.as_deref());
            background::save(&name, &rows);
            rows
        });
        self.session_reads.request((), || {
            let rows = claude::load(40);
            background::save("sessions", &rows);
            rows
        });
    }

    fn load_agents(&mut self) {
        self.agent_reads.invalidate();
        self.extra_reads.invalidate();
        self.refresh_agents();
    }

    /// Run 1-off action RUN off the UI thread; its Job comes back in `tick` (tests: at once).
    fn job(&mut self, run: impl FnOnce() -> Job + Send + 'static) {
        if cfg!(test) {
            let j = run();
            self.job_done(j);
            return;
        }
        self.job_n += 1;
        self.jobs.request(self.job_n, run);
    }

    fn job_done(&mut self, j: Job) {
        match j {
            Job::Threads(lines) => {
                self.load_threads();
                self.modal = Some(Modal::Info { title: "delete threads".into(), lines, scroll: 0 });
            }
            Job::AgentDeleted(lines) => {
                self.load_agents();
                self.worktrees = None;
                self.modal = Some(Modal::Info { title: "delete agent".into(), lines, scroll: 0 });
            }
            Job::Restarted(msg) => {
                self.usage = None;
                self.load_agents();
                self.toast(msg, false);
            }
        }
    }

    fn refresh_agents(&mut self) {
        self.agent_reads.request((), || {
            let rows = agents::load();
            let effort = agents::effort_name();
            let usage = agents::usage();
            background::save("usage", &usage);
            (rows, effort, usage)
        });
    }

    fn current_agent(&self) -> Option<&Agent> {
        self.agents.get(self.acursor)
    }

    fn uses(&self, a: &Agent) -> Vec<Use> {
        self.usage.as_ref().and_then(|u| u.get(&a.name)).cloned().unwrap_or_default()
    }

    fn current(&self) -> Option<&Row> {
        self.rows.get(self.cursor)
    }

    fn n_worktrees(&self) -> usize {
        self.worktrees.as_ref().map_or(0, Vec::len)
    }

    /// Rows of the threads tab's own views (worktrees without thread, Claude Code sessions) after
    /// the repo tab and the filter: indices into worktrees ++ sessions, best match first. The
    /// card views: 0 (the thread list has them).
    fn tvisible(&self) -> Vec<usize> {
        use crate::threadlist::View;
        let wts = self.worktrees.as_deref().unwrap_or(&[]);
        let nw = wts.len();
        let items: Vec<usize> = match self.tlist.view {
            View::Worktrees => (0..nw).filter(|&i| wts[i].thread.is_none()).collect(),
            View::Claude => (nw..nw + self.sessions.len()).collect(),
            View::Active | View::Archived => return Vec::new(),
        };
        let q = self.tlist.query().trim().to_lowercase();
        let name = |i: usize| -> String {
            if i < nw {
                wts[i].label()
            } else {
                let c = &self.sessions[i - nw];
                format!("{} {}", c.title, c.place())
            }
        };
        let mut v: Vec<(i64, usize)> = items
            .into_iter()
            .filter(|&i| self.tlist.keeps(self.trepo_of(i)))
            .filter_map(|i| if q.is_empty() { Some((0, i)) } else { fuzzy(&q, &name(i)).map(|sc| (-sc, i)) })
            .collect();
        v.sort();
        v.into_iter().map(|x| x.1).collect()
    }

    fn n_threads(&self) -> usize {
        self.tvisible().len()
    }

    /// The row of a card: the full read of its thread (preview: changes, agents, task), else
    /// the card's data.
    fn wt_of_card(&self, c: &crate::nav::Card) -> Worktree {
        self.worktrees.iter().flatten().find(|w| w.thread.as_ref().is_some_and(|t| t.id == c.id)).cloned().unwrap_or_else(|| worktree_of(c))
    }

    /// The thread / worktree under the cursor (cards: the thread list's).
    fn current_wt(&self) -> Option<Worktree> {
        if self.tlist.view.cards() {
            return self.tlist.current().map(|c| self.wt_of_card(c));
        }
        let i = *self.tvisible().get(self.tcursor)?;
        self.worktrees.as_ref()?.get(i).cloned()
    }

    fn current_session(&self) -> Option<&Session> {
        let i = *self.tvisible().get(self.tcursor)?;
        i.checked_sub(self.n_worktrees()).and_then(|j| self.sessions.get(j))
    }

    fn toggle_select(&mut self) {
        let Some(row) = self.current() else { return };
        let path = row.path().to_string();
        let inside = self.notes.inside(&path);
        if row.is_dir() {
            if inside.iter().all(|p| self.selected.contains(p)) {
                self.selected.retain(|s| !inside.contains(s));
            } else {
                for p in inside {
                    if !self.selected.contains(&p) {
                        self.selected.push(p);
                    }
                }
            }
        } else if let Some(i) = self.selected.iter().position(|s| *s == path) {
            self.selected.remove(i);
        } else {
            self.selected.push(path);
        }
    }

    /// ctrl+c, everywhere: a selection in the focused input → copied. Else quit; with unsent text
    /// (title or prompt) only on a 2nd ctrl+c within 1s.
    fn ctrl_c(&mut self) {
        let sel = match (self.screen, self.field) {
            (Screen::Compose, Field::Title) => self.title.selected_text(),
            (Screen::Compose, Field::Prompt) => self.prompt.selected_text(),
            _ => None,
        };
        if let Some(t) = sel {
            crate::clip::copy(&t);
            return self.toast(format!("copied {} chars", t.chars().count()), false);
        }

        // as in the thread view: the field's text → cleared (ctrl+z brings it back)
        let field = match (self.screen, self.field) {
            (Screen::Compose, Field::Title) => Some(&mut self.title),
            (Screen::Compose, Field::Prompt) => Some(&mut self.prompt),
            _ => None,
        };
        if let Some(ta) = field
            && !ta.text().is_empty()
        {
            ta.load_text("");
            return self.toast("cleared · ctrl+z: back", false);
        }

        // then: quit on a 2nd ctrl+c within 1 s (the task stays as a draft)
        if self.last_ctrl_c.is_some_and(|t| t.elapsed() < Duration::from_secs(1)) {
            self.outcome = Some(Outcome::Quit);
            return;
        }
        self.last_ctrl_c = Some(Instant::now());
        self.toast("ctrl+c again: quit", false);
    }

    fn toast(&mut self, msg: impl Into<String>, err: bool) {
        self.toast = Some((msg.into(), Instant::now(), err));
    }

    /// Background reads in. True: something on screen may change (a redraw is due).
    pub fn tick(&mut self) -> bool {
        let mut dirty = self.previews.poll();
        // the where row's branches: the pick stays on its choice
        for (repo, c) in self.where_reads.poll() {
            if self.cfg.repo.as_ref() != Some(&repo) || self.repo.as_ref().is_some_and(|r| r.url.is_some()) {
                continue;
            }
            let picked = self.wheres.get(self.where_i).cloned();
            self.wheres = c.wheres;
            self.checkouts = c.checkouts;
            self.repo_branch = c.repo_branch;
            self.here_branch = c.here_branch;
            self.here_dir = c.here_dir;
            self.where_i = picked.and_then(|p| self.wheres.iter().position(|w| *w == p)).unwrap_or(0);
            dirty = true;
        }
        // the new-thread view opens: the plan windows start now, the task can take its time
        if self.screen == Screen::Pick && self.tab == Tab::Merge {
            dirty |= self.merge_tab.poll();
            if let Some(n) = self.merge_tab.take_note() {
                self.toast(n, false);
                dirty = true;
            }
        }
        let compose = self.screen == Screen::Compose && !self.cfg.copy_mode;
        if compose && !self.warmed {
            crate::engine::warm::warm();
        }
        self.warmed = compose;
        // avatars on screen (agents tab, agent form, picker, menu): a frame every 150 ms
        if matches!(self.modal, Some(Modal::Agent(_))) || self.cur_tab() == Tab::Agents || self.menu.is_some() {
            use std::sync::atomic::{AtomicU64, Ordering};
            static LAST: AtomicU64 = AtomicU64::new(0);
            let ms = (crate::avatar::now() * 1000.0) as u64;
            if ms.saturating_sub(LAST.load(Ordering::Relaxed)) >= 150 {
                LAST.store(ms, Ordering::Relaxed);
                dirty = true;
            }
        }
        // the settings' MCP tab: harness lists, probes, sign-ins in the background
        if let Some(s) = self.settings.as_mut() {
            dirty |= s.poll();
        }
        // the thread list: its reads, its jobs (archive, delete, edit: this screen's rows again),
        // the menu's jobs too
        let shown = self.screen == Screen::Pick && self.tab == Tab::Threads;
        let busy = self.tlist.waiting();
        self.tlist.poll(shown);
        let menu_job = self.menu.as_mut().is_some_and(|m| m.list.take_changed());
        if self.tlist.take_changed() || menu_job {
            self.load_threads();
            self.tlist.reload();
        }
        dirty |= busy || self.tlist.waiting();
        if self.tlist.roster.len() != self.agents.len() {
            self.tlist.roster = self.agents.iter().map(|a| a.name.clone()).collect();
        }
        let keep = if self.tlist.view.cards() { None } else { self.current_wt().map(|w| w.key()).or_else(|| self.current_session().map(|s| s.key())) };
        let mut changed = false;
        for (_, rows) in self.thread_reads.poll() {
            if self.worktrees.as_ref() == Some(&rows) {
                continue;
            }
            self.threads_version += 1;
            self.worktrees = Some(rows);
            changed = true;
        }
        for (_, rows) in self.session_reads.poll() {
            if self.sessions == rows {
                continue;
            }
            self.threads_version += 1;
            self.sessions = rows;
            changed = true;
        }
        dirty |= changed;
        if changed || (shown && self.tlist.extra[1].0 != self.sessions.len()) {
            // the thread list's tabs: the counts and repos of this screen's rows
            let wts = self.worktrees.as_deref().unwrap_or(&[]);
            let orphans: Vec<Option<PathBuf>> = wts.iter().filter(|w| w.thread.is_none()).map(|w| w.repo.clone()).collect();
            let sessions: Vec<Option<PathBuf>> = self.sessions.iter().map(|s| s.repo.clone()).collect();
            self.tlist.extra = [(orphans.len(), orphans), (sessions.len(), sessions)];
        }
        if changed {
            let vis = self.tvisible();
            let nw = self.n_worktrees();
            self.tcursor = keep.and_then(|key| vis.iter().position(|&i| {
                if i < nw { self.worktrees.as_ref().unwrap()[i].key() == key } else { self.sessions[i - nw].key() == key }
            })).unwrap_or_else(|| self.tcursor.min(vis.len().saturating_sub(1)));
        }
        for (_, j) in self.jobs.poll() {
            dirty = true;
            self.job_done(j);
        }
        for (_, (rows, effort, usage)) in self.agent_reads.poll() {
            dirty = true;
            let keep = self.current_agent().map(|a| a.name.clone());
            self.agents = rows;
            self.effort = effort;
            self.usage = Some(usage);
            self.acursor = keep.and_then(|name| self.agents.iter().position(|a| a.name == name)).unwrap_or_else(|| self.acursor.min(self.agents.len().saturating_sub(1)));
            let names: HashSet<String> = self.agents.iter().map(|a| a.name.clone()).collect();
            self.picks.retain(|p| names.contains(p));
            if self.picks.is_empty() && names.contains(&self.cfg.agent) { self.picks.push(self.cfg.agent.clone()); }
        }
        let (details, extras) = (self.detail_reads.poll(), self.extra_reads.poll());
        dirty |= !details.is_empty() || !extras.is_empty();
        self.tpreview.extend(details);
        self.apreview.extend(extras);
        for (_, notes) in self.note_reads.poll() {
            dirty = true;
            self.notes = notes;
            self.selected.retain(|s| self.notes.files.contains(s));
            self.rebuild();
        }
        match self.cur_tab() {
            Tab::Threads => self.refresh_threads(),
            Tab::Agents => self.refresh_agents(),
            Tab::Notes => {
                let root = self.cfg.notes.clone();
                self.note_reads.request((), move || Notes::load(&root));
            }
            _ => {}
        }
        for (h, res) in self.model_reads.poll() {
            dirty = true;
            match res {
                Ok(e) => {
                    self.model_errors.remove(&h);
                    self.models.get_or_insert_with(HashMap::new).insert(h, e);
                }
                Err(e) => {
                    self.model_errors.insert(h, e);
                }
            }
        }
        if self.toast.as_ref().is_some_and(|t| t.1.elapsed() > Duration::from_secs(3)) {
            self.toast = None;
            dirty = true;
        }
        // the menu animates (working waves) and reads in the background; the > dropdown reads too
        dirty || self.menu.is_some() || self.command.busy()
    }

    // ---------- flow ----------
    /// Enter on the notes list: only the space-selected notes are the context (0 allowed).
    /// Copy mode (-c) with 0 selected: copies the row under the cursor.
    fn confirm_notes(&mut self) {
        if self.cfg.copy_mode {
            if self.selected.is_empty()
                && let Some(row) = self.current()
            {
                self.selected = self.notes.inside(row.path());
            }
            self.outcome = Some(Outcome::Copy(self.selected.iter().map(|p| self.notes.abs(p)).collect()));
            return;
        }
        self.screen = Screen::Compose;
        self.field = Field::Notes;
    }

    fn open_compose(&mut self) {
        self.screen = Screen::Compose;
        self.tabs_focus = false;
        self.field = Field::Prompt;
    }

    /// Enter on the task's notes field: the notes picker.
    fn open_notes(&mut self) {
        self.screen = Screen::Pick;
        self.tab = Tab::Notes;
        self.tabs_focus = false;
        self.pscroll = 0;
    }

    /// Claude Code session → task: its dir, its title, its last response as context.
    fn pick_session(&mut self, s: Session) {
        if self.title.is_empty() {
            self.title.set_text(&s.title);
        }
        self.target = Some(Target::Dir(s.cwd.clone()));
        self.toast(format!("context: last response of {}", s.title), false);
        if !self.carry.iter().any(|c| c.key() == s.key()) {
            self.carry.push(s);
        }
        self.open_compose();
    }

    /// c on a threads row: its last response in / out of the context. Rows without a transcript: toast.
    fn toggle_context(&mut self) {
        let s = if let Some(s) = self.current_session() {
            Some(s.clone())
        } else if let Some(t) = self.current_wt().and_then(|w| w.thread.clone()) {
            if let Some(i) = self.carry.iter().position(|c| c.thread.as_deref() == Some(&t.id)) {
                let c = self.carry.remove(i);
                return self.toast(format!("context −: {} · {} left", c.title, self.carry.len()), false);
            }
            match claude::of_thread(&t.id, &t.title) {
                Some(s) => Some(s),
                None => return self.toast("0 Claude responses in this thread", true),
            }
        } else {
            None
        };
        let Some(s) = s else { return self.toast("worktree without thread: 0 context", true) };
        if let Some(i) = self.carry.iter().position(|c| c.key() == s.key()) {
            self.carry.remove(i);
            self.toast(format!("context −: {} · {} left", s.title, self.carry.len()), false);
        } else {
            let title = s.title.clone();
            self.carry.push(s);
            self.toast(format!("context +: {title} · {} in total", self.carry.len()), false);
        }
        if self.tlist.view.cards() {
            if self.tlist.cursor + 1 < self.tlist.visible().len() {
                self.tlist.cursor += 1;
            }
        } else if self.tcursor + 1 < self.n_threads() {
            self.tcursor += 1;
        }
        self.pscroll = 0;
    }

    fn in_context(&self, key: &str) -> bool {
        self.carry.iter().any(|c| c.key() == key)
    }

    fn open_thread(&mut self) {
        if let Some(s) = self.current_session().cloned() {
            return self.pick_session(s);
        }
        let Some(w) = self.current_wt() else { return };
        if w.thread.is_some() {
            self.outcome = Some(Outcome::Resume(Box::new(w)));
        } else {
            self.target = Some(Target::Worktree { path: w.path, branch: w.branch });
            self.open_compose();
        }
    }

    /// Card C of the thread list opens (an archived thread: the open unarchives it, its host
    /// makes the worktree again).
    fn open_card(&mut self, c: &crate::nav::Card) {
        self.outcome = Some(Outcome::Resume(Box::new(self.wt_of_card(c))));
    }

    fn target(&self) -> Result<Target, String> {
        if let Some(t) = &self.target {
            return Ok(t.clone());
        }
        if self.cfg.repo.is_none() {
            return Ok(Target::InPlace);
        }
        if let Some(b) = &self.cfg.branch {
            return Ok(Target::Branch(b.clone()));
        }
        let w = self.wheres.get(self.where_i).cloned().unwrap_or(Where::NewWorktree);
        if w == Where::Current {
            return Ok(Target::Worktree { path: self.here_dir.clone(), branch: self.here_branch.clone() });
        }
        if let Where::Branch(b) = w {
            let (switch, _) = self.checkout_box();
            let (stash, _) = self.stash_box();
            return Ok(Target::Checkout { branch: b, switch, stash });
        }
        let s = slug(&self.task_title());
        if s.is_empty() {
            return Err("title needs letters or digits (branch name)".into());
        }
        let b = format!("{}/{s}", self.cfg.prefix);
        Ok(if w == Where::NewBranch { Target::NewBranch(b) } else { Target::Branch(b) })
    }

    fn repo_text(&self) -> String {
        self.cfg.repo.as_ref().map(|r| tilde(&r.display().to_string())).unwrap_or_default()
    }

    /// Short label of the where choice (the chip of the where row).
    fn where_chip(&self) -> String {
        match (&self.target, self.wheres.get(self.where_i)) {
            (Some(Target::Worktree { branch, .. }), _) => format!("worktree {branch}"),
            (Some(Target::Dir(_)), _) => "session dir".into(),
            (Some(_), _) | (None, None) => "here".into(),
            (None, Some(Where::Current)) if self.here_branch.is_empty() => "current branch (detached)".into(),
            (None, Some(Where::Current)) => format!("current branch · {}", self.here_branch),
            (None, Some(Where::Branch(b))) => b.clone(),
            (None, Some(Where::NewBranch)) => "+ new branch".into(),
            (None, Some(Where::NewWorktree)) => "+ new branch + worktree".into(),
        }
    }

    /// Enter / click on the repo row: the repo picker on the repo (else the dir codehead runs in).
    fn open_repo_pick(&mut self) {
        let at = self.repo.as_ref().map(|r| r.path.clone()).filter(|p| p.exists()).unwrap_or_else(|| self.cfg.cwd.clone());
        self.repo_pick = Some(crate::repopick::Picker::new(&at, self.repos.clone()));
    }

    /// A pick of the repo picker: E's repo and its branch choices. The start dir (or a dir above
    /// it in its repo): the dir codehead started in, current branch first; another repo: its dir,
    /// a new branch + worktree first; a home repo without a clone: a new worktree after the clone.
    /// 0 git repo: a toast, the repo stays.
    fn set_repo(&mut self, e: crate::repopick::Entry) {
        let root = if e.clone.is_some() { Some(e.path.clone()) } else { threads::repo_root(&e.path) };
        let Some(root) = root else {
            return self.toast(format!("{}: 0 git repo", tilde(&e.path.display().to_string())), true);
        };
        self.repo_pick = None;
        self.target = None;
        let start = self.start_cwd.starts_with(&e.path) && threads::repo_root(&self.start_cwd).as_ref() == Some(&root);
        let cwd = if start { self.start_cwd.clone() } else { e.path.clone() };
        self.cfg.repo = Some(root.clone());
        self.cfg.cwd = cwd.clone();
        let c = if e.clone.is_some() {
            Choices { wheres: vec![Where::NewWorktree], here_dir: root.display().to_string(), ..Default::default() }
        } else if cfg!(test) {
            where_choices(&root, &cwd)
        } else {
            // the branches: from the background read (`tick`)
            let (r, d) = (root.clone(), cwd.clone());
            self.where_reads.request(root.clone(), move || where_choices(&r, &d));
            quick_choices(&root, &cwd)
        };
        self.wheres = c.wheres;
        self.checkouts = c.checkouts;
        self.repo_branch = c.repo_branch;
        self.here_branch = c.here_branch;
        self.here_dir = c.here_dir;
        self.where_i = if start { 0 } else { self.wheres.iter().position(|w| *w == Where::NewWorktree).unwrap_or(0) };
        let name = root.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        self.repo = Some(crate::home::Repo { name, path: root, url: e.clone });
        self.flow_default = None;
        self.load_task_flows();
    }

    /// The setting `repo` (DIR): the where row starts on DIR's repo, as if picked. DIR in the
    /// repo codehead started in: 0 change.
    pub fn use_default_repo(&mut self, dir: &Path) {
        let Some(root) = threads::repo_root(dir) else { return };
        if self.cfg.repo.as_ref() == Some(&root) {
            return;
        }
        let name = root.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        self.set_repo(crate::repopick::Entry { name, path: root, git: true, clone: None });
    }

    /// Keys of the open repo picker.
    fn repo_pick_key(&mut self, k: KeyEvent) {
        let Some(p) = self.repo_pick.as_mut() else { return };
        match p.key(k) {
            crate::repopick::Act::None => {}
            crate::repopick::Act::Close => self.repo_pick = None,
            crate::repopick::Act::Pick(e) => self.set_repo(e),
        }
    }

    /// The where row's repo, when it needs a clone first: (url, path).
    fn clone_first(&self) -> Option<(String, PathBuf)> {
        let r = self.repo.as_ref()?;
        (!r.path.join(".git").exists()).then(|| r.url.clone().map(|u| (u, r.path.clone()))).flatten()
    }

    /// Text + detail of where choice W: a row of the where dropdown.
    fn where_label(&self, w: &Where) -> (String, String) {
        match w {
            Where::Current => ("current branch".into(), if self.here_branch.is_empty() { "detached".into() } else { self.here_branch.clone() }),
            Where::NewBranch => ("+ new branch".into(), "git switch -c · 0 worktrees".into()),
            Where::NewWorktree => ("+ new branch + worktree".into(), "git worktree add".into()),
            Where::Branch(b) => (b.clone(), self.checkouts.get(b).map_or_else(|| "branch".into(), |p| format!("worktree {}", tilde(p)))),
        }
    }

    /// Rows of the where dropdown, top to bottom (indexes of WHERES): the branches (the 1st choice,
    /// main or the newest, right above the rest), new branch + worktree, new branch, current
    /// branch at the bottom. With Q: the matches in that order by score, the best at the bottom.
    fn where_rows(&self, q: &str) -> Vec<usize> {
        let mut v: Vec<usize> = (0..self.wheres.len()).filter(|&i| matches!(self.wheres[i], Where::Branch(_))).rev().collect();
        for w in [Where::NewWorktree, Where::NewBranch, Where::Current] {
            v.extend(self.wheres.iter().position(|x| *x == w));
        }
        let q = q.trim().to_lowercase();
        if q.is_empty() {
            return v;
        }
        let mut m: Vec<(i64, usize, usize)> = v
            .into_iter()
            .enumerate()
            .filter_map(|(pos, i)| {
                let (label, detail) = self.where_label(&self.wheres[i]);
                let text = if self.wheres[i] == Where::Current { format!("{label} {detail}") } else { label };
                fuzzy(&q, &text).map(|s| (s, pos, i))
            })
            .collect();
        m.sort_by_key(|&(s, pos, _)| (s, pos));
        m.into_iter().map(|(_, _, i)| i).collect()
    }

    /// Enter / space / click on the where row: the dropdown, its cursor on the current choice.
    fn open_where(&mut self) {
        let rows = self.where_rows("");
        if rows.is_empty() {
            return;
        }
        let cursor = rows.iter().position(|&i| i == self.where_i).unwrap_or(rows.len() - 1);
        self.where_menu = Some(WhereMenu { cursor, offset: 0, query: String::new() });
    }

    /// Choice I (of WHERES) from the dropdown. A target from the threads tab gives way to it.
    fn pick_where(&mut self, i: usize) {
        self.where_menu = None;
        self.repo_pick = None;
        self.target = None;
        self.where_i = i;
    }

    /// Keys of the open where dropdown: ↑↓ move, type: filter, enter: pick, esc: close.
    fn where_key(&mut self, k: KeyEvent) {
        let Some(q) = self.where_menu.as_ref().map(|m| m.query.clone()) else { return };
        let rows = self.where_rows(&q);
        let n = rows.len();
        let typed = !k.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER);
        let mut query = None;
        let Some(m) = self.where_menu.as_mut() else { return };
        match k.code {
            KeyCode::Esc => self.where_menu = None,
            KeyCode::Up if n > 0 => m.cursor = (m.cursor + n - 1) % n,
            KeyCode::Down if n > 0 => m.cursor = (m.cursor + 1) % n,
            KeyCode::PageUp => m.cursor = m.cursor.saturating_sub(dropdown::MAX_ROWS),
            KeyCode::PageDown if n > 0 => m.cursor = (m.cursor + dropdown::MAX_ROWS).min(n - 1),
            KeyCode::Enter => {
                if let Some(&i) = rows.get(m.cursor) {
                    self.pick_where(i);
                }
            }
            KeyCode::Backspace | KeyCode::Char('u' | 'w' | 'h') if {
                let mut s = q.clone();
                crate::textarea::delete_key(&mut s, &k) && {
                    query = Some(s);
                    true
                }
            } => {}
            KeyCode::Char(c) if typed => query = Some(format!("{q}{c}")),
            _ => {}
        }
        // new filter: the cursor on the best match (bottom); 0 filter: on the current choice
        if let Some(q) = query {
            let rows = self.where_rows(&q);
            let cursor = if q.is_empty() { rows.iter().position(|&i| i == self.where_i) } else { None }.unwrap_or(rows.len().saturating_sub(1));
            self.where_menu = Some(WhereMenu { cursor, offset: 0, query: q });
        }
    }

    /// Checkout box: (checked, enabled). Fixed on for the current branch and for branches that
    /// another checkout has (work goes there); off + disabled for new branches.
    fn checkout_box(&self) -> (bool, bool) {
        if self.target.is_some() {
            return (false, false);
        }
        match self.wheres.get(self.where_i) {
            Some(Where::Current) => (true, false),
            Some(Where::Branch(b)) if self.checkouts.contains_key(b) => (true, false),
            Some(Where::Branch(_)) => (self.checkout, true),
            _ => (false, false),
        }
    }

    /// Stash box: (checked, enabled). Only when the checkout box is on and a switch happens.
    fn stash_box(&self) -> (bool, bool) {
        match self.checkout_box() {
            (true, true) => (self.stash, true),
            _ => (false, false),
        }
    }

    /// Space / click on an opts box.
    fn toggle_opt(&mut self, i: usize) {
        self.opt_i = i;
        match (i, self.checkout_box(), self.stash_box()) {
            (0, (_, true), _) => self.checkout = !self.checkout,
            (1, _, (_, true)) => self.stash = !self.stash,
            (0, _, _) => self.toast("checkout: fixed for this choice", true),
            _ => self.toast("stash: only with a checkout", true),
        }
    }

    fn where_text(&self) -> String {
        if let Some((url, path)) = self.clone_first() {
            let b = match self.target() {
                Ok(Target::Branch(b)) => b,
                _ => "…".into(),
            };
            return format!("clones {url} into {}, then a new worktree · branch {b}", tilde(&path.display().to_string()));
        }
        if self.target.is_none() && self.wheres.get(self.where_i) == Some(&Where::Current) {
            return format!("works in {} · 0 checkout", tilde(&self.here_dir));
        }
        match self.target() {
            Ok(Target::Branch(b)) => format!("new worktree · branch {b}"),
            Ok(Target::Checkout { branch: b, switch, stash }) => match self.checkouts.get(&b) {
                Some(p) => format!("works in {} (checked out there)", tilde(p)),
                None if !switch => format!("works in {} on {} · 0 checkout", self.repo_text(), self.repo_branch),
                None if stash => format!("switches {} from {} to {b} · conflicting changes → git stash", self.repo_text(), self.repo_branch),
                None => format!("switches {} from {} to {b}", self.repo_text(), self.repo_branch),
            },
            Ok(Target::NewBranch(b)) => format!("git switch -c {b} in {} (from {}) · 0 worktrees", self.repo_text(), self.repo_branch),
            Ok(Target::Worktree { branch, .. }) => format!("existing worktree · branch {branch}"),
            Ok(Target::InPlace) => format!("in place · {}", self.cfg.cwd.display()),
            Ok(Target::Dir(d)) => format!("in place · {d} (dir of the Claude Code session)"),
            Err(_) if self.wheres.get(self.where_i) == Some(&Where::NewBranch) => format!("new branch {}/… (from the title)", self.cfg.prefix),
            Err(_) => format!("new worktree · branch {}/… (from the title)", self.cfg.prefix),
        }
    }

    /// The thread's title: the title field, else the prompt's 1st line (max. 80 chars).
    fn task_title(&self) -> String {
        let t = self.title.text().trim().to_string();
        if !t.is_empty() {
            return t;
        }
        let line = self.prompt.text().lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or_default().to_string();
        if line.chars().count() > 80 { line.chars().take(79).collect::<String>() + "…" } else { line }
    }

    /// Thread message: line 1 = the thread title, the notes to read, the Claude Code context, then the
    /// prompt. 0 title: the prompt comes first (its 1st line: the title), 0 line twice.
    pub fn message(&self) -> String {
        let derived = self.title.text().trim().is_empty();
        // `#name` of a shortcut: its text
        let text = crate::shortcuts::expand(&self.prompt.text());
        let prompt = crate::textarea::without_empty_items(&text);
        let mut parts = Vec::new();
        if derived {
            parts.push(crate::config::expand_aliases(prompt.trim()));
        } else {
            parts.push(self.title.text().trim().to_string());
        }
        if !self.selected.is_empty() {
            let list: Vec<String> = self.selected.iter().map(|p| format!("- {}", self.notes.abs(p).display())).collect();
            parts.push(format!("Read these notes first:\n{}", list.join("\n")));
        }
        parts.extend(self.carry.iter().map(claude::context));
        if !derived && !prompt.trim().is_empty() {
            parts.push(crate::config::expand_aliases(prompt.trim_end()));
        }
        // pasted images, long texts: their markers in the prompt, the files here
        let files: Vec<String> = self.files.iter().filter(|(m, _)| prompt.contains(m.as_str())).map(|(m, p)| format!("- {m}: {}", p.display())).collect();
        if !files.is_empty() {
            parts.push(format!("Attachments (read these files):\n{}", files.join("\n")));
        }
        parts.join("\n\n")
    }

    fn start(&mut self) {
        // the title: optional (the prompt's 1st line); a prompt or a title: required
        if self.task_title().is_empty() {
            self.field = Field::Prompt;
            self.toast("type the task first", true);
            return;
        }
        match self.target() {
            Ok(_) if self.picks.is_empty() => {
                self.field = Field::Agents;
                self.toast("pick 1+ agents (space)", true);
            }
            Ok(target) => {
                let agents: Vec<String> = self.agents.iter().filter(|a| self.picks.contains(&a.name)).map(|a| a.name.clone()).collect();
                // each agent's flow (its start step); the lead's: the repo's next default
                let flows: Vec<(String, String, usize)> = agents.iter().map(|a| (a.clone(), self.flow_of(a))).filter(|(_, (s, _))| !s.is_empty()).map(|(a, (s, st))| (a, s, st)).collect();
                if let Some(lead) = agents.first()
                    && let Some((_, slug, _)) = flows.iter().find(|(a, _, _)| a == lead)
                {
                    flow_default_set(&self.flow_key(), slug);
                }
                self.outcome = Some(Outcome::New { message: self.message(), target, agents, clone: self.clone_first(), flows })
            }
            Err(e) => {
                self.field = Field::Title;
                self.toast(e, true);
            }
        }
    }

    // ---------- drafts ----------
    /// The task as it is now.
    fn draft(&self) -> Draft {
        let place = self.cfg.repo.as_ref().unwrap_or(&self.cfg.cwd).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        Draft {
            id: self.draft_id.clone(),
            saved: 0,
            place,
            title: self.title.text(),
            prompt: self.prompt.text(),
            notes: self.selected.clone(),
            picks: self.picks.clone(),
            place_choice: self.wheres.get(self.where_i).map(|w| format!("{w:?}")).unwrap_or_default(),
            checkout: self.checkout,
            stash: self.stash,
            files: self.files.iter().filter(|(m,_)| self.prompt.text().contains(m)).map(|(m, p)| (m.clone(), p.display().to_string())).collect(),
            carry: self.carry.iter().map(|c| (c.thread.clone(), c.path.display().to_string(), c.title.clone())).collect(),
            kept: self.draft_kept,
        }
    }

    /// After every key, paste, click: the task to its draft file, if it changed. 0 title and
    /// 0 prompt: 0 file.
    fn backup(&mut self) {
        if self.cfg.copy_mode || self.outcome.is_some() {
            return;
        }
        let d = self.draft();
        let content = d.content();
        if content == self.draft_saved {
            return;
        }
        if d.is_empty() {
            drafts::remove(&self.cfg.drafts, &self.draft_id);
        } else if let Err(e) = drafts::save(&self.cfg.drafts, &d) {
            self.toast(format!("draft not saved: {e}"), true);
        }
        self.draft_saved = content;
    }

    /// The thread started: its draft goes.
    pub fn drop_draft(&self) {
        drafts::remove(&self.cfg.drafts, &self.draft_id);
    }

    /// ctrl+s: the task as a kept draft (0 cleanup by age), the form empty for the next one. The
    /// drafts row loads it later.
    fn save_draft(&mut self) {
        let mut d = self.draft();
        if d.is_empty() {
            return self.toast("0 title, 0 prompt: nothing to save", true);
        }
        d.kept = true;
        if let Err(e) = drafts::save(&self.cfg.drafts, &d) {
            return self.toast(format!("draft not saved: {e}"), true);
        }
        let name = d.name();
        // a new id first: clear_task drops the draft of the id on screen
        self.draft_id = drafts::new_id();
        self.clear_task();
        self.drafts = drafts::load(&self.cfg.drafts, &self.draft_id);
        self.toast(format!("draft saved: {name} · drafts row: load it"), false);
    }

    /// The thread started: the new task form empty again (defaults), with a draft id of its own.
    pub fn clear_task(&mut self) {
        self.drop_draft();
        self.title.set_text("");
        self.prompt.set_text("");
        self.selected.clear();
        self.files.clear();
        self.carry.clear();
        self.target = None;
        self.checkout = true;
        self.stash = false;
        self.where_i = if self.cfg.branch.is_some() { self.wheres.iter().position(|w| *w == Where::NewWorktree).unwrap_or(0) } else { 0 };
        let mut picks: Vec<String> = self.cfg.picks.iter().filter(|n| self.agents.iter().any(|a| &a.name == *n)).cloned().collect();
        if picks.is_empty() && self.agents.iter().any(|a| a.name == self.cfg.agent) {
            picks.push(self.cfg.agent.clone());
        }
        self.picks = picks;
        let home = PathBuf::from(std::env::var("HOME").unwrap_or_default());
        self.paster = Paster::new(home.join(".cache/codehead/paste").join(format!("new-{}", (crate::engine::now_ms() / 1000))));
        self.draft_id = drafts::new_id();
        self.draft_kept = false;
        self.draft_saved = self.draft().content();
        self.field = Field::Prompt;
    }

    /// "n" of the menu: the form empty, also in a draft. The task on screen stays a draft (its
    /// file: written by backup), listed in "view drafts (n)".
    fn new_task(&mut self) {
        self.backup();
        // a new id first: clear_task drops the draft of the id on screen
        self.draft_id = drafts::new_id();
        self.clear_task();
        self.drafts = drafts::load(&self.cfg.drafts, &self.draft_id);
    }

    /// The button of the drafts row under the cursor: "view drafts" only with 1+ drafts.
    fn draft_btn(&self) -> usize {
        if self.drafts.is_empty() { 1 } else { self.draft_btn.min(1) }
    }

    fn open_drafts(&mut self) {
        self.drafts = drafts::load(&self.cfg.drafts, &self.draft_id);
        if self.drafts.is_empty() {
            self.field = Field::Prompt;
            return;
        }
        // the latest (last) row is current
        self.drafts_menu = Some((self.drafts.len() - 1, 0));
    }

    fn drafts_key(&mut self, k: KeyEvent) {
        let Some((cur, _)) = self.drafts_menu else { return };
        let n = self.drafts.len();
        if n == 0 {
            self.drafts_menu = None;
            return;
        }
        let set = |s: &mut App, c: usize| {
            if let Some(m) = &mut s.drafts_menu {
                m.0 = c;
            }
        };
        let armed = self.draft_armed.take() == Some(cur);
        match k.code {
            KeyCode::Esc => self.drafts_menu = None,
            KeyCode::Up | KeyCode::Char('k') => set(self, (cur + n - 1) % n),
            KeyCode::Down | KeyCode::Char('j') => set(self, (cur + 1) % n),
            KeyCode::Enter => self.load_draft(cur),
            // d d: delete the draft (a 1st d asks)
            KeyCode::Char('d') | KeyCode::Delete if !armed => {
                self.draft_armed = Some(cur);
                self.toast("d again: delete the draft", false);
            }
            KeyCode::Char('d') | KeyCode::Delete => {
                let d = self.drafts.remove(cur.min(n - 1));
                match trash(&drafts::path(&self.cfg.drafts, &d.id)) {
                    Ok(how) => self.toast(format!("draft {how}: {}", d.name()), false),
                    Err(e) => self.toast(format!("draft not deleted: {e}"), true),
                }
                if self.drafts.is_empty() {
                    self.drafts_menu = None;
                    self.field = Field::Prompt;
                } else {
                    set(self, cur.min(self.drafts.len() - 1));
                }
            }
            _ => {}
        }
    }

    /// Draft I into the task screen. The task on screen stays a draft of its own (its file is
    /// written already); the loaded draft's file is the task's file from now on.
    fn load_draft(&mut self, i: usize) {
        self.drafts_menu = None;
        let Some(d) = self.drafts.get(i).cloned() else { return };
        self.draft_id = d.id.clone();
        self.draft_kept = d.kept;
        self.title.set_text(&d.title);
        self.prompt.set_text(&d.prompt);
        self.selected = d.notes.iter().filter(|n| self.notes.files.contains(n)).cloned().collect();
        let picks: Vec<String> = d.picks.iter().filter(|p| self.agents.iter().any(|a| &a.name == *p)).cloned().collect();
        if !picks.is_empty() {
            self.picks = picks;
        }
        if let Some(w) = self.wheres.iter().position(|w| format!("{w:?}") == d.place_choice) {
            self.where_i = w;
        }
        self.checkout = d.checkout;
        self.stash = d.stash;
        self.target = None;
        self.files = d.files.iter().map(|(m, p)| (m.clone(), PathBuf::from(p))).collect();
        // new pastes: numbers after the draft's markers
        for (m, _) in &self.files {
            self.paster.seen(m);
        }
        self.carry = d
            .carry
            .iter()
            .filter_map(|(thread, path, title)| match thread {
                Some(t) => claude::of_thread(t, title),
                None => claude::from_transcript(std::path::Path::new(path)),
            })
            .collect();
        self.field = Field::Prompt;
        self.draft_saved = self.draft().content();
        self.drafts = drafts::load(&self.cfg.drafts, &self.draft_id);
        self.toast(format!("draft loaded: {}", d.name()), false);
    }

    /// The open drafts dropdown, below its field.
    fn draw_drafts(&mut self, f: &mut Frame, bounds: Rect) {
        let Some((cur, off)) = self.drafts_menu else { return };
        let n = self.drafts.len();
        // kept (ctrl+s): `★ saved` in the detail
        let rows: Vec<(String, String)> = self
            .drafts
            .iter()
            .map(|d| (d.name(), format!("{}{} · {}", if d.kept { "★ saved · " } else { "" }, d.place, crate::notes::ago(std::time::UNIX_EPOCH + Duration::from_secs(d.saved)))))
            .collect();
        let w = rows.iter().map(|(a, b)| a.width().min(60) + b.width() + 5).max().unwrap_or(0).clamp(44, 96) as u16;
        let Some(r) = dropdown::place(self.regions.drafts, w, n, bounds, false) else { return };
        let shown = (r.height as usize).saturating_sub(2);
        let off = dropdown::fit(off, cur, n, shown);
        self.drafts_menu = Some((cur, off));
        let inner = dropdown::frame(f, r, "drafts", "enter: load · d d: delete · esc: back", off > 0, off + shown < n, true);
        for (i, (name, detail)) in rows.iter().enumerate().skip(off).take(shown) {
            let row = Rect { y: inner.y + (i - off) as u16, height: 1, ..inner };
            let hl = i == cur;
            let st = if hl { theme::pill().add_modifier(Modifier::BOLD) } else { Style::default().fg(theme::pal().text) };
            let dim = if hl { theme::pill() } else { theme::muted() };
            let room = (row.width as usize).saturating_sub(detail.width() + 4);
            let name: String = if name.width() > room { name.chars().take(room.saturating_sub(1)).chain(['…']).collect() } else { name.clone() };
            let pad = room.saturating_sub(name.width());
            let line = Line::from(vec![Span::styled(format!(" {name}"), st), Span::styled(" ".repeat(pad + 2), st), Span::styled(format!("{detail} "), dim)]);
            f.render_widget(Paragraph::new(line).style(st), row);
            self.regions.draft_rows.push((row, i));
        }
    }

    /// The open where dropdown, above the where chip (its rows grow upwards: current branch at the
    /// bottom, next to the chip). The current choice: `●`.
    fn draw_where_menu(&mut self, f: &mut Frame, bounds: Rect) {
        let Some((q, cur, off)) = self.where_menu.as_ref().map(|m| (m.query.clone(), m.cursor, m.offset)) else { return };
        let rows = self.where_rows(&q);
        let n = rows.len();
        let items: Vec<dropdown::Row> = rows
            .iter()
            .map(|&i| {
                let (text, detail) = self.where_label(&self.wheres[i]);
                dropdown::Row { text, detail, now: i == self.where_i && self.target.is_none(), marks: Vec::new() }
            })
            .collect();
        let w = items.iter().map(|r| r.text.width() + r.detail.width() + 8).max().unwrap_or(0).clamp(40, 96) as u16;
        let Some(r) = dropdown::place(self.regions.where_chip, w, n, bounds, true) else { return };
        let cur = cur.min(n.saturating_sub(1));
        let label = if q.is_empty() { "where".to_string() } else { format!("where › {q}") };
        let note = if q.is_empty() { "type: filter".to_string() } else { format!("{n} of {}", self.wheres.len()) };
        let (off, drawn) = dropdown::list(f, r, &label, &note, &items, cur, off, "0 matches");
        if let Some(m) = self.where_menu.as_mut() {
            m.cursor = cur;
            m.offset = off;
        }
        self.regions.where_rows = drawn.into_iter().map(|(row, j)| (row, rows[j])).collect();
    }

    // ---------- harness + model dropdowns of the agent form ----------
    /// Rows of the harness dropdown: the installed harnesses (label, bin), the form's harness
    /// too when it is not installed. (value, row).
    fn harness_items(form: &AgentForm) -> Vec<(String, dropdown::Row)> {
        let cur = form.harness_name();
        let mut v: Vec<(String, dropdown::Row)> =
            form.harnesses.iter().map(|(name, label, bin)| (name.clone(), dropdown::Row { text: label.clone(), detail: bin.clone(), now: *name == cur, marks: Vec::new() })).collect();
        if !v.iter().any(|(n, _)| *n == cur) {
            v.push((cur.clone(), dropdown::Row { text: cur, detail: "not installed".into(), now: true, marks: Vec::new() }));
        }
        v
    }

    /// Rows of the model dropdown of harness H (value, row), and a status: models being read, a
    /// read error. Claude Code: aliases + exact ids; others: `default` + the cached list.
    fn model_items(&self, form: &AgentForm, h: &str) -> (Vec<(String, dropdown::Row)>, Option<String>) {
        let cur = form.model_id();
        let row = |m: &crate::models::Model| (m.id.clone(), dropdown::Row { text: m.id.clone(), detail: m.name.clone(), now: m.id == cur, marks: Vec::new() });
        let mut v: Vec<(String, dropdown::Row)> = Vec::new();
        let mut status = None;
        if h == "claude" {
            v.extend(crate::models::claude(&crate::config::models()).iter().map(row));
        } else {
            v.push(row(&crate::models::Model { id: "default".into(), name: "harness default".into() }));
            if let Some(e) = self.models.as_ref().and_then(|m| m.get(h)) {
                v.extend(e.models.iter().map(row));
            }
            if self.model_reads.waiting(&h.to_string()) {
                status = Some("reading models…".to_string());
            } else if let Some(e) = self.model_errors.get(h) {
                status = Some(format!("models: {e}"));
            }
        }
        if !v.iter().any(|(id, _)| *id == cur) {
            v.push((cur.clone(), dropdown::Row { text: cur, detail: "current".into(), now: true, marks: Vec::new() }));
        }
        (v, status)
    }

    /// The rows of FORM's open dropdown for its filter, best match first.
    fn pick_items(&self, form: &AgentForm) -> Vec<(String, dropdown::Row)> {
        let Some(p) = &form.pick else { return Vec::new() };
        let all = if p.model { self.model_items(form, &p.harness).0 } else { Self::harness_items(form) };
        // the text (marked), the value and detail too (substring)
        let ranked = crate::fuzzy::rank(&all, &p.query, |(_, r)| r.text.clone(), |(v, r)| format!("{v} {}", r.detail));
        let mut slots: Vec<Option<(String, dropdown::Row)>> = all.into_iter().map(Some).collect();
        ranked
            .into_iter()
            .filter_map(|(i, marks)| {
                let (v, mut r) = slots[i].take()?;
                r.marks = marks;
                Some((v, r))
            })
            .collect()
    }

    /// The model list of harness H: from the cache file; 0 list or older than a day (or FORCE,
    /// `r`): read in the background.
    fn ensure_models(&mut self, h: &str, force: bool) {
        if h == "claude" {
            return;
        }
        let fresh = self.models.get_or_insert_with(crate::models::load).get(h).is_some_and(crate::models::fresh);
        if fresh && !force {
            return;
        }
        let (probe, name) = (self.probe, h.to_string());
        self.model_errors.remove(h);
        self.model_reads.request(name.clone(), move || probe(&name).map(|m| crate::models::store(&name, &m)));
    }

    /// Enter / click on the harness (MODEL: model) field: its dropdown, the cursor on the current value.
    fn open_form_pick(&mut self, form: &mut AgentForm, model: bool) {
        let h = form.harness_name();
        if model {
            self.ensure_models(&h, false);
            form.field = F_MODEL;
        }
        form.pick = Some(FormPick { model, harness: h, cursor: 0, offset: 0, query: String::new() });
        let cursor = self.pick_items(form).iter().position(|(_, r)| r.now).unwrap_or(0);
        if let Some(p) = &mut form.pick {
            p.cursor = cursor;
        }
    }

    /// Value V picked: a harness → the field, then its model list; a model → the form.
    fn choose_form_pick(&mut self, form: &mut AgentForm, v: &str) {
        let model = form.pick.as_ref().is_some_and(|p| p.model);
        form.pick = None;
        form.error = None;
        if model {
            form.set_model(v);
        } else {
            form.set_harness(v);
            self.open_form_pick(form, true);
        }
    }

    /// Keys of the open harness / model dropdown: ↑↓ move, type: filter, enter: pick (models: 0
    /// match → the typed id), r (ctrl+r while typing): read the models again, esc: close.
    fn form_pick_key(&mut self, form: &mut AgentForm, k: KeyEvent) {
        let items = self.pick_items(form);
        let n = items.len();
        let typed = !k.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER);
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        let Some(p) = form.pick.as_mut() else { return };
        let (model, h, q) = (p.model, p.harness.clone(), p.query.clone());
        match k.code {
            KeyCode::Esc => form.pick = None,
            KeyCode::Up if n > 0 => p.cursor = (p.cursor + n - 1) % n,
            KeyCode::Down if n > 0 => p.cursor = (p.cursor + 1) % n,
            KeyCode::PageUp => p.cursor = p.cursor.saturating_sub(dropdown::MAX_ROWS),
            KeyCode::PageDown if n > 0 => p.cursor = (p.cursor + dropdown::MAX_ROWS).min(n - 1),
            KeyCode::Enter => match items.get(p.cursor) {
                Some((v, _)) => self.choose_form_pick(form, v),
                None if model && !q.trim().is_empty() => self.choose_form_pick(form, q.trim()),
                None => {}
            },
            KeyCode::Char('r') if model && (ctrl || q.is_empty()) => self.ensure_models(&h, true),
            KeyCode::Backspace | KeyCode::Char('u' | 'w' | 'h') if crate::textarea::delete_key(&mut p.query, &k) => (p.cursor, p.offset) = (0, 0),
            KeyCode::Char(c) if typed => {
                p.query.push(c);
                (p.cursor, p.offset) = (0, 0);
            }
            _ => {}
        }
    }

    /// A click while the dropdown is open: a row picks it, elsewhere closes it.
    fn form_pick_click(&mut self, form: &mut AgentForm, at: Position) {
        match self.regions.form_rows.iter().find(|(r, _)| r.contains(at)).map(|&(_, j)| j) {
            Some(j) => {
                if let Some((v, _)) = self.pick_items(form).get(j) {
                    let v = v.clone();
                    self.choose_form_pick(form, &v);
                }
            }
            None => form.pick = None,
        }
    }

    /// The open harness / model dropdown, below its field.
    fn draw_form_pick(&mut self, f: &mut Frame, bounds: Rect) {
        self.regions.form_rows.clear();
        let Some(Modal::Agent(mut form)) = self.modal.take() else { return };
        if let Some((model, h, q, cur, off)) = form.pick.as_ref().map(|p| (p.model, p.harness.clone(), p.query.clone(), p.cursor, p.offset)) {
            let items = self.pick_items(&form);
            let n = items.len();
            let total = if model { self.model_items(&form, &h).0.len() } else { Self::harness_items(&form).len() };
            let status = if model { self.model_items(&form, &h).1 } else { None };
            let row = form.areas[if model { F_MODEL } else { F_HARNESS }];
            let anchor = Rect { x: row.x + 10, y: row.y, width: 1, height: 1 };
            let rows: Vec<dropdown::Row> = items.into_iter().map(|(_, r)| r).collect();
            let w = rows.iter().map(|r| r.text.width() + r.detail.width() + 8).max().unwrap_or(0).clamp(44, 80) as u16;
            if let Some(r) = dropdown::place(anchor, w, n, bounds, false) {
                let cur = cur.min(n.saturating_sub(1));
                let label = match (model, q.is_empty()) {
                    (true, true) => format!("models · {h}"),
                    (true, false) => format!("models · {h} › {q}"),
                    (false, true) => "harnesses · installed".to_string(),
                    (false, false) => format!("harnesses › {q}"),
                };
                let note = match (&status, q.is_empty()) {
                    (Some(s), _) => s.clone(),
                    (None, false) => format!("{n} of {total}"),
                    (None, true) if model && h != "claude" => "type: filter · r: read again".into(),
                    (None, true) => "type: filter".into(),
                };
                let empty = if model && !q.trim().is_empty() { "0 matches · enter: use as typed" } else { "0 matches" };
                let (off, drawn) = dropdown::list(f, r, &label, &note, &rows, cur, off, empty);
                if let Some(p) = form.pick.as_mut() {
                    (p.cursor, p.offset) = (cur, off);
                }
                self.regions.form_rows = drawn;
            }
        }
        self.modal = Some(Modal::Agent(form));
    }

    pub fn set_prompt(&mut self, text: &str) {
        self.prompt.set_text(text);
        self.backup();
    }

    pub fn prompt_text(&self) -> String {
        self.prompt.text()
    }

    // ---------- commands ----------
    fn commands(&self) -> (Vec<Cmd>, Vec<(&'static str, &'static str)>) {
        let c = |key, label, act| Cmd { key, label, act };
        match (self.screen, self.tab) {
            (Screen::Compose, _) => (
                vec![
                    c("ctrl+enter", "start the thread (also cmd+enter, alt+enter)", Act::Start),
                    // no hotkey: ⌘→ arrives as ctrl+e in macOS terminals (it opened the editor)
                    c("menu", "edit the prompt in $EDITOR", Act::EditPrompt),
                    c("ctrl+o", "pick notes (also enter on notes)", Act::OpenNotes),
                    c("ctrl+s", "save the task as a draft for later, start over", Act::SaveDraft),
                    c("menu", "reset: where from the where row, 0 notes, 0 Claude Code context (asks first)", Act::Reset),
                    c("esc esc", "quit", Act::Quit),
                ],
                vec![
                    ("↑↓", "field (↑ on the prompt's 1st row leaves it)"),
                    ("tab / shift+tab", "next / previous field"),
                    ("F1-F6 · alt+1-6", "main tabs"),
                    ("enter", "title: next field · notes: pick · prompt: new line"),
                    ("x on notes", "drop notes + Claude Code context"),
                    ("enter", "where: dropdown · current branch · new branch · new worktree · recent branches · type: filter"),
                    ("↑↓ space", "below where: checkout · git stash boxes, 1 per row"),
                    ("@", "prompt: agent names (↑↓ tab / enter, esc)"),
                    ("ctrl+v", "prompt: clipboard image → [Image #n] (dropped path too)"),
                    ("←→ space", "agents: move · pick / unpick"),
                    ("shift+←→↑↓", "select text"),
                    ("mouse", "click: cursor · drag: select + copy"),
                    ("dbl / triple", "select word / line"),
                    ("bksp", "remove char or selection"),
                    ("ctrl+shift+a c x v", "all · copy · cut · paste"),
                    ("ctrl+a e · ⌘←→", "row start · row end"),
                    ("ctrl+u k · ⌘⌫", "delete to row start · row end"),
                    ("ctrl+z · ⌘z", "undo"),
                    ("ctrl+shift+z · ⌘⇧z · ctrl+y", "redo"),
                    ("ctrl+←→", "word left / right"),
                ],
            ),
            (Screen::Pick, Tab::Notes) => (
                vec![
                    c("ctrl+f", "filter by name + content", Act::Filter),
                    c("d", "delete the note / folder under the cursor", Act::DeleteNote),
                    c("e", "edit the note in $EDITOR", Act::EditNote),
                    c("y", "copy the picked note paths", Act::CopyPaths),
                    c("x", "clear the selection", Act::ClearSelection),
                    c("c", "collapse all folders", Act::CollapseAll),
                    c("o", "expand all folders", Act::ExpandAll),
                    c("r", "reload from disk", Act::Reload),
                    c("menu", "quit codehead", Act::Quit),
                ],
                vec![
                    ("↑↓", "move"),
                    ("←→", "fold / unfold folder"),
                    ("space", "select note / folder"),
                    ("enter", "confirm selection → new (0 notes allowed)"),
                    ("pgup pgdn", "scroll preview"),
                    ("esc", "clear filter · back to new"),
                    ("esc esc", "back to new"),
                ],
            ),
            (Screen::Pick, Tab::Threads) => (
                vec![
                    c("ctrl+f", "filter thread names (fuzzy)", Act::ThreadFilter),
                    c("a", "archive: worktree goes, branch stays (the selection too)", Act::Thread(crate::threadlist::Act::Archive)),
                    c("u", "unarchive: the host makes the worktree again", Act::Thread(crate::threadlist::Act::Unarchive)),
                    c("e", "edit: title, repo, linked repos, agents", Act::Thread(crate::threadlist::Act::Edit)),
                    c("d", "delete thread + worktree (+ branch, if pushed)", Act::DeleteThreads),
                    c("r", "reload", Act::Reload),
                    c("menu", "quit codehead", Act::Quit),
                ],
                vec![
                    ("↑↓", "move · ↑ on the top card: view tabs, repo tabs, tabs"),
                    ("←→", "previous / next repo (on the view tabs: the view)"),
                    ("space", "select thread"),
                    ("enter", "open thread (worktree: new task there; claude: new thread, last response as context)"),
                    ("ctrl+o", "actions of the thread / the selection"),
                    ("c", "thread / claude session: last response in / out of the task context"),
                    ("F1-F6 · alt+1-6", "switch tab"),
                    ("pgup pgdn", "scroll preview"),
                    ("esc", "clear filter · clear selection"),
                ],
            ),
            (Screen::Pick, Tab::Agents | Tab::New) => (
                vec![
                    c("ctrl+f", "filter agents (fuzzy: name, harness, model)", Act::AgentFilter),
                    c("n", "new agent", Act::NewAgent),
                    c("e", "edit agent (also enter)", Act::EditAgent),
                    c("d", "delete agent: its threads move to the default agent", Act::DeleteAgent),
                    c("r", "reload", Act::Reload),
                    c("menu", "quit codehead", Act::Quit),
                ],
                vec![
                    ("↑↓←→", "move in the card grid · ↑ on top row: tabs"),
                    ("enter", "edit agent"),
                    ("ctrl+f", "filter: enter / esc back to the cards"),
                    ("F1-F6 · alt+1-6", "switch tab"),
                    ("pgup pgdn", "scroll preview"),
                    ("esc", "clear the filter"),
                ],
            ),
            (Screen::Pick, Tab::Merge) => (
                vec![c("r", "read the pull requests again", Act::Reload), c("menu", "quit codehead", Act::Quit)],
                vec![
                    ("↑↓ / j k", "move: PRs; in a PR: the things to act on"),
                    ("enter", "open the PR · act (edit, dropdown, merge, update)"),
                    ("ctrl+enter / ctrl+click", "open on GitHub"),
                    ("←→", "state tabs: open, merged, closed, all"),
                    ("[ ] · < >", "previous / next PR · overview / changes"),
                    ("↑ on top · ←→", "PR / views row: switch PR / overview, changes (the diff too)"),
                    ("i · tab", "the prompt; tab / esc: back"),
                    ("i", "prompt: a new thread with this task"),
                    ("esc", "back to the list"),
                ],
            ),
            (Screen::Pick, Tab::Flows) => (
                vec![c("r", "read the flow and step files again", Act::Reload), c("menu", "quit codehead", Act::Quit)],
                vec![
                    ("←→", "flows / steps"),
                    ("enter", "flow: its rail · step: edit"),
                    ("n D R", "new · duplicate · rename"),
                    ("d d · ctrl+d ctrl+d · r", "delete · reset an edited built-in · read again"),
                    ("m", "flow: user ↔ repo (.codehead/flows)"),
                    ("rail: a enter J K", "add · edit · move a step"),
                    ("rail: g o L + -", "gate · gate outcome · loop · loop max"),
                    ("rail: s", "save a one-off as a shared step"),
                    ("editor: ctrl+g ctrl+f", "gate · fork a shared step into a one-off"),
                ],
            ),
            (Screen::Pick, Tab::Settings) => (
                vec![c("menu", "add path alias (also enter on + add)", Act::AddAlias), c("ctrl+r", "reload file", Act::Reload), c("menu", "quit codehead", Act::Quit)],
                vec![("↑↓", "field / alias box"), ("enter", "alias box: edit / done"), ("d", "delete the highlighted alias"), ("F1-F6 · alt+1-6", "switch tab"), ("esc", "close path suggestions"), ("auto", "changes apply on leaving a field")],
            ),
        }
    }

    fn open_menu(&mut self) {
        let (cmds, help) = self.commands();
        let mut items: Vec<crate::menu::Item> =
            cmds.iter().enumerate().map(|(i, c)| crate::menu::Item { label: c.label.to_string(), key: c.key.to_string(), id: i, section: "this screen" }).collect();
        items.push(crate::menu::Item { label: "keys of this screen".into(), key: String::new(), id: KEYS_ITEM, section: "help" });
        self.menu = Some(crate::menu::Menu::new(items, None, self.agents.clone(), Vec::new()));
        self.menu_cmds = cmds;
        self.menu_help = help;
    }

    /// A start screen tab, from the menu (here or in the thread view).
    pub fn go_home(&mut self, h: crate::menu::Home) {
        use crate::menu::Home;
        self.modal = None;
        self.drafts_menu = None;
        if h == Home::New {
            self.new_task();
        }
        self.switch_tab(match h {
            Home::New => Tab::New,
            Home::Settings => Tab::Settings,
            Home::Threads => Tab::Threads,
            Home::Merge(_) => Tab::Merge,
            Home::Flows(_) => Tab::Flows,
        });
        if h != Home::Settings {
            self.tabs_focus = false;
        }
        if let Home::Merge(Some(pr)) = &h {
            self.merge_tab.open(pr);
        }
        if let Home::Flows(Some(slug)) = &h {
            self.flows_tab.open(slug);
        }
    }

    /// What the flows tab asked for.
    fn flows_out(&mut self, o: crate::flows::Out) {
        match o {
            crate::flows::Out::None => {}
            crate::flows::Out::Toast(t) => self.toast(t, false),
            crate::flows::Out::Up => self.tabs_focus = true,
        }
    }

    /// What the merge tab asked for.
    fn merge_out(&mut self, o: crate::mergetab::Out) {
        use crate::mergetab::Out;
        match o {
            Out::None | Out::Back => {}
            Out::Toast(t) => self.toast(t, false),
            // 0 thread: the new-thread view, the task written; the owner sends it
            Out::Compose(task) => {
                self.prompt.set_text(&task);
                self.prompt.move_end();
                self.switch_tab(Tab::New);
                self.tabs_focus = false;
                self.field = Field::Prompt;
                self.toast("the task is written: pick agents, send", false);
            }
        }
    }

    fn menu_action(&mut self, a: crate::menu::Action) {
        use crate::menu::Action;
        match a {
            Action::None => return,
            // home: the start screen is home
            Action::Close | Action::Back => {}
            Action::Home(h) => self.go_home(h),
            Action::Open(c) => self.outcome = Some(Outcome::Resume(Box::new(worktree_of(&c)))),
            Action::Pr(e) => match e.thread {
                Some(t) => {
                    crate::view::open_pr(&t.id, &e.item.pr);
                    let c = crate::nav::Card { id: t.id.clone(), title: t.title.clone(), dir: Some(t.dir.clone()), ..Default::default() };
                    self.outcome = Some(Outcome::Resume(Box::new(worktree_of(&c))));
                }
                None => self.go_home(crate::menu::Home::Merge(Some(e.item.pr.clone()))),
            },
            Action::Jump(c, ts) => {
                crate::search::jump_to(&c.id, ts);
                self.outcome = Some(Outcome::Resume(Box::new(worktree_of(&c))));
            }
            Action::Command(KEYS_ITEM) => {
                let kw = self.menu_help.iter().map(|(k, _)| k.width()).max().unwrap_or(0);
                let lines = self.menu_help.iter().map(|(k, l)| Line::from(vec![Span::styled(format!("{k:<kw$}  "), theme::accent()), Span::styled(*l, theme::muted())])).collect();
                self.modal = Some(Modal::Info { title: "keys".into(), lines, scroll: 0 });
            }
            Action::Command(i) => {
                if let Some(act) = self.menu_cmds.get(i).map(|c| c.act) {
                    self.menu = None;
                    return self.run(act);
                }
            }
        }
        self.menu = None;
    }

    fn run(&mut self, act: Act) {
        match act {
            Act::AddAlias => {
                if let Some(s) = self.settings.as_mut() {
                    s.form.add_alias();
                }
            }
            Act::AgentFilter => {
                self.switch_tab(Tab::Agents);
                self.tabs_focus = false;
                if self.afilter.is_none() {
                    self.afilter = Some(String::new());
                }
                if self.afilter_ta.text() != self.afilter.as_deref().unwrap_or("") {
                    self.afilter_ta.set_text(self.afilter.as_deref().unwrap_or(""));
                }
                self.afilter_ta.move_end();
                self.afilter_editing = true;
            }
            Act::Thread(a) => {
                self.switch_tab(Tab::Threads);
                self.tabs_focus = false;
                self.tlist.focus = crate::threadlist::Focus::List;
                if let crate::threadlist::Out::Open(c) = self.tlist.run(a) {
                    self.open_card(&c);
                }
            }
            Act::ThreadFilter => {
                self.switch_tab(Tab::Threads);
                self.tabs_focus = false;
                self.tlist.focus = crate::threadlist::Focus::List;
                self.tlist.filter.move_end();
                self.tlist.filtering = true;
            }
            Act::Filter => {
                self.open_notes();
                if self.filter.is_none() {
                    self.filter = Some(String::new());
                    self.cursor = 0;
                    self.rebuild();
                }
                if self.filter_ta.text() != self.filter.as_deref().unwrap_or("") {
                    self.filter_ta.set_text(self.filter.as_deref().unwrap_or(""));
                }
                self.filter_ta.move_end();
                self.filter_editing = true;
            }
            Act::DeleteNote => {
                let Some(row) = self.current() else { return };
                let path = row.path().to_string();
                let question = if row.is_dir() {
                    format!("delete folder {path} with {} notes?", self.notes.inside(&path).len())
                } else {
                    format!("delete {path}?")
                };
                self.modal = Some(Modal::Confirm { title: "delete note".into(), question, yes: false, action: ConfirmAction::Note(path) });
            }
            Act::EditNote => match self.current() {
                Some(r) if !r.is_dir() => self.suspend = Some(Suspend::Note(self.notes.abs(r.path()))),
                _ => self.toast("move to a note first", true),
            },
            Act::CopyPaths => {
                let paths: Vec<String> = if self.selected.is_empty() {
                    self.current().map(|r| self.notes.inside(r.path())).unwrap_or_default()
                } else {
                    self.selected.clone()
                };
                let text: String = paths.iter().map(|p| format!("{}\n", self.notes.abs(p).display())).collect();
                if paths.is_empty() {
                    self.toast("0 notes to copy", true);
                } else if crate::clip::copy(&text) {
                    self.toast(format!("copied {} path(s)", paths.len()), false);
                } else {
                    self.toast("0 clipboard tools found (wl-copy, xclip, xsel)", true);
                }
            }
            Act::ClearSelection => self.selected.clear(),
            Act::CollapseAll => {
                self.collapsed = self.notes.tree(&HashSet::new()).iter().filter(|r| r.is_dir()).map(|r| r.path().to_string()).collect();
                self.rebuild();
            }
            Act::ExpandAll => {
                self.collapsed.clear();
                self.rebuild();
            }
            Act::Reload => {
                match self.tab {
                    Tab::Threads => self.load_threads(),
                    Tab::Merge => {
                        let o = self.merge_tab.key(KeyEvent::from(KeyCode::Char('r')));
                        return self.merge_out(o);
                    }
                    Tab::Agents => self.load_agents(),
                    Tab::Notes | Tab::New => self.reload_notes(),
                    Tab::Settings => { self.settings_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL)); return; }
                    Tab::Flows => self.flows_tab.enter(),
                }
                self.toast("reloaded", false);
            }
            // threads: the thread list asks and deletes; a worktree without thread: here
            Act::DeleteThreads if self.tlist.view.cards() => {
                self.switch_tab(Tab::Threads);
                self.tabs_focus = false;
                self.tlist.run(crate::threadlist::Act::Delete);
            }
            Act::DeleteThreads => {
                let items: Vec<Worktree> = self.current_wt().filter(|w| w.thread.is_none()).into_iter().collect();
                if items.is_empty() {
                    return;
                }
                let question = format!("delete {} thread(s) + worktree(s)? uncommitted files → git stash", items.len());
                self.modal = Some(Modal::Confirm { title: "delete threads".into(), question, yes: false, action: ConfirmAction::Threads(items) });
            }
            Act::NewAgent => {
                let d = self.default_style();
                self.modal = Some(Modal::Agent(Box::new(AgentForm::new(None, &self.styles, d.as_deref()))));
            }
            Act::EditAgent => {
                if let Some(a) = self.current_agent().cloned() {
                    let d = self.default_style();
                    self.modal = Some(Modal::Agent(Box::new(AgentForm::new(Some(a), &self.styles, d.as_deref()))));
                }
            }
            Act::DeleteAgent => {
                let Some(a) = self.current_agent().cloned() else { return };
                if a.name == self.cfg.agent {
                    self.toast(format!("{} is the default agent: threads of deleted agents go there", a.name), true);
                    return;
                }
                if !self.agents.iter().any(|x| x.name == self.cfg.agent) {
                    self.toast(format!("default agent {} not found: 0 agents to take over", self.cfg.agent), true);
                    return;
                }
                let uses = self.uses(&a);
                let live = uses.iter().filter(|u| !u.live.is_empty()).count();
                let question = format!("delete {}? {} threads move to {} ({live} live: {} takes over now)", a.name, uses.len(), self.cfg.agent, self.cfg.agent);
                self.modal = Some(Modal::Confirm { title: "delete agent".into(), question, yes: false, action: ConfirmAction::Agent(a) });
            }
            Act::Start => self.start(),
            Act::EditPrompt => self.suspend = Some(Suspend::Prompt),
            Act::OpenNotes => self.open_notes(),
            Act::SaveDraft => self.save_draft(),
            // asked first: the reset drops picked notes and context
            Act::Reset => self.modal = Some(Modal::Confirm { title: "reset".into(), question: "reset the where row, drop the picked notes and context?".into(), yes: false, action: ConfirmAction::ResetTask }),
            Act::Quit => self.outcome = Some(Outcome::Quit),
        }
    }

    /// No to a confirm: a discard of the agent form goes back to the form.
    fn declined(&mut self, action: ConfirmAction) {
        if let ConfirmAction::DiscardAgent(form) = action {
            self.modal = Some(Modal::Agent(form));
        }
    }

    fn confirmed(&mut self, action: ConfirmAction) {
        match action {
            ConfirmAction::DiscardAgent(_) => {}
            ConfirmAction::ResetTask => {
                self.target = None;
                self.carry.clear();
                self.selected.clear();
                self.toast("reset: where row, 0 notes, 0 context", false);
            }
            ConfirmAction::Note(path) => match trash(&self.notes.abs(&path)) {
                Ok(how) => {
                    self.selected.retain(|s| !(s == &path || (path.ends_with('/') && s.starts_with(&path))));
                    self.reload_notes();
                    self.toast(format!("{path}: {how}"), false);
                }
                Err(e) => self.toast(format!("{path}: {e}"), true),
            },
            // teardown scripts, git, host stops: seconds; the report comes as a modal
            ConfirmAction::Threads(items) => {
                let repo = self.cfg.repo.clone().unwrap_or_else(|| self.cfg.cwd.clone());
                let logs = self.cfg.logs.clone();
                self.toast(format!("deleting {} threads…", items.len()), false);
                self.job(move || Job::Threads(threads::remove(&repo, &items, &logs)));
            }
            ConfirmAction::Agent(a) => {
                let Some(fallback) = self.agents.iter().find(|x| x.name == self.cfg.agent).cloned() else { return };
                let uses = self.uses(&a);
                let logs = self.cfg.logs.clone();
                self.toast(format!("deleting {}…", a.name), false);
                self.job(move || Job::AgentDeleted(agents::delete(&a, &fallback, &uses, &logs)));
            }
        }
    }

    fn save_agent(&mut self, mut form: Box<AgentForm>) {
        let spec = form.spec(&self.styles, self.global_style.as_deref());
        if spec.name.is_empty() {
            form.error = Some("name required".into());
            form.field = 0;
            self.modal = Some(Modal::Agent(form));
            return;
        }
        let res = match &form.edit {
            Some(old) => agents::edit(old, &spec),
            None => agents::create(&spec),
        }
        .and_then(|_| agents::set_effort(form.effort_level()));
        match res {
            Ok(()) => {
                let old_name = form.edit.as_ref().map(|a| a.name.clone());
                if let Some(old) = &old_name
                    && let Some(p) = self.picks.iter_mut().find(|p| *p == old)
                {
                    *p = spec.name.clone();
                }
                self.load_agents();
                if let Some(i) = self.agents.iter().position(|a| a.name == spec.name) {
                    self.acursor = i;
                }
                let msg = format!("{} {}", spec.name, if old_name.is_some() { "saved" } else { "created" });
                self.toast(msg.clone(), false);
                // live sessions keep the profile they started with: restart them on the new one
                // (a socket per thread: off the UI thread; the toast with the restarts comes after)
                // `load_agents` reads in the background: `self.agents` still holds the old list
                if let Some(old) = form.edit.clone()
                    && let Some(new) = agents::roster_only().into_iter().find(|a| a.name == spec.name)
                    && agents::profile_changed(&old, &new)
                {
                    let logs = self.cfg.logs.clone();
                    let head = msg;
                    self.job(move || {
                        let uses = agents::usage().remove(&new.name).unwrap_or_default();
                        let r = agents::restart(&new, old.harness == new.harness, &uses, &logs);
                        Job::Restarted(format!("{head}{}", restart_note(&r)))
                    });
                }
            }
            Err(e) => {
                form.error = Some(e.lines().last().unwrap_or("failed").to_string());
                self.modal = Some(Modal::Agent(form));
            }
        }
    }

    // ---------- keys ----------
    pub fn key(&mut self, k: KeyEvent) {
        self.key_inner(k);
        self.backup();
    }

    fn key_inner(&mut self, k: KeyEvent) {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        // notifications (ctrl+n): over everything, the menu too
        if let Some(p) = self.notes_panel.as_mut() {
            let pick = p.key(k);
            return self.notes_pick(pick);
        }
        if crate::notify::is_key(&k) {
            self.menu = None;
            self.notes_panel = Some(crate::notify::Panel::open(None));
            return;
        }
        // F2 menu: over everything, its keys first (1 esc closes it)
        if let Some(m) = self.menu.as_mut() {
            self.last_esc = None;
            let a = m.key(k);
            return self.menu_action(a);
        }
        if crate::config::menu_key().matches(&k) {
            self.last_esc = None;
            return self.open_menu();
        }
        // drafts dropdown: its keys first (esc: back to the task, 0 double esc)
        if self.drafts_menu.is_some() && self.modal.is_none() {
            self.last_esc = None;
            return self.drafts_key(k);
        }
        // where dropdown: its keys first (esc closes it, 0 double esc)
        if self.where_menu.is_some() && self.modal.is_none() {
            self.last_esc = None;
            return self.where_key(k);
        }
        // flow dropdown (+ its steps): its keys first (esc closes 1 level, 0 double esc)
        if self.flow_menu.is_some() && self.modal.is_none() {
            self.last_esc = None;
            return self.flow_menu_key(k);
        }
        // repo picker: its keys first (esc: filter off, then closed; 0 double esc)
        if self.repo_pick.is_some() && self.modal.is_none() {
            self.last_esc = None;
            return self.repo_pick_key(k);
        }
        // the thread list's form / question / dropdown / filter: its keys first (esc closes it, 0
        // double esc)
        if self.screen == Screen::Pick && self.tab == Tab::Threads && self.modal.is_none() && self.tlist.modal() {
            // its esc: a 2nd esc soon after closes the other controls, 0 quit
            self.last_esc = (k.code == KeyCode::Esc).then(|| (Instant::now(), false));
            return self.threads_key(k);
        }
        // @ and > dropdowns of the prompt: their keys first (esc closes them, 0 double esc)
        if self.mention_on() {
            // skills: of the picked agents' harnesses (0 picked: of every agent's)
            let picked = |a: &&Agent| self.picks.is_empty() || self.picks.contains(&a.name);
            let mut harnesses: Vec<String> = self.agents.iter().filter(picked).map(|a| a.harness.clone()).collect();
            harnesses.sort();
            harnesses.dedup();
            self.command.harnesses = harnesses;
            if self.mention.key(k, &mut self.prompt, &self.agents) || self.command.key(k, &mut self.prompt) || self.shortcuts.key(k, &mut self.prompt) || self.paths.key(k, &mut self.prompt) {
                self.last_esc = None;
                return;
            }
        }
        // a modal (confirm, agent form) takes its own Esc: 0 double esc over it
        if k.code == KeyCode::Esc && k.modifiers.is_empty() && self.modal.is_none() {
            if let Some((t, from_start)) = self.last_esc.take()
                && t.elapsed() < DOUBLE_ESC
            {
                if from_start {
                    self.outcome = Some(Outcome::Quit);
                } else {
                    self.dismiss_controls();
                }
                return;
            }
            self.last_esc = Some((Instant::now(), self.at_start()));
        } else {
            self.last_esc = None;
        }
        // settings: Esc closes path suggestions. Typed ones: 0 double esc. Ones a path field
        // shows on focus by itself: the esc still counts for esc esc.
        if k.code == KeyCode::Esc && self.modal.is_none() && !self.tabs_focus
            && self.screen == Screen::Pick && self.tab == Tab::Settings
            && self.settings.as_mut().is_some_and(|s| s.suggestions_key(k))
        {
            if self.settings_typed { self.last_esc = None; }
            return;
        }
        if self.screen == Screen::Pick && self.tab == Tab::Settings && k.code != KeyCode::Esc { self.settings_typed = true; }
        if self.modal.is_some() {
            self.modal_key(k);
            return;
        }
        if ctrl && matches!(k.code, KeyCode::Char('c')) {
            return self.ctrl_c();
        }
        // F1-F4 (alt+1..4 too): the main tabs from every field (ctrl / alt+←→: word jumps); tab: the next field
        let main_tab = match k.code {
            KeyCode::F(n @ 1..=6) => Some(usize::from(n - 1)),
            KeyCode::Char(c @ '1'..='6') if k.modifiers.contains(KeyModifiers::ALT) && !ctrl => Some(usize::from(c as u8 - b'1')),
            _ => None,
        };
        if !self.cfg.copy_mode
            && let Some(i) = main_tab
        {
            let to = MAIN_TABS[i];
            self.switch_tab(to);
            // the keys: in the tab's content; settings: on its tabs row (0 path dropdown at entry)
            if to != Tab::Settings {
                self.tabs_focus = false;
            }
            return;
        }
        match self.screen {
            Screen::Compose if self.tabs_focus => self.tabs_key(k),
            Screen::Compose => self.compose_key(k),
            Screen::Pick => {
                if self.tab == Tab::Settings && !self.tabs_focus {
                    self.settings_key(k);
                    return;
                }
                if self.tab == Tab::Flows && !self.tabs_focus {
                    let o = self.flows_tab.key(k);
                    self.flows_out(o);
                    return;
                }
                if self.tab != Tab::Merge && self.preview_key(k) {
                    return;
                }
                if self.tabs_focus {
                    self.tabs_key(k);
                } else {
                    self.pick_content_key(k);
                }
            }
        }
    }

    /// The start screen: 1 of its main tabs (task, threads, agents, settings), 0 local controls
    /// for an Esc to close (modal, filter input, thread filter, the notes list of the task).
    fn at_start(&self) -> bool {
        let notes = self.screen == Screen::Pick && self.tab == Tab::Notes;
        let pr_open = self.screen == Screen::Pick && ((self.tab == Tab::Merge && (self.merge_tab.is_detail() || self.merge_tab.typing())) || (self.tab == Tab::Flows && self.flows_tab.typing()));
        let threads = self.screen == Screen::Pick && self.tab == Tab::Threads;
        self.modal.is_none() && self.drafts_menu.is_none() && self.where_menu.is_none() && self.flow_menu.is_none() && self.repo_pick.is_none() && !notes && !pr_open
            && !self.filter_editing && !self.afilter_editing
            && !(threads && (self.tlist.modal() || !self.tlist.query().is_empty() || !self.tlist.sel.is_empty()))
            && !(self.screen == Screen::Pick && self.tab == Tab::Agents && self.afilter.as_deref().is_some_and(|q| !q.is_empty()))
    }

    /// Close local controls without changing the current tab.
    fn dismiss_controls(&mut self) {
        self.modal = None;
        self.drafts_menu = None;
        self.where_menu = None;
        self.flow_menu = None;
        self.repo_pick = None;
        self.filter_editing = false;
        self.tlist.filtering = false;
        self.afilter_editing = false;
        self.tabs_focus = false;
    }

    /// Notes filter input → FILTER (list rebuilt, cursor to the top).
    fn sync_filter(&mut self) {
        let q = self.filter_ta.text();
        if self.filter.as_deref() != Some(q.as_str()) {
            self.filter = Some(q);
            self.cursor = 0;
            self.rebuild();
        }
    }

    /// Threads filter input → TFILTER (cursor to the top).
    fn sync_afilter(&mut self) {
        let q = self.afilter_ta.text();
        if self.afilter.as_deref() != Some(q.as_str()) {
            self.afilter = Some(q);
            if let Some(&first) = self.avisible().first().and_then(|(_, g)| g.first()) {
                self.acursor = first;
            }
            self.aoffset = 0;
        }
    }

    /// Agents by harness (groups and agents A→Z). Only the agents tab filters.
    fn avisible(&self) -> Vec<(String, Vec<usize>)> {
        let q = if self.screen == Screen::Compose { "" } else { self.afilter.as_deref().unwrap_or("") }.trim().to_lowercase();
        crate::cards::groups(&self.agents, |a| crate::cards::matches(a, &q))
    }

    /// Agents tab: the card in direction (DX, DY) of the cursor card. ↑ from the top row: None.
    fn agent_step(&self, dx: isize, dy: isize) -> Option<usize> {
        Self::grid_step(&self.aplace, self.acursor, dx, dy)
    }

    fn grid_step(places: &[(usize, u16, usize)], cursor: usize, dx: isize, dy: isize) -> Option<usize> {
        let at = places.iter().position(|&(i, _, _)| i == cursor)?;
        let (_, x, y) = places[at];
        if dx != 0 {
            let to = at as isize + dx;
            return (0..places.len() as isize).contains(&to).then(|| places[to as usize].0);
        }
        let rows = places.iter().map(|&(_, _, r)| r).filter(|&r| if dy < 0 { r < y } else { r > y });
        let row = if dy < 0 { rows.max()? } else { rows.min()? };
        places.iter().filter(|&&(_, _, r)| r == row).min_by_key(|&&(_, cx, _)| cx.abs_diff(x)).map(|&(i, _, _)| i)
    }

    /// The prompt has the keys: the @ dropdown can be up.
    fn mention_on(&self) -> bool {
        self.modal.is_none() && self.screen == Screen::Compose && !self.tabs_focus && self.field == Field::Prompt
    }

    /// Agents for the @ dropdown: name + color.
    fn agent_names(&self) -> Vec<(String, ratatui::style::Color)> {
        self.agents.iter().map(|a| (a.name.clone(), agents::color(a.color.as_deref()))).collect()
    }

    /// A paste for the prompt: text in, a file → its marker in, the file listed in the message.
    fn paste_prompt(&mut self, p: Pasted) {
        self.field = Field::Prompt;
        match p {
            Pasted::Text(t) => self.prompt.paste(&t),
            Pasted::File { path, marker } => {
                self.prompt.insert(&marker.to_string());
                self.toast(format!("{marker}: {}", tilde(&path.display().to_string())), false);
                self.files.push((marker, path));
            }
            Pasted::Nothing => self.toast("clipboard: 0 image, 0 text", true),
        }
    }

    /// The tab the header shows as active.
    fn cur_tab(&self) -> Tab {
        if self.screen == Screen::Compose { Tab::New } else { self.tab }
    }

    fn switch_tab(&mut self, tab: Tab) {
        self.where_menu = None;
        self.flow_menu = None;
        self.repo_pick = None;
        // leaving the settings: their last edit applies
        if tab != Tab::Settings && self.screen == Screen::Pick && self.tab == Tab::Settings {
            if let Some(s) = self.settings.as_mut() { s.commit(); }
            self.apply_settings();
        }
        if tab == Tab::New {
            self.screen = Screen::Compose;
            self.load_task_flows();
            return;
        }
        self.screen = Screen::Pick;
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
        if tab == Tab::Settings {
            self.settings_typed = false;
            self.tabs_focus = true;
        }
        if tab == Tab::Settings && self.settings.is_none() {
            self.settings = Some(crate::settings::Settings::new(self.settings_path.clone()));
        }
        self.tab = tab;
        self.filter_editing = false;
        self.pscroll = 0;
    }

    fn settings_action(&mut self, action: crate::settings::Action) {
        use crate::settings::Action;
        match action {
            Action::Header => self.tabs_focus = true,
            Action::None => {}
        }
        self.apply_settings();
    }

    /// Settings the form wrote to codehead.toml: live now.
    fn apply_settings(&mut self) {
        let Some(file) = self.settings.as_mut().and_then(|s| s.take_applied()) else { return };
        {
            {
                // tests: 0 change to the process-wide config other tests read
                if !cfg!(test) { crate::config::apply(file); }
                self.cfg.prefix = crate::config::prefix();
                self.cfg.agent = crate::default_agent(&self.agents).unwrap_or_default();
                let notes = crate::config::notes();
                if self.cfg.notes != notes {
                    self.cfg.notes = notes;
                    self.selected.clear();
                    self.reload_notes();
                }
                self.styles = styles::load(&styles::dir());
                self.global_style = styles::global_name();
            }
        }
    }

    fn settings_key(&mut self, key: KeyEvent) {
        let action = self.settings.as_mut().map(|s| s.key(key)).unwrap_or(crate::settings::Action::None);
        self.settings_action(action);
    }

    fn preview_key(&mut self, k: KeyEvent) -> bool {
        if (self.filter_editing || self.tlist.filtering || self.afilter_editing) && !matches!(k.code, KeyCode::PageUp | KeyCode::PageDown) {
            return false;
        }
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        let shift = k.modifiers.contains(KeyModifiers::SHIFT);
        let half = (self.regions.preview.height / 2).max(1);
        match k.code {
            KeyCode::PageDown => self.pscroll = self.pscroll.saturating_add(half * 2),
            KeyCode::PageUp => self.pscroll = self.pscroll.saturating_sub(half * 2),
            KeyCode::Char('d') if ctrl => self.pscroll = self.pscroll.saturating_add(half),
            KeyCode::Char('u') if ctrl => self.pscroll = self.pscroll.saturating_sub(half),
            KeyCode::Down if shift => self.pscroll = self.pscroll.saturating_add(1),
            KeyCode::Up if shift => self.pscroll = self.pscroll.saturating_sub(1),
            _ => return false,
        }
        true
    }

    fn tabs_key(&mut self, k: KeyEvent) {
        match k.code {
            KeyCode::Right | KeyCode::Char('l') => self.switch_tab(self.cur_tab().next()),
            KeyCode::Left | KeyCode::Char('h') => self.switch_tab(self.cur_tab().prev()),
            // tab: the next field, the tab's content (as ↓)
            KeyCode::Down | KeyCode::Enter | KeyCode::Char('j') | KeyCode::Tab => {
                self.tabs_focus = false;
                if self.screen == Screen::Compose {
                    self.field = self.fields()[0];
                } else if self.tab == Tab::Settings
                    && let Some(settings) = &mut self.settings { settings.focus_input(); }
                // the threads tab: its repo tabs first (↓↓: the view tabs, the list)
                if self.screen == Screen::Pick && self.tab == Tab::Threads && k.code == KeyCode::Down {
                    self.tlist.focus = crate::threadlist::Focus::Repos;
                }
            }
            // esc: the top level already (esc esc quits); q: 0 quit (menu → quit codehead)
            KeyCode::Up | KeyCode::BackTab | KeyCode::Esc => {}
            // any other key: the tab's own (its hotkeys, e.g. ctrl+f: filter), focus to the content.
            // Settings: 0 (a letter there would edit a field)
            _ if self.screen == Screen::Compose => {
                self.tabs_focus = false;
                self.field = self.fields()[0];
                self.compose_key(k);
            }
            _ if self.tab != Tab::Settings => {
                self.tabs_focus = false;
                self.pick_content_key(k);
            }
            _ => {}
        }
    }

    /// A key for the content of the current tab of the pick screen (0 tabs row focus).
    fn pick_content_key(&mut self, k: KeyEvent) {
        match self.tab {
            Tab::Notes => self.notes_key(k),
            Tab::Threads => self.threads_key(k),
            Tab::Merge => {
                let o = self.merge_tab.key(k);
                self.merge_out(o);
            }
            Tab::Agents | Tab::New => self.agents_key(k),
            Tab::Settings => self.settings_key(k),
            Tab::Flows => {
                let o = self.flows_tab.key(k);
                self.flows_out(o);
            }
        }
    }

    fn move_cursor(&mut self, delta: isize) {
        let n = self.rows.len();
        if n == 0 {
            return;
        }
        let c = (self.cursor as isize + delta).clamp(0, n as isize - 1) as usize;
        if c != self.cursor {
            self.cursor = c;
            self.pscroll = 0;
        }
    }

    fn fold(&mut self, open: bool) {
        let Some(row) = self.current().cloned() else { return };
        if let Row::Dir { path, collapsed, .. } = &row {
            if open && *collapsed {
                self.collapsed.remove(path);
                self.rebuild();
                return;
            }
            if open {
                self.move_cursor(1);
                return;
            }
            if !*collapsed {
                self.collapsed.insert(path.clone());
                self.rebuild();
                return;
            }
        }
        if open || self.filter.is_some() {
            return;
        }
        // ← on a file / closed folder: fold the parent, cursor onto it
        let p = row.path().trim_end_matches('/');
        if let Some((parent, _)) = p.rsplit_once('/') {
            let parent = format!("{parent}/");
            self.collapsed.insert(parent.clone());
            self.rebuild();
            if let Some(i) = self.rows.iter().position(|r| r.path() == parent) {
                self.cursor = i;
            }
        }
    }

    fn notes_key(&mut self, k: KeyEvent) {
        if self.filter_editing {
            match k.code {
                KeyCode::Esc => {
                    self.filter = None;
                    self.filter_editing = false;
                    self.rebuild();
                }
                KeyCode::Enter => self.filter_editing = false,
                KeyCode::Up => self.move_cursor(-1),
                KeyCode::Down => self.move_cursor(1),
                // backspace on an empty filter: filter closed
                KeyCode::Backspace if self.filter_ta.is_empty() => {
                    self.filter = None;
                    self.filter_editing = false;
                    self.cursor = 0;
                    self.rebuild();
                }
                _ => {
                    if matches!(self.filter_ta.key(k), Input::Handled) {
                        self.sync_filter();
                    }
                }
            }
            return;
        }
        match k.code {
            KeyCode::Up | KeyCode::Char('k') => self.move_cursor(-1),
            KeyCode::Down | KeyCode::Char('j') => self.move_cursor(1),
            KeyCode::Home | KeyCode::Char('g') => self.move_cursor(isize::MIN / 2),
            KeyCode::End | KeyCode::Char('G') => self.move_cursor(isize::MAX / 2),
            KeyCode::Right | KeyCode::Char('l') => self.fold(true),
            KeyCode::Left | KeyCode::Char('h') => self.fold(false),
            KeyCode::Char(' ') => {
                self.toggle_select();
                self.move_cursor(1);
            }
            KeyCode::Enter => self.confirm_notes(),
            KeyCode::Esc if self.filter.is_some() => {
                self.filter = None;
                self.rebuild();
            }
            KeyCode::Esc if self.cfg.copy_mode => self.outcome = Some(Outcome::Quit),
            // back to the task, selection kept
            KeyCode::Esc => {
                self.screen = Screen::Compose;
                self.field = Field::Notes;
            }
            // tab: done with the notes, back to the task (main tabs: alt+1-4)
            KeyCode::Tab | KeyCode::BackTab => {
                self.screen = Screen::Compose;
                self.field = Field::Notes;
            }
            _ => {
                let name = key_name(&k);
                if let Some(c) = self.commands().0.into_iter().find(|c| c.key == name) {
                    self.run(c.act);
                }
            }
        }
    }

    /// Repo of threads tab item I (worktrees ++ sessions).
    fn trepo_of(&self, i: usize) -> Option<&Path> {
        let wts = self.worktrees.as_deref().unwrap_or(&[]);
        match wts.get(i) {
            Some(w) => w.repo.as_deref(),
            None => self.sessions.get(i - wts.len()).and_then(|s| s.repo.as_deref()),
        }
    }

    /// Keys of the threads tab: the thread list's (cards, its tabs, its filter, its actions),
    /// this screen's rows in the views worktrees / claude.
    fn threads_key(&mut self, k: KeyEvent) {
        use crate::threadlist::{Focus, Out};
        let rows = !self.tlist.view.cards() && self.tlist.focus == Focus::List && !self.tlist.modal();
        let before = (self.tlist.cursor, self.tlist.view, self.tlist.query());
        let out = if rows { self.rows_key(k) } else { self.tlist.key(k) };
        if before != (self.tlist.cursor, self.tlist.view, self.tlist.query()) {
            self.pscroll = 0;
            if before.1 != self.tlist.view || before.2 != self.tlist.query() {
                self.tcursor = 0;
            }
        }
        match out {
            Out::None | Out::Home => {}
            Out::Up => self.tabs_focus = true,
            Out::Open(c) => self.open_card(&c),
            Out::Pass => match k.code {
                KeyCode::Char('c') => self.toggle_context(),
                // a list: 0 fields to tab to (main tabs: alt+1-6)
                KeyCode::Tab | KeyCode::BackTab => {}
                _ => {
                    let name = key_name(&k);
                    if let Some(c) = self.commands().0.into_iter().find(|c| c.key == name) {
                        self.run(c.act);
                    }
                }
            },
        }
    }

    /// Keys on the rows of the views worktrees / claude (↑ on the top row: the view tabs; ←→,
    /// the filter: the thread list's).
    fn rows_key(&mut self, k: KeyEvent) -> crate::threadlist::Out {
        use crate::threadlist::Out;
        let n = self.n_threads();
        match k.code {
            KeyCode::Up | KeyCode::Char('k') => {
                if self.tcursor == 0 {
                    self.tlist.focus_views();
                } else {
                    self.tcursor -= 1;
                    self.pscroll = 0;
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if self.tcursor + 1 < n {
                    self.tcursor += 1;
                    self.pscroll = 0;
                }
            }
            KeyCode::Enter => self.open_thread(),
            _ => return self.tlist.key(k),
        }
        Out::None
    }

    fn agents_key(&mut self, k: KeyEvent) {
        if self.afilter_editing {
            match k.code {
                KeyCode::Esc | KeyCode::Enter => self.afilter_editing = false,
                KeyCode::Up => if let Some(i) = self.agent_step(0, -1) { self.acursor = i },
                KeyCode::Down => if let Some(i) = self.agent_step(0, 1) { self.acursor = i },
                _ => {
                    if matches!(self.afilter_ta.key(k), Input::Handled) {
                        self.sync_afilter();
                    }
                }
            }
            self.pscroll = 0;
            return;
        }
        let step = |a: &mut Self, i: Option<usize>| if let Some(i) = i { a.acursor = i; a.pscroll = 0; };
        match k.code {
            KeyCode::Up | KeyCode::Char('k') => match self.agent_step(0, -1) {
                None => self.tabs_focus = true,
                i => step(self, i),
            },
            KeyCode::Down | KeyCode::Char('j') => step(self, self.agent_step(0, 1)),
            KeyCode::Left | KeyCode::Char('h') => step(self, self.agent_step(-1, 0)),
            KeyCode::Right | KeyCode::Char('l') => step(self, self.agent_step(1, 0)),
            KeyCode::Esc if self.afilter.as_deref().is_some_and(|q| !q.is_empty()) => {
                self.afilter = None;
                self.afilter_ta.set_text("");
                self.aoffset = 0;
            }
            KeyCode::Enter => self.run(Act::EditAgent),
            KeyCode::Tab | KeyCode::BackTab => {}
            _ => {
                let name = key_name(&k);
                if let Some(c) = self.commands().0.into_iter().find(|c| c.key == name) {
                    self.run(c.act);
                }
            }
        }
    }

    fn toggle_pick(&mut self) {
        let Some(a) = self.agents.get(self.pick_cursor) else { return };
        let name = a.name.clone();
        if let Some(i) = self.picks.iter().position(|p| *p == name) {
            self.picks.remove(i);
        } else {
            self.picks.push(name);
        }
    }

    /// The repo (else the dir) the flow default belongs to.
    fn flow_key(&self) -> String {
        self.repo.as_ref().map(|r| r.path.display().to_string()).unwrap_or_else(|| self.cfg.cwd.display().to_string())
    }

    /// The flows of the task's repo read again; the lead's default: the repo's last flow. A pick
    /// of a flow the repo lacks: 0 flow.
    pub fn load_task_flows(&mut self) {
        let dir = self.repo.as_ref().map(|r| r.path.clone()).unwrap_or_else(|| self.cfg.cwd.clone());
        self.flow_lib = crate::engine::flows::Library::load(Some(&dir));
        let lib = &self.flow_lib;
        self.flow_default = flow_default_get(&self.flow_key()).filter(|s| lib.flows.iter().any(|f| f.slug == *s));
        self.agent_flows.retain(|_, (s, _)| s.is_empty() || lib.flows.iter().any(|f| f.slug == *s));
        self.flow_menu = None;
    }

    /// The new thread's lead: the 1st picked agent in the cards' order.
    fn task_lead(&self) -> Option<String> {
        self.agents.iter().find(|a| self.picks.contains(&a.name)).map(|a| a.name.clone())
    }

    /// AGENT's flow for the new thread: (slug, start step); "" = 0 flow. The lead with 0 pick: the
    /// repo's last flow.
    fn flow_of(&self, agent: &str) -> (String, usize) {
        if let Some(f) = self.agent_flows.get(agent) {
            return f.clone();
        }

        match &self.flow_default {
            Some(d) if self.task_lead().as_deref() == Some(agent) => (d.clone(), 0),
            _ => (String::new(), 0),
        }
    }

    /// AGENT's flow as the dropdown's row (0: 0 flow, I: the I-th flow) and its start step.
    fn flow_at(&self, agent: &str) -> (usize, usize) {
        let (slug, start) = self.flow_of(agent);
        match self.flow_lib.flows.iter().position(|f| f.slug == slug) {
            Some(i) => (i + 1, start),
            None => (0, 0),
        }
    }

    /// The resolved steps of flow I (1-based; 0: 0 flow → 0 steps).
    fn flow_steps(&self, i: usize) -> Vec<crate::engine::flows::Resolved> {
        i.checked_sub(1).and_then(|i| self.flow_lib.flows.get(i)).map(|f| crate::engine::flows::resolve(f, &self.flow_lib)).unwrap_or_default()
    }

    /// The flow field for the setup tutorial's coach: (the flow dropdown open, the picked flow's
    /// slug; None: 0 flow).
    pub fn flow_peek(&self) -> (bool, Option<String>) {
        let lead = self.task_lead().map(|l| self.flow_of(&l).0);
        let slug = lead.into_iter().chain(self.agent_flows.values().map(|f| f.0.clone())).find(|s| !s.is_empty());
        (self.flow_menu.is_some(), slug)
    }

    /// The flow dropdown of the card under the cursor (`f`), on its flow (steps closed).
    fn open_flow_menu(&mut self) {
        let Some(agent) = self.agents.get(self.pick_cursor).map(|a| a.name.clone()) else { return };
        let (cursor, _) = self.flow_at(&agent);
        self.flow_menu = Some(FlowMenu { agent, cursor, offset: 0, sub: None });
    }

    /// Flow I picked for the dropdown's agent, from step START: the dropdown closes. A flow for an
    /// unpicked agent picks it.
    fn pick_flow(&mut self, i: usize, start: usize) {
        let Some(agent) = self.flow_menu.take().map(|m| m.agent) else { return };
        let slug = i.checked_sub(1).and_then(|i| self.flow_lib.flows.get(i)).map(|f| f.slug.clone()).unwrap_or_default();
        let start = if slug.is_empty() { 0 } else { start.min(self.flow_steps(i).len().saturating_sub(1)) };
        if !slug.is_empty() && !self.picks.contains(&agent) {
            self.picks.push(agent.clone());
        }
        self.agent_flows.insert(agent, (slug, start));
    }

    /// Keys of the open flow dropdown: ↑↓ move, enter / → the flow's steps (0 flow: picked),
    /// a letter: the next flow with it, esc / ← close. In the steps: ↑↓, enter picks, esc / ← back.
    fn flow_menu_key(&mut self, k: KeyEvent) {
        let Some(m) = self.flow_menu.as_ref() else { return };
        let (cur, sub) = (m.cursor, m.sub);
        let agent = m.agent.clone();
        let (fi, fs) = self.flow_at(&agent);
        let n = self.flow_lib.flows.len() + 1;
        if let Some((s, _)) = sub {
            let steps = self.flow_steps(cur).len().max(1);
            let set = |a: &mut App, s: usize| {
                if let Some(m) = a.flow_menu.as_mut() {
                    m.sub = Some((s, m.sub.map_or(0, |x| x.1)));
                }
            };
            match k.code {
                KeyCode::Esc | KeyCode::Left | KeyCode::Char('h') => {
                    if let Some(m) = self.flow_menu.as_mut() {
                        m.sub = None;
                    }
                }
                KeyCode::Up | KeyCode::Char('k') => set(self, (s + steps - 1) % steps),
                KeyCode::Down | KeyCode::Char('j') => set(self, (s + 1) % steps),
                KeyCode::Enter | KeyCode::Char(' ') => self.pick_flow(cur, s),
                _ => {}
            }
            return;
        }
        let set = |a: &mut App, c: usize| {
            if let Some(m) = a.flow_menu.as_mut() {
                m.cursor = c;
            }
        };
        match k.code {
            KeyCode::Esc | KeyCode::Left | KeyCode::Char('h') => self.flow_menu = None,
            KeyCode::Up | KeyCode::Char('k') => set(self, (cur + n - 1) % n),
            KeyCode::Down | KeyCode::Char('j') => set(self, (cur + 1) % n),
            KeyCode::Enter | KeyCode::Right | KeyCode::Char(' ' | 'l') => {
                if cur == 0 {
                    self.pick_flow(0, 0);
                } else if let Some(m) = self.flow_menu.as_mut() {
                    // the steps: on the picked start when it is this flow
                    m.sub = Some((if cur == fi { fs } else { 0 }, 0));
                }
            }
            KeyCode::Char('0') => self.pick_flow(0, 0),
            KeyCode::Char(c) if c.is_alphanumeric() => {
                let c = c.to_ascii_lowercase();
                let hits: Vec<usize> = (0..self.flow_lib.flows.len()).filter(|&i| self.flow_lib.flows[i].name.to_lowercase().starts_with(c)).map(|i| i + 1).collect();
                if let Some(&next) = hits.iter().find(|&&i| i > cur).or(hits.first()) {
                    set(self, next);
                }
            }
            _ => {}
        }
    }

    /// A click with the flow dropdown open: a flow row opens its steps (0 flow: picked), a step
    /// row picks it, a click next to them closes the dropdown.
    fn flow_menu_click(&mut self, at: Position) {
        if let Some(&(_, s)) = self.regions.flow_steps.iter().find(|(r, _)| r.contains(at)) {
            let cur = self.flow_menu.as_ref().map_or(0, |m| m.cursor);
            return self.pick_flow(cur, s);
        }
        if let Some(&(_, i)) = self.regions.flow_rows.iter().find(|(r, _)| r.contains(at)) {
            if i == 0 {
                return self.pick_flow(0, 0);
            }
            let agent = self.flow_menu.as_ref().map(|m| m.agent.clone()).unwrap_or_default();
            let (fi, fs) = self.flow_at(&agent);
            let start = if i == fi { fs } else { 0 };
            self.flow_menu = Some(FlowMenu { agent, cursor: i, offset: self.flow_menu.as_ref().map_or(0, |m| m.offset), sub: Some((start, 0)) });
            return;
        }
        self.flow_menu = None;
    }

    /// The open flow dropdown at its agent's card, the steps dropdown of the flow under the cursor
    /// beside it.
    fn draw_flow_menu(&mut self, f: &mut Frame, bounds: Rect) {
        let Some((agent, cur, off, sub)) = self.flow_menu.as_ref().map(|m| (m.agent.clone(), m.cursor, m.offset, m.sub)) else { return };
        let (fi, fs) = self.flow_at(&agent);
        let mut items = vec![dropdown::Row { text: "0 flow".into(), detail: "free-form".into(), now: fi == 0, marks: Vec::new() }];
        for (i, fl) in self.flow_lib.flows.iter().enumerate() {
            let n = crate::engine::flows::resolve(fl, &self.flow_lib).len();
            items.push(dropdown::Row { text: format!("◆ {}", fl.name), detail: format!("{n} steps ›"), now: fi == i + 1, marks: Vec::new() });
        }
        let n = items.len();
        let w = items.iter().map(|r| r.text.width() + r.detail.width() + 8).max().unwrap_or(0).clamp(30, 60) as u16;
        let anchor = self.regions.menu_rows.iter().find(|(_, i)| self.agents.get(*i).is_some_and(|a| a.name == agent)).map_or(Rect::default(), |(r, _)| *r);
        let Some(r) = dropdown::place(anchor, w, n, bounds, false) else { return };
        let note = if sub.is_some() { "← back".to_string() } else { "→ start step".to_string() };
        // the parent: gray while the steps have the keys
        let (off, drawn) = dropdown::list_in(f, r, &format!("flow of {agent}"), &note, &items, cur, off, "0 flows", sub.is_none());
        self.regions.flow_menu = r;
        self.regions.flow_rows = drawn.clone();
        if let Some(m) = self.flow_menu.as_mut() {
            m.offset = off;
        }
        // the flow under the cursor: its rail beside the list at once (the flows tab's look);
        // enter / →: the rail takes the keys (the start step)
        self.regions.flow_steps.clear();
        let Some(fl) = cur.checked_sub(1).and_then(|i| self.flow_lib.flows.get(i)).cloned() else { return };
        let nodes = crate::engine::flows::resolve(&fl, &self.flow_lib);
        let rows = crate::flows::rail_rows(&nodes);
        let arcs = crate::flowrail::arcs(&nodes, |_| 0);
        let head = crate::flows::head_line(&fl, &nodes);
        let titles = rows.iter().map(|r| r.title.width() + r.tags.iter().map(|(t, _)| t.width() + 2).sum::<usize>()).max().unwrap_or(0);
        let labels = nodes.iter().flat_map(|n| n.edges.iter().map(|(k, _)| k.width() + 4)).chain(nodes.iter().map(|n| n.loops.as_ref().map_or(0, |l| l.1.width() + 14))).max().unwrap_or(0);
        let want = (head.width().max(fl.description.width()).max(titles + 8 + usize::from(crate::flowrail::arcs_width(&arcs)) + labels) + 4) as u16;
        let row_y = drawn.iter().find(|(_, i)| *i == cur).map_or(r.y + 1, |(rr, _)| rr.y);
        // beside the list when 50+ columns are there (long loop labels then cut), else above / below it
        let right = bounds.right().saturating_sub(r.right());
        let w = if right >= 50 { want.clamp(40, right) } else { want.max(40) };
        let Some(sr) = dropdown::place_beside_max(r, row_y, w, rows.len() + 3, 40, bounds, r) else { return };
        f.render_widget(ratatui::widgets::Clear, sr);
        let note = if sub.is_some() { " ↑↓ start · enter: pick · ← back " } else { " → start step · enter: from step 1 " };
        let block = ratatui::widgets::Block::default()
            .borders(ratatui::widgets::Borders::ALL)
            .border_style(if sub.is_some() { theme::accent() } else { theme::line() })
            .title(Span::styled(" preview ", theme::muted()))
            .title_bottom(Span::styled(note, theme::muted()));
        let inner = block.inner(sr);
        f.render_widget(block, sr);
        let inner = Rect { x: inner.x + 1, width: inner.width.saturating_sub(2), ..inner };
        f.render_widget(Paragraph::new(head), Rect { height: 1.min(inner.height), ..inner });
        if !fl.description.is_empty() && inner.height > 1 {
            f.render_widget(Paragraph::new(Line::styled(crate::nav::fit(&fl.description, usize::from(inner.width)), theme::muted())), Rect { y: inner.y + 1, height: 1, ..inner });
        }
        let rail = Rect { y: inner.y + 3, height: inner.height.saturating_sub(3), ..inner };
        let cursor = match sub {
            Some((s, _)) => Some(s),
            None => (cur == fi).then_some(fs),
        };
        let off = sub.map_or(0, |(s, soff)| crate::dropdown::fit(soff, s, rows.len(), usize::from(rail.height)));
        self.regions.flow_steps = crate::flowrail::draw(f.buffer_mut(), rail, &rows, &arcs, cursor, sub.is_some(), off);
        if let Some(m) = self.flow_menu.as_mut()
            && let Some((s, _)) = m.sub
        {
            m.sub = Some((s, off));
        }
    }

    /// The task's fields top to bottom: drafts row, prompt, title, where + its boxes (a
    /// git repo), agents, notes.
    fn fields(&self) -> Vec<Field> {
        let mut v = vec![Field::Drafts];
        // the repo row: always (the picker finds a repo also when codehead runs outside of one)
        v.extend([Field::Prompt, Field::Title, Field::Repo]);
        if !self.wheres.is_empty() {
            v.extend([Field::Where, Field::Opts]);
        }
        v.extend([Field::Agents, Field::Notes]);
        v
    }

    /// ↑↓ to the field D steps away: above the top one the tabs row, below the last 0 move.
    fn go_field(&mut self, d: isize) {
        let f = self.fields();
        let at = f.iter().position(|x| *x == self.field).unwrap_or(0) as isize + d;
        if at < 0 {
            self.tabs_focus = true;
        } else if let Some(&x) = f.get(at as usize) {
            self.field = x;
            // the opts boxes from above: the 1st, from below: the last
            if x == Field::Opts {
                self.opt_i = if d > 0 { 0 } else { 1 };
            }
        }
    }

    /// Tab / shift+tab: the next / previous field, round (the drafts row only on ↑).
    fn tab_field(&mut self, d: isize) {
        let f: Vec<Field> = self.fields().into_iter().filter(|x| *x != Field::Drafts).collect();
        let n = f.len() as isize;
        let at = f.iter().position(|x| *x == self.field).map_or(0, |i| i as isize);
        self.field = f[(at + d).rem_euclid(n) as usize];
    }

    fn compose_key(&mut self, k: KeyEvent) {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        let alt = k.modifiers.contains(KeyModifiers::ALT);
        let plain = !k.modifiers.contains(KeyModifiers::SHIFT);
        match k.code {
            KeyCode::Esc => return,
            // tab: the next field (main tabs: alt+1-4)
            KeyCode::Tab if k.modifiers.is_empty() => return self.tab_field(1),
            KeyCode::BackTab => return self.tab_field(-1),
            // cmd+enter (SUPER) needs a terminal that reports it (kitty keyboard protocol)
            KeyCode::Enter if ctrl || alt || k.modifiers.contains(KeyModifiers::SUPER) => return self.start(),
            KeyCode::Char('o') if ctrl => return self.run(Act::OpenNotes),
            KeyCode::Char('s') if ctrl => return self.run(Act::SaveDraft),
            // ctrl+v / ⌘v in the prompt, like Claude Code: the clipboard image, else its text
            KeyCode::Char('v' | 'V') if self.field == Field::Prompt && (ctrl || k.modifiers.contains(KeyModifiers::SUPER)) => {
                let p = self.paster.clipboard();
                return self.paste_prompt(p);
            }
            // the prompt (top field): ↑ on its 1st row / ↓ on its last row leave it
            KeyCode::Up if self.field == Field::Prompt && plain && self.prompt.on_first_row() => return self.go_field(-1),
            KeyCode::Down if self.field == Field::Prompt && plain && self.prompt.on_last_row() => return self.go_field(1),
            KeyCode::Enter | KeyCode::Down if self.field == Field::Title => return self.go_field(1),
            KeyCode::Up if self.field == Field::Title => return self.go_field(-1),
            _ => {}
        }
        if self.field == Field::Drafts {
            match k.code {
                KeyCode::Enter | KeyCode::Char(' ') if self.draft_btn() == 0 => self.open_drafts(),
                KeyCode::Enter | KeyCode::Char(' ') => self.save_draft(),
                KeyCode::Left | KeyCode::Char('h') if !self.drafts.is_empty() => self.draft_btn = 0,
                KeyCode::Right | KeyCode::Char('l') => self.draft_btn = 1,
                KeyCode::Down => self.go_field(1),
                KeyCode::Up => self.go_field(-1),
                _ => {}
            }
            return;
        }
        if self.field == Field::Repo {
            match k.code {
                KeyCode::Enter | KeyCode::Char(' ') => self.open_repo_pick(),
                KeyCode::Up => self.go_field(-1),
                KeyCode::Down => self.go_field(1),
                _ => {}
            }
            return;
        }
        if self.field == Field::Where {
            match k.code {
                KeyCode::Enter | KeyCode::Char(' ') => self.open_where(),
                KeyCode::Up => self.go_field(-1),
                KeyCode::Down => self.go_field(1),
                _ => {}
            }
            return;
        }
        // opts: 1 box per row, ↑↓ go through them
        if self.field == Field::Opts {
            match k.code {
                KeyCode::Char(' ') => self.toggle_opt(self.opt_i),
                KeyCode::Up | KeyCode::Char('k') if self.opt_i > 0 => self.opt_i -= 1,
                KeyCode::Down | KeyCode::Char('j') if self.opt_i == 0 => self.opt_i = 1,
                KeyCode::Up => self.go_field(-1),
                KeyCode::Enter | KeyCode::Down => self.go_field(1),
                _ => {}
            }
            return;
        }
        if self.field == Field::Notes {
            match k.code {
                KeyCode::Enter | KeyCode::Char(' ') => self.open_notes(),
                KeyCode::Up => self.go_field(-1),
                KeyCode::Char('x') | KeyCode::Backspace | KeyCode::Delete => {
                    self.selected.clear();
                    self.carry.clear();
                }
                _ => {}
            }
            return;
        }
        if self.field == Field::Agents {
            let step = |a: &mut Self, dx, dy| {
                if let Some(i) = Self::grid_step(&a.pick_places, a.pick_cursor, dx, dy) {
                    a.pick_cursor = i;
                    true
                } else { false }
            };
            match k.code {
                KeyCode::Left | KeyCode::Char('h') => { step(self, -1, 0); }
                KeyCode::Right | KeyCode::Char('l') => { step(self, 1, 0); }
                KeyCode::Char(' ') => self.toggle_pick(),
                // f: the card's flow (a flow picks the agent)
                KeyCode::Char('f') => self.open_flow_menu(),
                KeyCode::Up | KeyCode::Char('k') => if !step(self, 0, -1) { self.go_field(-1); },
                KeyCode::Down | KeyCode::Char('j') => if !step(self, 0, 1) { self.go_field(1); },
                KeyCode::Enter => self.go_field(1),
                _ => {}
            }
            return;
        }
        let ta = if self.field == Field::Title { &mut self.title } else { &mut self.prompt };
        let _: Input = ta.key(k);
    }

    fn modal_key(&mut self, k: KeyEvent) {
        let Some(modal) = self.modal.take() else { return };
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        match modal {
            Modal::Confirm { title, question, yes, action } => match k.code {
                KeyCode::Char('y') => self.confirmed(action),
                KeyCode::Char('n') | KeyCode::Esc => self.declined(action),
                KeyCode::Enter => {
                    if yes {
                        self.confirmed(action)
                    } else {
                        self.declined(action)
                    }
                }
                KeyCode::Left | KeyCode::Right | KeyCode::Tab | KeyCode::BackTab | KeyCode::Char('h' | 'l') => {
                    self.modal = Some(Modal::Confirm { title, question, yes: !yes, action })
                }
                _ => self.modal = Some(Modal::Confirm { title, question, yes, action }),
            },
            Modal::Agent(mut form) => {
                let next = |f: usize| (f + 1) % FORM_FIELDS;
                let prev = |f: usize| (f + FORM_FIELDS - 1) % FORM_FIELDS;
                // the avatar picker: its keys first
                if let Some(p) = form.picker.as_mut() {
                    match p.key(k) {
                        crate::picker::Act::None => {}
                        crate::picker::Act::Close => form.picker = None,
                        crate::picker::Act::Take(s) => {
                            form.sprite = s;
                            form.picker = None;
                        }
                    }
                    self.modal = Some(Modal::Agent(form));
                    return;
                }
                // harness / model dropdown: its keys first
                if form.pick.is_some() {
                    self.form_pick_key(&mut form, k);
                    self.modal = Some(Modal::Agent(form));
                    return;
                }
                match k.code {
                    // 0 edits: closed; edits: asked first (Esc never drops work unasked)
                    KeyCode::Esc if !form.dirty() => return,
                    KeyCode::Esc => {
                        let what = if form.edit.is_some() { "edits of this agent" } else { "this new agent" };
                        self.modal = Some(Modal::Confirm { title: "discard".into(), question: format!("drop the {what}?"), yes: false, action: ConfirmAction::DiscardAgent(form) });
                        return;
                    }
                    KeyCode::Char('s') if ctrl => return self.save_agent(form),
                    KeyCode::Enter if ctrl => return self.save_agent(form),
                    KeyCode::Tab => form.field = next(form.field),
                    KeyCode::BackTab => form.field = prev(form.field),
                    KeyCode::Down if !form.multiline() => form.field = next(form.field),
                    KeyCode::Up if !form.multiline() => form.field = prev(form.field),
                    // voice, system prompt: ↑ on the 1st row / ↓ on the last leaves the box (shift: selects)
                    KeyCode::Down if k.modifiers.is_empty() && form.text().is_some_and(|t| t.on_last_row()) => form.field = next(form.field),
                    KeyCode::Up if k.modifiers.is_empty() && form.text().is_some_and(|t| t.on_first_row()) => form.field = prev(form.field),
                    KeyCode::Char('r') if !ctrl && matches!(form.field, F_SET | F_SPRITE) => form.random_sprite(),
                    KeyCode::Enter if matches!(form.field, F_SET | F_SPRITE) => form.picker = Some(crate::picker::Picker::new(&form.sprite)),
                    KeyCode::Enter if form.field == F_BUTTONS => {
                        if form.save {
                            self.save_agent(form);
                        }
                        return;
                    }
                    KeyCode::Enter if matches!(form.field, F_HARNESS | F_MODEL) => {
                        let model = form.field == F_MODEL;
                        self.open_form_pick(&mut form, model);
                    }
                    // MCP servers: ←→ pick 1, space: on / off
                    KeyCode::Left | KeyCode::Right if form.field == F_MCP => {
                        let n = form.mcp.len().max(1);
                        form.mcp_cur = if k.code == KeyCode::Left { (form.mcp_cur + n - 1) % n } else { (form.mcp_cur + 1) % n };
                    }
                    KeyCode::Char(' ') if form.field == F_MCP => {
                        let i = form.mcp_cur;
                        if let Some(m) = form.mcp.get_mut(i) {
                            m.1 = !m.1;
                        }
                    }
                    KeyCode::Enter if !form.multiline() => form.field = next(form.field),
                    KeyCode::Left | KeyCode::Right if matches!(form.field, F_MODEL | F_EFFORT | F_COLOR | F_MODE | F_STYLE | F_BUTTONS) => {
                        let d = if k.code == KeyCode::Left { -1 } else { 1 };
                        let cycle = |i: usize, n: usize| ((i as isize + d).rem_euclid(n as isize)) as usize;
                        match form.field {
                            F_MODEL if form.is_claude() => form.model = cycle(form.model, form.models.len()),
                            F_MODEL => {}
                            F_EFFORT => form.effort = cycle(form.effort, agents::EFFORTS.len()),
                            F_COLOR => form.color = cycle(form.color, agents::COLORS.len()),
                            F_MODE => form.mode = cycle(form.mode, agents::MODES.len()),
                            F_STYLE => form.style = cycle(form.style, form.styles.len()),
                            _ => form.save = !form.save,
                        }
                    }
                    _ => {
                        form.error = None;
                        if let Some(ta) = form.text() {
                            let _: Input = ta.key(k);
                        }
                    }
                }
                self.modal = Some(Modal::Agent(form));
            }
            Modal::Info { title, lines, scroll } => match k.code {
                KeyCode::Enter | KeyCode::Esc | KeyCode::Char('q') => {}
                KeyCode::Down | KeyCode::Char('j') => self.modal = Some(Modal::Info { title, lines, scroll: scroll.saturating_add(1) }),
                KeyCode::Up | KeyCode::Char('k') => self.modal = Some(Modal::Info { title, lines, scroll: scroll.saturating_sub(1) }),
                _ => self.modal = Some(Modal::Info { title, lines, scroll }),
            },
        }
    }

    pub fn paste(&mut self, text: &str) {
        self.paste_inner(text);
        self.backup();
    }

    fn paste_inner(&mut self, text: &str) {
        self.settings_typed |= self.screen == Screen::Pick && self.tab == Tab::Settings;
        if let Some(Modal::Agent(form)) = &mut self.modal
            && let Some(ta) = form.text()
        {
            ta.insert(text);
            return;
        }
        if self.modal.is_some() {
            return;
        }
        if self.screen == Screen::Pick && self.tab == Tab::Settings {
            if let Some(settings) = &mut self.settings { settings.paste(text); }
            return;
        }
        if self.screen == Screen::Pick && self.tab == Tab::Flows {
            self.flows_tab.paste(text);
            return;
        }
        match self.screen {
            Screen::Compose if self.field == Field::Title => self.title.insert(text),
            // like the thread view: a paste on any other field goes to the prompt
            Screen::Compose => {
                let p = self.paster.paste(text);
                self.paste_prompt(p);
            }
            Screen::Pick if self.tlist.filtering => {
                self.tlist.filter.insert(text);
                self.tlist.cursor = 0;
            }
            Screen::Pick if self.afilter_editing => {
                self.afilter_ta.insert(text);
                self.sync_afilter();
            }
            Screen::Pick if self.filter_editing => {
                self.filter_ta.insert(text);
                self.sync_filter();
            }
            Screen::Pick => {}
        }
    }

    // ---------- mouse ----------
    pub fn mouse(&mut self, m: MouseEvent) {
        if let Some(p) = self.notes_panel.as_mut() {
            if matches!(m.kind, MouseEventKind::Down(_)) {
                let pick = p.click(ratatui::layout::Position { x: m.column, y: m.row });
                self.notes_pick(pick);
            }
            return;
        }
        self.mouse_inner(m);
        if !matches!(m.kind, MouseEventKind::Moved) {
            self.backup();
        }
    }

    fn mouse_inner(&mut self, m: MouseEvent) {
        let at = Position { x: m.column, y: m.row };
        self.hover = Some(at);
        // ctrl+click on a URL anywhere on screen: the browser
        if matches!(m.kind, MouseEventKind::Down(MouseButton::Left))
            && m.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::SUPER)
            && let Some(url) = crate::urls::at(&self.last_frame, at)
        {
            crate::merge::open_url(&url);
            self.toast(format!("open {url}"), false);
            return;
        }
        if let Some(menu) = self.menu.as_mut() {
            let a = menu.mouse(m);
            return self.menu_action(a);
        }
        if self.modal.is_none() && self.screen == Screen::Pick && self.tab == Tab::Flows && self.flows_area.contains(at) {
            let o = self.flows_tab.mouse(m);
            return self.flows_out(o);
        }
        if self.modal.is_none() && self.screen == Screen::Pick && self.tab == Tab::Merge && self.merge_area.contains(at) {
            let o = self.merge_tab.mouse(m);
            return self.merge_out(o);
        }
        let down = matches!(m.kind, MouseEventKind::Down(MouseButton::Left));
        if let Some(Modal::Agent(form)) = &mut self.modal {
            // harness / model: a click opens the dropdown; an open one takes the click
            let opens = if down && form.pick.is_none() { [F_HARNESS, F_MODEL].into_iter().find(|&i| form.areas[i].contains(at)) } else { None };
            if down && (form.pick.is_some() || opens.is_some()) {
                let Some(Modal::Agent(mut form)) = self.modal.take() else { return };
                match opens {
                    Some(i) => {
                        form.field = i;
                        self.open_form_pick(&mut form, i == F_MODEL);
                    }
                    None => self.form_pick_click(&mut form, at),
                }
                self.modal = Some(Modal::Agent(form));
                return;
            }
            if down && let Some(i) = form.areas.iter().position(|r| r.contains(at)) {
                form.field = i;
            }
            let field = form.field;
            for (i, ta) in [(F_NAME, &mut form.name), (F_HARNESS, &mut form.harness), (F_VOICE, &mut form.voice), (F_PROMPT, &mut form.prompt)] {
                if i == field
                    && ta.mouse(m)
                    && let Some(n) = ta.copied.take()
                {
                    self.toast = Some((format!("copied {n} chars"), Instant::now(), false));
                }
            }
            if down && let Some(&(_, save)) = self.regions.buttons.iter().find(|(r, _)| r.contains(at)) {
                let Some(Modal::Agent(form)) = self.modal.take() else { return };
                if save {
                    self.save_agent(form);
                }
            }
            return;
        }
        if self.modal.is_some() {
            if !down {
                return;
            }
            // a click on yes / no: as y / n (no: a discarded agent form comes back)
            if let Some(&(_, yes)) = self.regions.buttons.iter().find(|(r, _)| r.contains(at))
                && matches!(self.modal, Some(Modal::Confirm { .. }))
                && let Some(Modal::Confirm { action, .. }) = self.modal.take()
            {
                if yes {
                    self.confirmed(action);
                } else {
                    self.declined(action);
                }
            }
            return;
        }
        // the threads tab's list (filter, repo + view tabs, cards, actions, form): its own clicks;
        // this screen's rows (worktrees, claude views): a click picks, a 2nd opens
        if self.screen == Screen::Pick && self.tab == Tab::Threads && self.regions.list.contains(at) && !self.psel.is_dragging() {
            use crate::threadlist::Out;
            if down {
                self.tabs_focus = false;
            }
            match self.tlist.mouse(m) {
                Out::Open(c) => return self.open_card(&c),
                Out::Pass if !self.tlist.view.cards() && self.tlist.list.contains(at) => {
                    let n = self.n_threads();
                    match m.kind {
                        MouseEventKind::Down(MouseButton::Left) => {
                            let i = self.toffset + (m.row - self.tlist.list.y) as usize;
                            if i < n {
                                self.tlist.focus = crate::threadlist::Focus::List;
                                if i == self.tcursor {
                                    self.open_thread();
                                } else {
                                    self.tcursor = i;
                                }
                            }
                        }
                        MouseEventKind::ScrollDown => self.tcursor = (self.tcursor + 1).min(n.saturating_sub(1)),
                        MouseEventKind::ScrollUp => self.tcursor = self.tcursor.saturating_sub(1),
                        _ => {}
                    }
                }
                _ => {}
            }
            self.pscroll = 0;
            return;
        }
        if down && let Some(&(_, tab)) = self.regions.tabs.iter().find(|(r, _)| r.contains(at)) {
            self.switch_tab(tab);
            self.tabs_focus = false;
            return;
        }
        if self.screen == Screen::Pick && self.tab == Tab::Settings {
            self.tabs_focus = false;
            let action = self.settings.as_mut().map(|s| s.mouse(m)).unwrap_or(crate::settings::Action::None);
            self.settings_action(action);
            return;
        }
        if self.screen == Screen::Compose {
            // drafts dropdown: a click on a row loads it, a click next to it closes it
            if self.drafts_menu.is_some() {
                if down {
                    match self.regions.draft_rows.iter().find(|(r, _)| r.contains(at)) {
                        Some(&(_, i)) => self.load_draft(i),
                        None => self.drafts_menu = None,
                    }
                }
                return;
            }
            // flow dropdown: a click on a flow opens its steps, on a step picks it, next to them closes it
            if self.flow_menu.is_some() {
                if down {
                    self.flow_menu_click(at);
                }
                return;
            }
            // where dropdown: a click on a row picks it, a click next to it closes it
            if self.where_menu.is_some() {
                if down {
                    match self.regions.where_rows.iter().find(|(r, _)| r.contains(at)) {
                        Some(&(_, i)) => self.pick_where(i),
                        None => self.where_menu = None,
                    }
                }
                return;
            }
            // repo picker: a click on a dir moves there (left: current dir, right: into it), a
            // click outside of it closes it
            if let Some(p) = self.repo_pick.as_mut() {
                if down && !p.click(at.x, at.y) && !self.regions.repo_pick.contains(at) {
                    self.repo_pick = None;
                }
                return;
            }
            if down && self.regions.drafts.contains(at) {
                self.field = Field::Drafts;
                self.draft_btn = 0;
                return self.open_drafts();
            }
            if down && self.regions.save_draft.contains(at) {
                self.field = Field::Drafts;
                self.draft_btn = 1;
                return self.save_draft();
            }
            if self.mention_on() {
                if down && self.mention.mouse(m, &mut self.prompt, &self.agents) {
                    return;
                }
                if self.command.mouse(m, &mut self.prompt) || self.shortcuts.mouse(m, &mut self.prompt) || self.paths.mouse(m, &mut self.prompt) {
                    return;
                }
            }
            if down && let Some(&(_, i)) = self.regions.opts.iter().find(|(r, _)| r.contains(at)) {
                self.field = Field::Opts;
                return self.toggle_opt(i);
            }
            if down {
                if self.title.area().contains(at) {
                    self.field = Field::Title;
                } else if self.prompt.area().contains(at) {
                    self.field = Field::Prompt;
                } else if self.regions.repo_row.contains(at) {
                    self.field = Field::Repo;
                    return self.open_repo_pick();
                } else if self.regions.where_row.contains(at) {
                    self.field = Field::Where;
                    return self.open_where();
                } else if self.regions.notes_label.contains(at) {
                    self.field = Field::Notes;
                    return self.open_notes();
                }
            }
            if down && let Some(&(_, i)) = self.regions.menu_rows.iter().find(|(r, _)| r.contains(at)) {
                self.field = Field::Agents;
                self.pick_cursor = i;
                self.toggle_pick();
            }
            if self.regions.task_agents.contains(at) {
                let dy = match m.kind {
                    MouseEventKind::ScrollUp => -1,
                    MouseEventKind::ScrollDown => 1,
                    _ => 0,
                };
                if dy != 0 {
                    self.field = Field::Agents;
                    if let Some(i) = Self::grid_step(&self.pick_places, self.pick_cursor, 0, dy) {
                        self.pick_cursor = i;
                    }
                    return;
                }
            }
            self.title.mouse(m);
            self.prompt.mouse(m);
            for ta in [&mut self.title, &mut self.prompt] {
                if let Some(n) = ta.copied.take() {
                    self.toast = Some((format!("copied {n} chars"), Instant::now(), false));
                }
            }
            return;
        }
        let list = self.regions.list;
        let in_list = list.contains(at);
        let in_preview = self.regions.preview.contains(at);
        // a filter input in edit: its own mouse (cursor, selection + copy)
        let mut copied = None;
        let mut took = false;
        for (on, ta) in [(self.filter_editing, &mut self.filter_ta), (self.tlist.filtering, &mut self.tlist.filter), (self.afilter_editing, &mut self.afilter_ta)] {
            if on && ta.mouse(m) {
                took = true;
                copied = ta.copied.take();
            }
        }
        if let Some(n) = copied {
            self.toast(format!("copied {n} chars"), false);
        }
        if took {
            return;
        }
        // the preview: text selection (drag, 2 clicks: word, 3: row), copied at the release
        let wheel = matches!(m.kind, MouseEventKind::ScrollDown | MouseEventKind::ScrollUp);
        if (in_preview && !wheel) || self.psel.is_dragging() {
            if matches!(m.kind, MouseEventKind::Down(_)) {
                self.tabs_focus = false;
            }
            if let Some(t) = self.psel.mouse(m, self.regions.preview, &self.last_frame) {
                let ok = crate::clip::copy(&t);
                self.toast(if ok { format!("copied {} chars", t.chars().count()) } else { "0 clipboard tools found (wl-copy, xclip, xsel)".into() }, !ok);
            }
            return;
        }
        if matches!(m.kind, MouseEventKind::Down(_)) {
            self.psel.clear();
        }
        match m.kind {
            MouseEventKind::Down(MouseButton::Left) if in_list => {
                self.tabs_focus = false;
                let i = (m.row - list.y) as usize;
                if self.tab == Tab::Notes {
                    let i = self.offset + i;
                    if i >= self.rows.len() {
                        return;
                    }
                    if i == self.cursor {
                        match &self.rows[i] {
                            Row::Dir { collapsed, .. } => {
                                let c = *collapsed;
                                self.fold(c);
                            }
                            _ => self.toggle_select(),
                        }
                    } else {
                        self.cursor = i;
                        self.pscroll = 0;
                    }
                } else if self.tab == Tab::Agents {
                    let _ = i;
                    if let Some(&(_, i)) = self.acards.iter().find(|(r, _)| r.contains(at)) {
                        if i == self.acursor {
                            self.run(Act::EditAgent);
                        } else {
                            self.acursor = i;
                            self.pscroll = 0;
                        }
                    }
                }
            }
            MouseEventKind::ScrollDown if in_preview => self.pscroll = self.pscroll.saturating_add(3),
            MouseEventKind::ScrollUp if in_preview => self.pscroll = self.pscroll.saturating_sub(3),
            MouseEventKind::ScrollDown | MouseEventKind::ScrollUp if in_list => {
                let d = if matches!(m.kind, MouseEventKind::ScrollDown) { 3 } else { -3 };
                if self.tab == Tab::Notes {
                    self.move_cursor(d);
                } else if self.tab == Tab::Agents {
                    // the card grid scrolls; the cursor stays on a card in view
                    self.aoffset = (self.aoffset as isize + d).max(0) as usize;
                    let (top, bottom) = (self.aoffset, self.aoffset + list.height as usize);
                    if let Some(&(i, _, _)) = self.aplace.iter().find(|&&(_, _, y)| y >= top && y + CARD_H as usize <= bottom)
                        && !self.aplace.iter().any(|&(c, _, y)| c == self.acursor && y >= top && y + CARD_H as usize <= bottom)
                    {
                        self.acursor = i;
                    }
                    self.pscroll = 0;
                } else {
                    let n = self.n_threads() as isize;
                    self.tcursor = (self.tcursor as isize + d).clamp(0, (n - 1).max(0)) as usize;
                    self.pscroll = 0;
                }
            }
            _ => {}
        }
    }

    // ---------- render ----------
    pub fn draw(&mut self, f: &mut Frame) {
        self.regions = Regions::default();
        let area = f.area();
        f.render_widget(Paragraph::new("").style(theme::text()), area);
        let [header, _, body, footer] = Layout::vertical([Constraint::Length(1), Constraint::Length(1), Constraint::Min(1), Constraint::Length(1)]).areas(area);
        let body = Rect { x: body.x + 1, width: body.width.saturating_sub(2), ..body };
        self.draw_header(f, header);
        match self.screen {
            Screen::Pick => self.draw_pick(f, body),
            Screen::Compose => self.draw_compose(f, body),
        }
        self.draw_footer(f, footer);
        // @ dropdown: after the screen, over every row of it (modals go on top)
        if self.mention_on() {
            self.mention.render(f, &self.prompt, &self.agents, area);
            let avoid = self.prompt.area();
            self.command.render(f, &self.prompt, area, avoid);
            self.shortcuts.render(f, &self.prompt, area, avoid);
            self.paths.render(f, &self.prompt, area, avoid);
            let open = self.shortcuts.is_open(&self.prompt);
            crate::shortcuts::render_popup(f, &self.prompt, self.hover, self.field == Field::Prompt, area, open);
        } else {
            self.shortcuts.hide();
        }
        if self.screen == Screen::Compose {
            self.draw_drafts(f, area);
            self.draw_where_menu(f, area);
            self.draw_flow_menu(f, area);
            let anchor = self.regions.repo_chip;
            if let Some(p) = self.repo_pick.as_mut() {
                self.regions.repo_pick = p.draw(f, anchor, body).unwrap_or_default();
            }
        }
        self.draw_modal(f, area);
        if let Some(m) = self.menu.as_mut() {
            m.render(f, area);
        }
        if let Some(p) = self.notes_panel.as_mut() {
            p.draw(f, area);
        }
        self.last_frame.clone_from(f.buffer_mut());
        if let Some(r) = self.hover_rect() {
            theme::hover(f.buffer_mut(), r);
        }
    }

    /// A key / click of the notifications dropdown: a jump opens the thread at the note's event.
    fn notes_pick(&mut self, p: crate::notify::Pick) {
        use crate::notify::Pick;
        match p {
            Pick::Stay => {}
            Pick::Close => self.notes_panel = None,
            Pick::Jump(n) => {
                self.notes_panel = None;
                crate::search::jump_to(&n.thread, n.ts);
                if !n.comment.is_empty() {
                    crate::changes::threads::jump_to(&n.thread, &n.comment);
                }
                self.outcome = Some(Outcome::Resume(Box::new(worktree_of(&n.card()))));
            }
        }
    }

    /// The clickable item under the mouse (regions of this frame): its area.
    fn hover_rect(&self) -> Option<Rect> {
        let at = self.hover?;
        if let Some(m) = &self.menu {
            return m.row_at(at);
        }
        let buttons = self.regions.buttons.iter().map(|(r, _)| *r).find(|r| r.contains(at));
        match &self.modal {
            Some(Modal::Agent(f)) if f.pick.is_some() => return find_rows(&self.regions.form_rows, at),
            Some(Modal::Confirm { .. } | Modal::Agent(_)) => return buttons,
            Some(_) => return None,
            None => {}
        }
        if let Some(r) = self.regions.tabs.iter().map(|(r, _)| *r).find(|r| r.contains(at)) {
            return Some(r);
        }
        if self.screen == Screen::Pick && self.tab == Tab::Settings {
            return self.settings.as_ref().and_then(|s| s.row_at(at));
        }
        if self.screen == Screen::Compose {
            if self.drafts_menu.is_some() {
                return find_rows(&self.regions.draft_rows, at);
            }
            if self.where_menu.is_some() {
                return find_rows(&self.regions.where_rows, at);
            }
            if self.flow_menu.is_some() {
                return find_rows(&self.regions.flow_rows, at).or_else(|| find_rows(&self.regions.flow_steps, at));
            }
            if let Some(p) = self.repo_pick.as_ref() {
                return p.left_rows.iter().chain(p.right_rows.iter()).map(|(r, _)| *r).find(|r| r.contains(at));
            }
            if self.mention_on()
                && let Some(r) = self.mention.row_at(at).or_else(|| self.command.row_at(at)).or_else(|| self.shortcuts.row_at(at)).or_else(|| self.paths.row_at(at))
            {
                return Some(r);
            }
            if let Some(r) = [self.regions.drafts, self.regions.save_draft].into_iter().find(|r| r.contains(at)) {
                return Some(r);
            }
            return find_rows(&self.regions.opts, at)
                .or_else(|| find_rows(&self.regions.menu_rows, at))
                .or_else(|| [self.regions.repo_row, self.regions.where_row, self.regions.notes_label].into_iter().find(|r| r.contains(at)));
        }
        let list = self.regions.list;
        if !list.contains(at) {
            return None;
        }
        if self.tab == Tab::Agents {
            return self.acards.iter().find(|(r, _)| r.contains(at)).map(|(r, _)| *r);
        }
        if self.tab == Tab::Threads {
            if let Some(r) = self.tlist.row_at(at) {
                return Some(r);
            }
            let rows = self.tlist.list;
            let i = at.y.checked_sub(rows.y)? as usize;
            return (!self.tlist.view.cards() && rows.contains(at) && self.toffset + i < self.n_threads()).then_some(Rect { y: at.y, height: 1, ..rows });
        }
        let i = (at.y - list.y) as usize;
        let item = match self.tab {
            Tab::Notes => self.offset + i < self.rows.len(),
            _ => false,
        };
        item.then_some(Rect { y: at.y, height: 1, ..list })
    }

    fn draw_header(&mut self, f: &mut Frame, area: Rect) {
        let mut spans = vec![Span::styled(BRAND, theme::accent_bold()), Span::raw(" ")];
        let mut x = area.x + BRAND.width() as u16 + 1;
        if self.screen == Screen::Pick && self.tab == Tab::Notes {
            spans.push(Span::styled(" new › notes ", theme::pill()));
        } else {
            for (tab, label) in [(Tab::New, "new"), (Tab::Threads, "threads"), (Tab::Merge, "merge"), (Tab::Agents, "agents"), (Tab::Flows, "flows"), (Tab::Settings, "settings")] {
                let active = self.cur_tab() == tab;
                let focused = active && self.tabs_focus;
                let (bg, fg) = match (active, focused) {
                    (true, true) => (Some(theme::pal().accent), theme::pal().on_accent),
                    (true, false) => (theme::pill_dim().bg, theme::pal().accent),
                    _ => (None, theme::pal().subtext0),
                };
                let base = bg.map_or(Style::default(), |b| Style::default().bg(b));
                let mut label = label.to_string();
                if tab == Tab::New && !self.selected.is_empty() {
                    label = format!("{label} {}", self.selected.len());
                }
                let w = (label.width() + 2) as u16;
                self.regions.tabs.push((Rect { x, y: area.y, width: w, height: 1 }, tab));
                x += w + 1;
                spans.push(Span::styled(format!(" {label} "), base.fg(fg).add_modifier(if active { Modifier::BOLD } else { Modifier::empty() })));
                spans.push(Span::raw(" "));
            }
        }
        f.render_widget(Paragraph::new(Line::from(spans)), area);
        // notifications: kind + agent count (ctrl+n lists them)
        let right = Line::from(crate::notify::badge(&crate::notify::unseen(None))).right_aligned();
        let right_area = Rect { x: x.min(area.right()), width: area.right().saturating_sub(x), ..area };
        f.render_widget(Paragraph::new(right), right_area);
    }

    fn split(&self, area: Rect) -> (Rect, Rect, bool) {
        // agents: a card grid, more room than a list
        let cards = matches!(self.tab, Tab::Agents | Tab::New);
        if area.width < THIN {
            let list = if cards { Constraint::Ratio(1, 2) } else { Constraint::Ratio(1, 3) };
            let [a, _, b] = Layout::vertical([list, Constraint::Length(1), Constraint::Fill(1)]).areas(area);
            (a, b, true)
        } else {
            let list = if cards { Constraint::Percentage(60) } else { Constraint::Percentage(40) };
            let [a, _, b] = Layout::horizontal([list, Constraint::Length(1), Constraint::Fill(1)]).areas(area);
            (a, b, false)
        }
    }

    fn draw_pick(&mut self, f: &mut Frame, area: Rect) {
        self.merge_area = Rect::default();
        if self.tab == Tab::Merge {
            self.merge_area = area;
            self.merge_tab.draw(f, area);
            return;
        }
        self.flows_area = Rect::default();
        if self.tab == Tab::Flows {
            self.flows_area = area;
            let active = !self.tabs_focus;
            self.flows_tab.draw(f, area, active);
            return;
        }
        if self.tab == Tab::Settings {
            if let Some(settings) = &mut self.settings { settings.draw(f, area, !self.tabs_focus); }
            return;
        }
        let mut area = area;
        if self.tab == Tab::Notes
            && let Some(q) = &self.filter
        {
            let [row, _, rest] = Layout::vertical([Constraint::Length(1), Constraint::Length(1), Constraint::Min(1)]).areas(area);
            let label = Style::default().fg(if self.filter_editing { theme::pal().accent } else { theme::pal().subtext0 });
            let count = format!("{} of {} ", self.rows.len(), self.notes.files.len());
            if self.filter_editing {
                f.render_widget(Paragraph::new(Span::styled("filter › ", label)).style(theme::panel()), row);
                self.filter_ta.render(f, filter_input(row, count.width()), true);
            } else {
                let line = Line::from(vec![Span::styled("filter › ", label), Span::styled(q.clone(), theme::text())]);
                f.render_widget(Paragraph::new(line).style(theme::panel()), row);
            }
            f.render_widget(Paragraph::new(Line::styled(count, theme::panel().fg(theme::pal().subtext0)).right_aligned()), row);
            area = rest;
        }
        if matches!(self.tab, Tab::Agents | Tab::New) {
            let [row, _, rest] = Layout::vertical([Constraint::Length(1), Constraint::Length(1), Constraint::Min(1)]).areas(area);
            let label = Style::default().fg(if self.afilter_editing { theme::pal().accent } else { theme::pal().subtext0 });
            let q = self.afilter.clone().unwrap_or_default();
            let shown: usize = self.avisible().iter().map(|(_, g)| g.len()).sum();
            let count = if q.is_empty() { String::new() } else { format!("{shown} of {} ", self.agents.len()) };
            if self.afilter_editing {
                f.render_widget(Paragraph::new(Span::styled("filter › ", label)).style(theme::panel()), row);
                self.afilter_ta.render(f, filter_input(row, count.width()), true);
            } else {
                let mut spans = vec![Span::styled("filter › ", label), Span::styled(q.clone(), theme::text())];
                if q.is_empty() {
                    spans.push(Span::styled("f: fuzzy search name, harness, model", theme::panel().fg(theme::pal().overlay0)));
                }
                f.render_widget(Paragraph::new(Line::from(spans)).style(theme::panel()), row);
            }
            if !count.is_empty() {
                f.render_widget(Paragraph::new(Line::styled(count, theme::panel().fg(theme::pal().subtext0)).right_aligned()), row);
            }
            area = rest;
        }
        let (list, preview, thin) = self.split(area);
        self.regions.list = list;
        let block = if thin {
            Block::new().borders(Borders::TOP).border_style(theme::line())
        } else {
            Block::new().borders(Borders::LEFT).border_style(theme::line())
        };
        let (title, lines, wrap) = self.preview_content(preview, thin);
        let block = block.title(Span::styled(format!(" {title} "), theme::muted()));
        let inner = block.inner(preview);
        let inner = Rect { x: inner.x + 1, width: inner.width.saturating_sub(1), ..inner };
        self.regions.preview = inner;
        f.render_widget(block, preview);
        let max = (lines.len() as u16).saturating_sub(inner.height);
        self.pscroll = self.pscroll.min(max);
        let mut p = Paragraph::new(lines).scroll((self.pscroll, 0));
        if wrap {
            p = p.wrap(Wrap { trim: false });
        }
        f.render_widget(p, inner);
        // the preview's selection: gone once the preview shows something else
        let key = format!("{:?} {} {} {} {} {} {:?} {inner:?}", self.tab, self.cursor, self.tcursor, self.tlist.cursor, self.acursor, self.pscroll, self.tlist.view);
        if key != self.psel_key {
            self.psel_key = key;
            self.psel.clear();
        }
        self.psel.paint(inner, f.buffer_mut());
        match self.tab {
            Tab::Notes => self.draw_notes(f, list),
            Tab::Threads => {
                let tick = (crate::engine::now_ms() as u128 / crate::nav::WAVE_MS) as usize;
                self.tlist.draw(f, list, tick);
                // worktrees / claude: this screen's rows in the list's place
                if !self.tlist.view.cards() {
                    let rows = self.tlist.list;
                    self.draw_threads(f, rows);
                    self.threads_fx.apply(f, rows, self.threads_version);
                }
            }
            Tab::Agents | Tab::New => self.draw_agents(f, list),
            Tab::Settings | Tab::Merge | Tab::Flows => {}
        }
    }

    fn preview_content(&mut self, area: Rect, thin: bool) -> (String, Vec<Line<'static>>, bool) {
        let width = area.width.saturating_sub(if thin { 1 } else { 3 }).max(20);
        if matches!(self.tab, Tab::Agents | Tab::New) {
            let Some(a) = self.current_agent().cloned() else {
                return ("agents".into(), vec![Line::styled("0 agents. n: new agent", theme::muted())], true);
            };
            return (a.name.clone(), self.agent_preview(&a), true);
        }
        if self.tab == Tab::Threads {
            if let Some(s) = self.current_session() {
                return (format!("claude · {}", s.place()), claude::preview(s), true);
            }
            let Some(w) = self.current_wt() else {
                let msg = if self.tlist.view.cards() { "0 threads" } else if self.cfg.repo.is_some() { "0 worktrees" } else { "not in a git repo: 0 worktrees" };
                return ("threads".into(), vec![Line::styled(msg, theme::muted())], true);
            };
            let key = w.key();
            let read = w.clone();
            // 0 dir (a card not read yet): the header only, 0 git in the start dir
            if !w.path.is_empty() {
                self.detail_reads.request(key.clone(), move || threads::preview(&read));
            }
            let lines = self.tpreview.get(&key).cloned().unwrap_or_else(|| threads::preview_header(&w));
            return (w.label(), lines, true);
        }
        let Some(row) = self.current().cloned() else {
            let lines = vec![
                Line::styled(format!("0 notes in {}", self.cfg.notes.display()), theme::muted()),
                Line::styled("add .md files there, or press enter to start without notes", theme::muted()),
            ];
            return ("notes".into(), lines, true);
        };
        if row.is_dir() {
            let files = self.notes.inside(row.path());
            let mut lines = vec![Line::from(vec![Span::styled(row.path().to_string(), theme::bold()), Span::styled(format!("  {} notes", files.len()), theme::muted())]), Line::raw("")];
            let mut files = files;
            files.sort_by_key(|f| std::cmp::Reverse(self.notes.mtime(f)));
            for p in files {
                lines.push(Line::from(vec![Span::raw(format!("  {}", &p[row.path().len()..])), Span::styled(format!("  {}", self.notes.age(&p)), theme::muted())]));
            }
            return (row.path().to_string(), lines, true);
        }
        let path = self.notes.abs(row.path());
        let lines = match self.previews.get(path, width) {
            Some(l) => l.clone(),
            None => vec![Line::styled("…", theme::muted())],
        };
        (row.path().to_string(), lines, false)
    }

    fn row_style(&self, hl: bool) -> Style {
        match (hl, self.tabs_focus || self.filter_editing || self.tlist.filtering) {
            (true, false) => theme::pill(),
            (true, true) => theme::panel(),
            _ => Style::default(),
        }
    }

    fn draw_notes(&mut self, f: &mut Frame, area: Rect) {
        let h = area.height as usize;
        if self.rows.is_empty() {
            let msg = if self.filter.is_some() { "0 matches" } else { "0 notes" };
            f.render_widget(Paragraph::new(Line::styled(msg, theme::muted())), area);
            return;
        }
        if self.cursor < self.offset {
            self.offset = self.cursor;
        } else if self.cursor >= self.offset + h {
            self.offset = self.cursor + 1 - h;
        }
        self.offset = self.offset.min(self.rows.len().saturating_sub(h));
        let sel: HashSet<&String> = self.selected.iter().collect();
        for (i, row) in self.rows.iter().enumerate().skip(self.offset).take(h) {
            let hl = i == self.cursor;
            let on_pill = hl && !(self.tabs_focus || self.filter_editing);
            let style = self.row_style(hl);
            let fg = |s: Style| if on_pill { s.fg(theme::pal().on_accent) } else { s };
            let muted = fg(theme::muted());
            let mark = |on: bool| {
                if on {
                    Span::styled("◼ ", fg(theme::accent()))
                } else {
                    Span::styled("◻ ", fg(theme::line()))
                }
            };
            let mut spans = vec![Span::raw(" ")];
            match row {
                Row::Dir { path, name, depth, count, collapsed } => {
                    let n_sel = self.selected.iter().filter(|s| s.starts_with(path.as_str())).count();
                    spans.push(Span::raw("  ".repeat(*depth)));
                    spans.push(Span::styled(if *collapsed { "▸ " } else { "▾ " }, muted));
                    spans.push(Span::styled(format!("{name}/"), fg(theme::bold())));
                    spans.push(Span::styled(format!(" {count}"), muted));
                    if n_sel > 0 {
                        spans.push(Span::raw(" "));
                        spans.push(Span::styled(format!(" ◼ {n_sel} "), theme::badge(on_pill)));
                    }
                }
                Row::File { path, name, depth } => {
                    spans.push(Span::raw("  ".repeat(*depth)));
                    spans.push(mark(sel.contains(path)));
                    spans.push(Span::styled(name.clone(), fg(theme::text())));
                    spans.push(Span::styled(format!(" {}", self.notes.age(path)), muted));
                }
                Row::Hit { path, snippet } => {
                    spans.push(mark(sel.contains(path)));
                    spans.push(Span::styled(path.clone(), fg(theme::text())));
                    let extra = if snippet.is_empty() { self.notes.age(path) } else { snippet.clone() };
                    spans.push(Span::styled(format!("  {extra}"), muted));
                }
            }
            let r = Rect { y: area.y + (i - self.offset) as u16, height: 1, ..area };
            f.render_widget(Paragraph::new(Line::from(spans)).style(style), r);
        }
    }
    /// The rows of the views worktrees (0 thread) / claude (sessions) in AREA.
    fn draw_threads(&mut self, f: &mut Frame, area: Rect) {
        let h = area.height as usize;
        let Some(items) = &self.worktrees else { return };
        let vis = self.tvisible();
        if vis.is_empty() {
            let msg = if !self.tlist.query().trim().is_empty() {
                "0 matches"
            } else if self.tlist.view == crate::threadlist::View::Claude {
                "0 Claude Code sessions"
            } else {
                "0 worktrees without thread"
            };
            f.render_widget(Paragraph::new(Line::styled(msg, theme::muted())), area);
            return;
        }
        let hint = if self.tlist.view == crate::threadlist::View::Claude { "enter: new thread, last response as context · c: context" } else { "enter: new task there · d: delete worktree" };
        f.render_widget(Paragraph::new(Line::styled(format!(" {hint}"), theme::muted())), Rect { height: 1, ..area });
        let area = Rect { y: area.y + 1, height: area.height.saturating_sub(1), ..area };
        let h = h.saturating_sub(1);
        self.tcursor = self.tcursor.min(vis.len() - 1);
        if self.tcursor < self.toffset {
            self.toffset = self.tcursor;
        } else if self.tcursor >= self.toffset + h {
            self.toffset = self.tcursor + 1 - h;
        }
        self.toffset = self.toffset.min(vis.len().saturating_sub(h));
        let nw = items.len();
        let dim = self.tabs_focus || self.tlist.focus != crate::threadlist::Focus::List || self.tlist.modal();
        for (r, &i) in vis.iter().enumerate().skip(self.toffset).take(h) {
            let rect = Rect { y: area.y + (r - self.toffset) as u16, height: 1, ..area };
            let hl = r == self.tcursor;
            let on_pill = hl && !dim;
            let style = self.row_style(hl && !dim);
            let fg = |s: Style| if on_pill { s.fg(theme::pal().on_accent) } else { s };
            let muted = fg(theme::muted());
            let spans = if i >= nw {
                let c = &self.sessions[i - nw];
                let ctx = self.in_context(&c.key());
                vec![
                    Span::raw(" "),
                    if ctx { Span::styled("↳ ", fg(theme::accent())) } else { Span::styled("› ", fg(theme::line())) },
                    Span::styled(c.place(), fg(theme::bold())),
                    Span::raw("  "),
                    Span::styled(c.title.chars().take(60).collect::<String>(), fg(theme::text())),
                    Span::styled(format!(" {}", c.when), muted),
                ]
            } else {
                let w = &items[i];
                let mut spans = vec![Span::raw(" "), Span::styled("› ", fg(theme::line())), Span::styled(w.branch.clone(), fg(theme::bold())), Span::styled(format!("  {}", w.path), muted)];
                if w.unpushed > 0 {
                    spans.push(Span::styled(format!(" {} unpushed", w.unpushed), fg(theme::fg(theme::pal().attention()))));
                }
                spans.push(Span::styled(format!(" {}", w.when), muted));
                spans
            };
            f.render_widget(Paragraph::new(Line::from(spans)).style(style), rect);
        }
    }

    fn agent_preview(&mut self, a: &Agent) -> Vec<Line<'static>> {
        let name = a.name.clone();
        self.extra_reads.request(name.clone(), move || agents::show_extra(&name));
        let extra = self.apreview.get(&a.name).cloned().unwrap_or_default();
        let kv = |k: &str, v: String| Line::from(vec![Span::styled(format!("{k:<9}"), theme::muted()), Span::raw(v)]);
        let side = vec![vec![], vec![Span::styled(format!("   {}", a.name), theme::accent_bold())], vec![Span::styled(format!("   {}", a.harness_model()), theme::muted())]];
        let sprite = agents::sprite(a);
        let mut out = sprites::lines(&sprite, agents::color(a.color.as_deref()), side);
        out.extend([
            Line::raw(""),
            kv("harness", a.harness_model()),
            kv("model", agents::model_of(a)),
            kv("effort", format!("{} (global)", self.effort)),
            kv("mode", a.mode.clone()),
            kv("color", a.color.clone().unwrap_or_else(|| "-".into())),
            kv("style", {
                let p = agents::parse_profile(a.prompt.as_deref(), None);
                match p.style.as_deref().and_then(|id| self.styles.iter().find(|s| s.id == id)) {
                    Some(s) => s.name.clone(),
                    None if a.prompt.as_deref().is_some_and(agents::written_by_us) => "none".into(),
                    None => format!("{} (global)", self.global_style.clone().unwrap_or_else(|| "-".into())),
                }
            }),
            kv(
                "seen",
                a.last_seen.map_or("never".into(), |t| crate::notes::ago(std::time::UNIX_EPOCH + Duration::from_secs(t.max(0) as u64))),
            ),
        ]);
        if a.name == self.cfg.agent {
            out.push(Line::styled("default agent: takes over threads of deleted agents", theme::accent()));
        }
        let p = agents::parse_profile(a.prompt.as_deref(), None);
        for (head, body) in [("voice", &p.voice), ("system prompt", &p.prompt)] {
            out.push(Line::raw(""));
            out.push(Line::styled(head.to_string(), theme::bold()));
            if body.is_empty() {
                out.push(Line::styled("-", theme::muted()));
            }
            out.extend(body.lines().map(|l| Line::raw(format!("  {l}"))));
        }
        out.push(Line::raw(""));
        out.extend(extra.into_iter().map(|l| Line::styled(l, theme::muted())));
        let uses = self.uses(a);
        out.push(Line::raw(""));
        out.push(Line::styled(format!("threads: {}", uses.len()), theme::bold()));
        for u in uses.iter().take(30) {
            let live = if u.live.is_empty() { Span::raw("") } else { Span::styled(" live", theme::bold().fg(theme::pal().progress())) };
            out.push(Line::from(vec![Span::raw(format!("  {}", u.title)), live, Span::styled(format!(" {}", u.state), theme::muted())]));
        }
        out
    }

    /// Shared by the agents tab and task selection.
    fn agent_grid(&self, width: u16) -> crate::cards::Grid {
        let text_w = crate::cards::text_width(self.agents.iter().flat_map(|a| self.agent_card_text(a)).collect::<Vec<_>>().iter());
        crate::cards::tight_grid(&self.avisible(), crate::cards::card_width(text_w, true), CARD_H, width)
    }

    /// 1 card grid per harness (A→Z), as many columns as fit.
    fn draw_agents(&mut self, f: &mut Frame, area: Rect) {
        self.draw_agent_grid(f, area, false);
    }

    fn draw_agent_grid(&mut self, f: &mut Frame, area: Rect, task: bool) {
        let g = self.agent_grid(area.width);
        let mut cursor = if task { self.pick_cursor } else { self.acursor };
        let mut offset = if task { self.pick_offset } else { self.aoffset };
        if !g.places.iter().any(|&(i, _, _)| i == cursor) {
            cursor = g.places.first().map_or(0, |p| p.0);
        }
        if task { self.pick_cursor = cursor; } else { self.acursor = cursor; }
        if let Some(&(_, _, top)) = g.places.iter().find(|&&(i, _, _)| i == cursor) {
            offset = crate::cards::scroll(&g, top, CARD_H, offset, area.height as usize);
        }
        offset = offset.min(g.height.saturating_sub(area.height as usize));
        if task { self.pick_offset = offset; } else { self.aoffset = offset; }
        let focused = !self.tabs_focus && if task { self.field == Field::Agents } else { !self.afilter_editing };
        let cards = crate::cards::draw_grid(f, area, &g, CARD_H, offset, |f, r, i| {
            let a = &self.agents[i];
            if task { self.draw_card(f, r, a, focused && i == cursor); } else { self.draw_agent_card(f, r, a, focused && i == cursor); }
        });
        let places = g.places;
        if places.is_empty() {
            let msg = if self.agents.is_empty() { &format!("0 agents. {} → n: new agent", crate::config::menu_key().label) } else { "0 agents match the filter" };
            f.render_widget(Paragraph::new(Line::styled(msg, theme::muted())), area);
        }
        if task {
            self.pick_places = places;
            self.regions.menu_rows = cards;
        } else {
            self.aplace = places;
            self.acards = cards;
        }
    }

    /// Agents tab card rows: name, harness · model, effort, threads (+ live).
    fn agent_card_text(&self, a: &Agent) -> [String; 4] {
        let [name, tech, effort] = self.card_text(a);
        let uses = self.usage.as_ref().and_then(|u| u.get(&a.name)).map(Vec::as_slice).unwrap_or(&[]);
        let live = uses.iter().filter(|u| !u.live.is_empty()).count();
        let threads = if live > 0 { format!("{} threads · {live} live", uses.len()) } else { format!("{} threads", uses.len()) };
        [name, tech, effort, threads]
    }

    /// Agents tab card: like a task card, 0 pick mark; the default agent named in the border.
    fn draw_agent_card(&self, f: &mut Frame, r: Rect, a: &Agent, cur: bool) {
        let title = (a.name == self.cfg.agent).then(|| Span::styled(" default ", theme::fg(theme::pal().info())));
        let look = crate::cards::Look { cur, lit: false, border: Some(crate::cards::border(cur, false, theme::pal().accent)), title, bg: None };
        crate::cards::draw(f, r, a, &self.agent_card_text(a), look);
    }

    fn draw_compose(&mut self, f: &mut Frame, area: Rect) {
        let label = |text: &str, focused: bool| Line::styled(text.to_string(), if focused { theme::accent_bold() } else { theme::muted() });
        let title_rows = (self.title.rows_for(area.width.saturating_sub(2)) as u16).min(3);
        let n_notes = self.selected.len();
        let note_rows = n_notes.min(5) as u16 + u16::from(n_notes > 5) + self.carry.len().min(5) as u16;
        // the opts boxes below the where row: 1 row each
        let opts_h = if self.wheres.is_empty() { 0 } else { 2 };
        // "where": a label, the repo row, the branch row (a git repo only)
        let branch_h = u16::from(!self.wheres.is_empty());
        // the drafts row: "view drafts (n)" (1+ drafts), "save as draft"; a blank row below
        let drafts_h = 2;
        let available = area.height.saturating_sub(drafts_h + title_rows + opts_h + note_rows + branch_h + 15);
        let agents_h = if self.agents.is_empty() { 1 } else {
            let g = self.agent_grid(area.width);
            let view = g.height.min(2 * (CARD_H as usize + 1)).min(available.max(CARD_H + 1) as usize);
            // only the rows the cards fill: the rest goes to the prompt
            match g.places.iter().find(|&&(i, _, _)| i == self.pick_cursor).or(g.places.first()) {
                Some(&(_, _, top)) => {
                    let off = crate::cards::scroll(&g, top, CARD_H, self.pick_offset, view);
                    crate::cards::used(&g, CARD_H, off, view).max(1) as u16
                }
                None => view as u16,
            }
        };
        // prompt first and largest (the rest of the height), then title, where (repo, branch,
        // opts), agents, notes, a
        // blank row below the notes (above the footer)
        let [r_drafts, l_prompt, i_prompt, _, l_title, i_title, _, l_where, repo_row, info, opts, _, l_agents, r_agents, _, l_notes, notes, _] = Layout::vertical([
            Constraint::Length(drafts_h),
            Constraint::Length(1),
            Constraint::Min(3),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(title_rows),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(branch_h),
            Constraint::Length(opts_h),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(agents_h),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(note_rows),
            Constraint::Length(1),
        ])
        .areas(area);
        {
            let on = self.field == Field::Drafts;
            let btn = self.draft_btn();
            let st = |cur: bool| if cur { theme::pill().add_modifier(Modifier::BOLD) } else { theme::pill_dim() };
            let mut x = r_drafts.x;
            self.regions.drafts = Rect::default();
            if !self.drafts.is_empty() {
                let text = format!(" view drafts ({}) ", self.drafts.len());
                let r = Rect { x, width: (text.width() as u16).min(r_drafts.width), height: 1, ..r_drafts };
                f.render_widget(Paragraph::new(text).style(st(on && btn == 0)), r);
                self.regions.drafts = r;
                x = r.right() + 2;
            }
            let text = " save as draft ";
            let r = Rect { x, width: (text.width() as u16).min(r_drafts.right().saturating_sub(x)), height: 1, ..r_drafts };
            f.render_widget(Paragraph::new(text).style(st(on && btn == 1)), r);
            self.regions.save_draft = r;
            if on && self.drafts_menu.is_none() {
                let hint = if btn == 0 { "enter: list of unsent tasks" } else { "enter: keep the task for later, start over" };
                let hx = r.right() + 2;
                let h = Rect { x: hx, width: r_drafts.right().saturating_sub(hx), height: 1, ..r_drafts };
                f.render_widget(Paragraph::new(Line::styled(hint, theme::muted())), h);
            }
        }
        f.render_widget(Paragraph::new(label(if self.title.is_empty() { "title · optional: the prompt's 1st line" } else { "title" }, self.field == Field::Title)), l_title);
        f.render_widget(Paragraph::new("").style(theme::panel()), i_title);
        self.title.render(f, Rect { x: i_title.x + 1, width: i_title.width.saturating_sub(2), ..i_title }, self.field == Field::Title);
        // where: a label, then the repo row and the branch row (each: a chip, enter: its picker
        // ▾, what it does), then the opts boxes; 1 column in from the label, like the title input
        let any = matches!(self.field, Field::Repo | Field::Where | Field::Opts);
        f.render_widget(Paragraph::new(label("where", any)), l_where);
        let chip = Style::default().fg(theme::pal().text).add_modifier(Modifier::BOLD);
        // a row: the sub-label, the chip (its area back), the text
        let row = |f: &mut Frame, r: Rect, name: &str, on: bool, value: String, text: String| -> Rect {
            let head = format!(" {name:<7}");
            let x = r.x + head.width() as u16;
            let value = format!(" {value} ");
            let area = Rect { x: x.min(r.right().saturating_sub(1)), y: r.y, width: value.width() as u16, height: 1 };
            let spans = vec![
                Span::styled(head, if on { theme::accent_bold() } else { theme::muted() }),
                Span::styled(value, if on { chip.bg(theme::pal().surface0) } else { chip }),
                Span::styled(if on { "▾" } else { " " }, theme::accent()),
                Span::raw("  "),
                Span::styled(text, theme::muted()),
            ];
            f.render_widget(Paragraph::new(Line::from(spans)), r);
            area
        };
        let (repo_value, repo_text) = match &self.repo {
            Some(r) if self.clone_first().is_some() => (r.name.clone(), format!("{} · 0 clone yet", tilde(&r.path.display().to_string()))),
            Some(r) => (r.name.clone(), tilde(&r.path.display().to_string())),
            None => ("0 git repo".into(), format!("works in {} · enter: pick a repo", tilde(&self.cfg.cwd.display().to_string()))),
        };
        self.regions.repo_chip = row(f, repo_row, "repo", self.field == Field::Repo, repo_value, repo_text);
        self.regions.repo_row = repo_row;
        if !self.wheres.is_empty() {
            self.regions.where_chip = row(f, info, "branch", self.field == Field::Where, self.where_chip(), self.where_text());
            self.regions.where_row = info;
        }
        // opts: ◼ checkout, ◻ stash below it, in the column of the sub-labels
        if opts_h > 0 {
            let on = self.field == Field::Opts;
            let x = opts.x;
            for (i, (checked, enabled), label) in [(0, self.checkout_box(), "checkout selected branch"), (1, self.stash_box(), "stash conflicting local changes (git stash)")] {
                if i as u16 >= opts.height {
                    break;
                }
                let text = format!(" {} {label} ", if checked { "◼" } else { "◻" });
                let w = (text.width() as u16).min((opts.x + opts.width).saturating_sub(x));
                // the box under the cursor: accent, like the other fields (disabled: dim on accent)
                let style = match (enabled, on && self.opt_i == i) {
                    (true, true) => theme::pill().add_modifier(Modifier::BOLD),
                    (false, true) => theme::pill().add_modifier(Modifier::DIM),
                    (false, false) => Style::default().fg(theme::pal().overlay0),
                    (true, false) if checked => Style::default().fg(theme::pal().text),
                    (true, false) => theme::muted(),
                };
                let r = Rect { x, y: opts.y + i as u16, width: w, height: 1 };
                f.render_widget(Paragraph::new(text).style(style), r);
                self.regions.opts.push((r, i));
            }
        }
        let focused = self.field == Field::Agents;
        let agents_label = match self.picks.len() {
            0 => "agents · 0 picked".to_string(),
            1 => "agents · 1 picked".to_string(),
            n => format!("agents · {n} picked: 1st starts the thread, the others join"),
        };
        f.render_widget(Paragraph::new(label(&agents_label, focused)), l_agents);
        self.regions.task_agents = r_agents;
        self.draw_agent_grid(f, r_agents, true);
        let focused = self.field == Field::Notes;
        let mut notes_label = if n_notes == 0 { "notes · 0 picked".to_string() } else { format!("notes · {n_notes} picked, listed above the prompt") };
        if !self.carry.is_empty() {
            notes_label.push_str(&format!(" · {} context (threads tab: c)", self.carry.len()));
        }
        if focused {
            notes_label.push_str(" · enter: pick · x: drop all");
        }
        f.render_widget(Paragraph::new(label(&notes_label, focused)), l_notes);
        self.regions.notes_label = l_notes;
        let mut lines: Vec<Line> = self.selected.iter().take(5).map(|p| Line::from(vec![Span::styled("  ◼ ", theme::accent()), Span::raw(p.clone())])).collect();
        if n_notes > 5 {
            lines.push(Line::styled(format!("  … {} more", n_notes - 5), theme::muted()));
        }
        for c in self.carry.iter().take(5) {
            lines.push(Line::from(vec![
                Span::styled(if c.thread.is_some() { "  ↳ thread " } else { "  ↳ claude code " }, theme::accent()),
                Span::raw(c.title.clone()),
                Span::styled(format!(" · last response, {} lines", c.last.lines().count()), theme::muted()),
            ]));
        }
        f.render_widget(Paragraph::new(lines), notes);
        f.render_widget(Paragraph::new(label("prompt", self.field == Field::Prompt)), l_prompt);
        f.render_widget(Paragraph::new("").style(theme::panel()), i_prompt);
        let inner = Rect { x: i_prompt.x + 1, width: i_prompt.width.saturating_sub(2), ..i_prompt };
        self.prompt.marks = self.agent_names();
        self.prompt.render(f, inner, self.field == Field::Prompt);
    }

    /// Text rows of an agent card: name, harness · model, effort.
    fn card_text(&self, a: &Agent) -> [String; 3] {
        let tech = crate::cards::tech(a);
        let effort = crate::cards::effort(a, &self.effort);
        [a.name.clone(), tech, effort]
    }

    /// Agent card: box, sprite left, text right. Picked: border in the agent color, ◼ (+ order).
    /// Cursor: accent border.
    fn draw_card(&self, f: &mut Frame, r: Rect, a: &Agent, cur: bool) {
        let color = agents::color(a.color.as_deref());
        let order = self.picks.iter().position(|p| *p == a.name);
        let mark = match order {
            Some(n) if self.picks.len() > 1 => format!(" ◼ {} ", n + 1),
            Some(_) => " ◼ ".to_string(),
            None => " ◻ ".to_string(),
        };
        let title = Span::styled(mark, if order.is_some() { Style::default().fg(color).add_modifier(Modifier::BOLD) } else { theme::muted() });
        let look = crate::cards::Look { cur, lit: order.is_some(), border: Some(crate::cards::border(cur, order.is_some(), color)), title: Some(title), bg: None };
        crate::cards::draw(f, r, a, &self.task_card_text(a), look);
    }

    /// New-thread card rows: name, harness · model, effort, the agent's flow.
    fn task_card_text(&self, a: &Agent) -> [String; 4] {
        let [name, tech, effort] = self.card_text(a);
        let (slug, start) = self.flow_of(&a.name);
        let flow = match self.flow_lib.flow(&slug).filter(|_| !slug.is_empty()) {
            Some(fl) if start > 0 => format!("◆ {} › {}", fl.name, start + 1),
            Some(fl) => format!("◆ {}", fl.name),
            None => "0 flow".into(),
        };
        [name, tech, effort, flow]
    }

    fn draw_footer(&mut self, f: &mut Frame, area: Rect) {
        let mut spans = vec![Span::raw(" ")];
        // a filter takes the letters: its mode badge first
        if self.screen == Screen::Pick && (self.filter_editing || self.tlist.filtering || self.afilter_editing) {
            spans.extend(theme::mode_badge("filter", false));
        }
        let cmd = crate::config::command_key().to_string();
        if !self.tabs_focus && self.screen == Screen::Pick && self.tab == Tab::Merge {
            spans.extend(theme::hints(&self.merge_tab.hints()));
        }
        let hints: &[(&str, &str)] = match self.screen {
            _ if !self.tabs_focus && self.screen == Screen::Pick && self.tab == Tab::Merge => &[],
            _ if self.tabs_focus => &[("←→", "tab"), ("↓ / esc", "back"), ("q", "quit")],
            Screen::Compose if self.field == Field::Agents => &[("↑↓←→", "move"), ("space", "pick"), ("f", "flow"), ("enter", "notes"), ("⌘/ctrl+⏎", "start"), ("tab", "next field")],
            Screen::Compose if self.where_menu.is_some() => &[("↑↓", "move"), ("type", "filter"), ("enter", "pick"), ("esc", "close")],
            Screen::Compose if self.flow_menu.as_ref().is_some_and(|m| m.sub.is_some()) => &[("↑↓", "step"), ("enter", "start here"), ("← esc", "flows")],
            Screen::Compose if self.flow_menu.is_some() => &[("↑↓", "flow"), ("enter →", "its steps"), ("a-z", "jump"), ("0", "0 flow"), ("esc", "close")],
            Screen::Compose if self.repo_pick.is_some() => &[("↑↓", "dir"), ("←→", "out · in"), ("type", "filter"), ("~", "home"), ("enter", "pick"), ("esc", "close")],
            Screen::Compose if self.field == Field::Repo => &[("enter", "pick repo"), ("↑↓", "field"), ("⌘/ctrl+⏎", "start")],
            Screen::Compose if self.field == Field::Where => &[("enter", "choices"), ("↑↓", "field"), ("⌘/ctrl+⏎", "start")],
            Screen::Compose if self.field == Field::Opts => &[("space", "check / uncheck"), ("↑↓", "box · field"), ("⌘/ctrl+⏎", "start")],
            Screen::Compose if self.drafts_menu.is_some() => &[("↑↓", "move"), ("enter", "load"), ("d", "delete"), ("esc", "back")],
            Screen::Compose if self.field == Field::Drafts => &[("←→", "button"), ("enter", "press"), ("↓", "prompt"), ("⌘/ctrl+⏎", "start"), ("ctrl+s", "save draft"), ("F1-F6", "tabs")],
            Screen::Compose if self.field == Field::Prompt => &[("@", "agents"), (cmd.as_str(), "commands"), ("dir/", "paths"), ("ctrl+v", "image"), ("↑↓", "field"), ("⌘/ctrl+⏎", "start"), ("esc esc", "quit")],
            Screen::Compose if self.field == Field::Notes => &[("enter", "pick notes"), ("x", "drop"), ("↑↓", "field"), ("⌘/ctrl+⏎", "start"), ("tab", "next field")],
            Screen::Compose => &[("↑↓", "field"), ("⌘/ctrl+⏎", "start"), ("ctrl+s", "save draft"), ("tab", "next field"), ("F1-F6", "tabs"), ("esc esc", "quit")],
            Screen::Pick if self.tlist.filtering => &[("type", "filter"), ("↑↓", "move"), ("enter / esc", "back to list")],
            Screen::Pick if self.filter_editing => &[("type", "filter"), ("↑↓", "move"), ("enter", "done"), ("esc", "clear")],
            Screen::Pick if self.tab == Tab::Notes && self.cfg.copy_mode => &[("↑↓", "move"), ("←→", "fold"), ("space", "select"), ("enter", "copy"), ("esc", "quit")],
            Screen::Pick if self.tab == Tab::Notes => &[("↑↓", "move"), ("←→", "fold"), ("space", "select"), ("enter", "confirm"), ("esc", "new"), ("tab", "back")],
            Screen::Pick if self.tab == Tab::Threads => &[("↑↓", "move"), ("space", "select"), ("a", "archive"), ("e", "edit"), ("ctrl+o", "actions"), ("enter", "open"), ("ctrl+f", "filter")],
            Screen::Pick if self.tab == Tab::Settings => self.settings.as_ref().map_or(&[], |s| s.help()),
            Screen::Pick if self.tab == Tab::Flows => self.flows_tab.help(),
            Screen::Pick if self.afilter_editing => &[("type", "filter"), ("↑↓", "move"), ("enter / esc", "back to cards")],
            Screen::Pick => &[("↑↓←→", "move"), ("ctrl+f", "filter"), ("enter", "edit"), ("F1-F6", "tabs")],
        };
        for (k, l) in hints {
            spans.extend(hint(k, l));
        }
        f.render_widget(Paragraph::new(Line::from(spans)), area);
        let mut right = Vec::new();
        if let Some((msg, _, err)) = &self.toast {
            right.push(Span::styled(format!("{msg}   "), Style::default().fg(if *err { theme::pal().error() } else { theme::pal().success() })));
        }
        right.push(Span::styled(crate::config::menu_key().label, theme::accent()));
        right.push(Span::styled(" menu ", theme::muted()));
        f.render_widget(Paragraph::new(Line::from(right).right_aligned()), area);
    }

    fn frame(f: &mut Frame, area: Rect, title: &str, w: u16, h: u16) -> Rect {
        let w = w.min(area.width.saturating_sub(2));
        let h = h.min(area.height.saturating_sub(2));
        let r = Rect { x: area.x + (area.width - w) / 2, y: area.y + (area.height - h) / 2, width: w, height: h };
        // a modal: the screen behind it fades
        theme::dim(f.buffer_mut(), area);
        f.render_widget(Clear, r);
        let block = Block::new()
            .borders(Borders::ALL)
            .border_type(BorderType::Plain)
            .border_style(theme::accent())
            .title(Span::styled(format!(" {title} "), theme::accent_bold()))
            .style(theme::text());
        let inner = block.inner(r);
        f.render_widget(block, r);
        Rect { x: inner.x + 1, width: inner.width.saturating_sub(2), ..inner }
    }

    fn draw_modal(&mut self, f: &mut Frame, area: Rect) {
        let Some(modal) = &self.modal else { return };
        match modal {
            Modal::Confirm { title, question, yes, .. } => {
                let w = (question.width() as u16 + 8).clamp(44, 90);
                let inner = Self::frame(f, area, title, w, 7);
                let [_, q, _, b] = Layout::vertical([Constraint::Length(1), Constraint::Length(1), Constraint::Length(1), Constraint::Length(1)]).areas(inner);
                f.render_widget(Paragraph::new(Line::styled(question.clone(), theme::text()).centered()), q);
                let labels = [(true, "  yes (y)  "), (false, "  no (n/esc)  ")];
                let total: u16 = labels.iter().map(|(_, l)| l.width() as u16).sum::<u16>() + 2;
                let mut x = b.x + b.width.saturating_sub(total) / 2;
                let mut buttons = Vec::new();
                for (value, l) in labels {
                    let focused = value == *yes;
                    let style = if focused {
                        theme::pill().add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().bg(theme::pal().surface0).fg(theme::pal().text)
                    };
                    let r = Rect { x, y: b.y, width: l.width() as u16, height: 1 };
                    f.render_widget(Paragraph::new(l).style(style), r);
                    buttons.push((r, value));
                    x += r.width + 2;
                }
                self.regions.buttons = buttons;
            }
            Modal::Agent(_) => {
                self.draw_agent_form(f, area);
                self.draw_form_pick(f, area);
            }
            Modal::Info { title, lines, scroll } => {
                let w = lines.iter().map(|l| l.width()).max().unwrap_or(20) as u16 + 6;
                let h = lines.len() as u16 + 4;
                let inner = Self::frame(f, area, title, w.clamp(40, 120), h);
                let [body, foot] = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(inner);
                f.render_widget(Paragraph::new(lines.clone()).scroll((*scroll, 0)), body);
                f.render_widget(Paragraph::new(Line::from(hint("enter", "close"))), foot);
            }
        }
    }
}

impl App {
    fn draw_agent_form(&mut self, f: &mut Frame, area: Rect) {
        let global = self.global_style.clone();
        let Some(Modal::Agent(form)) = &mut self.modal else { return };
        let title = if form.edit.is_some() { "edit agent" } else { "new agent" };
        let inner = Self::frame(f, area, title, 90, area.height.saturating_sub(2).min(42));
        // 0-6 single-line fields, 7 note, 8 MCP servers, 10 sprite set, 11 sprite, 13-14 voice,
        // 16-17 system prompt, 19 error, 20 buttons
        let rows = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(sprites::H as u16),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(5),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(3),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(inner);
        let lab = |t: &str, on: bool| Span::styled(format!("{t:<10}"), if on { theme::accent_bold() } else { theme::muted() });
        // single-line rows: label left, value right
        for (i, name) in ["name", "harness", "model", "effort", "color", "mode", "style"].iter().enumerate() {
            let on = form.field == i;
            let row = rows[i];
            f.render_widget(Paragraph::new(Line::from(lab(name, on))), row);
            let vr = Rect { x: row.x + 10, width: row.width.saturating_sub(10), ..row };
            form.areas[i] = row;
            match i {
                F_MODEL if !form.is_claude() => {
                    let m = form.model_id();
                    let st = if on { theme::bold().bg(theme::pal().surface0) } else { theme::bold() };
                    let line = Line::from(vec![Span::styled(if on { "▾" } else { " " }, theme::accent()), Span::styled(format!(" {m} "), st), Span::styled(format!("  {} models: enter", form.harness_name()), theme::muted())]);
                    f.render_widget(Paragraph::new(line), vr);
                }
                F_NAME | F_HARNESS => {
                    f.render_widget(Paragraph::new("").style(theme::panel()), vr);
                    let ta = if i == 0 { &mut form.name } else { &mut form.harness };
                    ta.render(f, Rect { x: vr.x + 1, width: vr.width.saturating_sub(2), ..vr }, on);
                }
                _ => {
                    let labels: Vec<(String, Style)> = match i {
                        F_MODEL => form.models.iter().map(|m| (m.clone(), theme::text())).collect(),
                        F_EFFORT => agents::EFFORTS.iter().map(|e| (e.to_string(), theme::text())).collect(),
                        F_COLOR => agents::COLORS.iter().map(|c| (c.to_string(), Style::default().fg(agents::color(Some(c))))).collect(),
                        F_MODE => agents::MODES.iter().map(|m| (m.to_string(), theme::text())).collect(),
                        _ => form.styles.iter().map(|(_, n)| (n.clone(), theme::text())).collect(),
                    };
                    let cur = [form.model, form.effort, form.color, form.mode, form.style][i - F_MODEL];
                    let mut spans = vec![Span::styled(if on { "‹" } else { " " }, theme::accent())];
                    for (j, (l, st)) in labels.into_iter().enumerate() {
                        let st = if j == cur {
                            let st = st.add_modifier(Modifier::BOLD);
                            if on { st.bg(theme::pal().surface0) } else { st }
                        } else {
                            theme::muted()
                        };
                        spans.push(Span::styled(format!(" {l} "), st));
                    }
                    spans.push(Span::styled(if on { "›" } else { "" }, theme::accent()));
                    f.render_widget(Paragraph::new(Line::from(spans)), vr);
                }
            }
        }
        let note = rows[7];
        let note_text = if form.field == F_EFFORT {
            "effort: global, ~/.claude/settings.json effortLevel · every Claude agent + Claude Code".to_string()
        } else if form.field == F_HARNESS {
            "enter: installed harnesses · or type harness/model".to_string()
        } else if form.field == F_MODEL {
            format!("enter: the models of {}{}", form.harness_name(), if form.is_claude() { " · ←→ cycle" } else { "" })
        } else {
            form.style_note(global.as_deref())
        };
        f.render_widget(Paragraph::new(Line::styled(note_text, theme::muted())), Rect { x: note.x + 11, width: note.width.saturating_sub(11), ..note });
        // MCP servers: codehead's, ←→ pick, space on / off
        let on = form.field == F_MCP;
        let mr = rows[8];
        form.areas[F_MCP] = mr;
        let mut spans = vec![lab("mcp", on), Span::styled(if on { "‹" } else { " " }, theme::accent())];
        if form.mcp.is_empty() {
            spans.push(Span::styled(" 0 servers · settings › mcp servers: add one", theme::muted()));
        }
        for (j, (name, sel)) in form.mcp.iter().enumerate() {
            let st = match (on && j == form.mcp_cur, *sel) {
                (true, _) => theme::bold().bg(theme::pal().surface0),
                (false, true) => theme::bold(),
                _ => theme::muted(),
            };
            spans.push(Span::styled(format!(" {} {name} ", if *sel { "☑" } else { "☐" }), st));
        }
        spans.push(Span::styled(if on { "›  space: on / off" } else { "" }, theme::accent()));
        f.render_widget(Paragraph::new(Line::from(spans)), mr);
        // avatar: the category, then the live avatar; enter opens the picker
        let on = form.field == F_SET;
        let tr = rows[10];
        form.areas[F_SET] = tr;
        let cat = form.sprite.avatar.cat;
        let mut spans = vec![lab("avatars", on), Span::raw(" ")];
        for c in crate::avatar::CATS {
            let st = if c == cat { if on { theme::bold().bg(theme::pal().surface0) } else { theme::bold() } } else { theme::muted() };
            spans.push(Span::styled(format!(" {} ", c.name()), st));
        }
        f.render_widget(Paragraph::new(Line::from(spans)), tr);
        let on = form.field == F_SPRITE;
        let sr = rows[11];
        form.areas[F_SPRITE] = sr;
        f.render_widget(Paragraph::new(Line::from(lab("avatar", on))), sr);
        let color = agents::color(Some(agents::COLORS[form.color]));
        let a = form.sprite.avatar;
        let side = vec![
            vec![Span::styled(format!("   {}", a.name()), if on { theme::bold() } else { theme::muted() })],
            vec![Span::styled(format!("   {} {}/{} · float {}x", cat.name(), a.idx + 1, cat.count(), a.speed), theme::muted())],
            vec![],
            vec![Span::styled("   enter: pick · r random", if on || form.field == F_SET { theme::muted() } else { Style::default().fg(theme::pal().overlay0) })],
        ];
        f.render_widget(Paragraph::new(sprites::lines(&form.sprite, color, side)), Rect { x: sr.x + 11, width: sr.width.saturating_sub(11), ..sr });
        for (field, label_row, box_row, head) in [(F_VOICE, rows[13], rows[14], "voice · own tone on top of the style"), (F_PROMPT, rows[16], rows[17], "system prompt · standing instructions")] {
            let on = form.field == field;
            f.render_widget(Paragraph::new(Line::styled(head, if on { theme::accent_bold() } else { theme::muted() })), label_row);
            f.render_widget(Paragraph::new("").style(theme::panel()), box_row);
            form.areas[field] = box_row;
            let ta = if field == F_VOICE { &mut form.voice } else { &mut form.prompt };
            ta.render(f, Rect { x: box_row.x + 1, width: box_row.width.saturating_sub(2), ..box_row }, on);
        }
        if let Some(e) = &form.error {
            f.render_widget(Paragraph::new(Line::styled(e.clone(), Style::default().fg(theme::pal().error()))), rows[19]);
        }
        let b = rows[20];
        let labels = [(true, if form.edit.is_some() { "  save (ctrl+s)  " } else { "  create (ctrl+s)  " }), (false, "  cancel (esc)  ")];
        let total: u16 = labels.iter().map(|(_, l)| l.width() as u16).sum::<u16>() + 2;
        let mut x = b.x + b.width.saturating_sub(total) / 2;
        let mut buttons = Vec::new();
        for (value, l) in labels {
            let focused = form.field == F_BUTTONS && value == form.save;
            let style = if focused { theme::pill().add_modifier(Modifier::BOLD) } else { Style::default().bg(theme::pal().surface0).fg(theme::pal().text) };
            let r = Rect { x, y: b.y, width: l.width() as u16, height: 1 };
            f.render_widget(Paragraph::new(l).style(style), r);
            buttons.push((r, value));
            x += r.width + 2;
        }
        self.regions.buttons = buttons;
        // the avatar picker over the form
        if let Some(Modal::Agent(form)) = &mut self.modal {
            let color = agents::color(Some(agents::COLORS[form.color]));
            if let Some(p) = form.picker.as_mut() {
                p.render(f, inner, color);
            }
        }
    }
}

/// App name pill at the left of the header and the waiting screen.
const BRAND: &str = " codehead ";

/// Waiting screen while a thread starts.
pub fn draw_status(f: &mut Frame, text: &str, tick: usize) {
    let area = f.area();
    f.render_widget(Paragraph::new("").style(theme::text()), area);
    let spin = ["◐", "◓", "◑", "◒"][tick % 4];
    let line = Line::from(vec![Span::styled(BRAND, theme::accent_bold()), Span::styled(format!(" {spin} "), theme::accent()), Span::styled(text.to_string(), theme::muted())]);
    f.render_widget(Paragraph::new(line), Rect { height: 1, ..area });
}

/// The flow a new task of repo KEY starts with: the last one picked there.
fn flow_defaults_path() -> PathBuf {
    crate::engine::data_dir().join("flow-defaults.json")
}

fn flow_default_get(key: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_slice(&std::fs::read(flow_defaults_path()).ok()?).ok()?;
    v[key].as_str().map(String::from)
}

fn flow_default_set(key: &str, slug: &str) {
    let p = flow_defaults_path();
    let mut v: serde_json::Value = std::fs::read(&p).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_else(|| serde_json::json!({}));
    v[key] = serde_json::Value::String(slug.to_string());
    let _ = crate::engine::write_atomic(&p, v.to_string().as_bytes());
}

#[cfg(test)]
mod render_tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::crossterm::event::KeyEventState;

    fn app() -> App {
        app_with(None)
    }

    #[test]
    fn thread_return_keeps_cached_state_and_queues_background_refresh() {
        let mut a = app();
        a.tab = Tab::Threads;
        a.prompt.set_text("retained draft");
        a.last_esc = Some((Instant::now(), false));
        let files = a.notes.files.clone();
        let agents = a.agents.iter().map(|a| a.name.clone()).collect::<Vec<_>>();
        a.return_from_thread();
        assert!(a.tab == Tab::Threads);
        assert_eq!(a.prompt.text(), "retained draft");
        assert_eq!(a.notes.files, files);
        assert_eq!(a.agents.iter().map(|a| a.name.clone()).collect::<Vec<_>>(), agents);
        assert!(a.last_esc.is_none());
        assert!(a.worktrees.is_some());
        assert_eq!(a.thread_reads.pending(), 1);
        assert_eq!(a.agent_reads.pending(), 1);
    }

    /// DRAFTS: the drafts dir of another test app (its drafts show here). None: a new, empty dir.
    pub(super) fn app_with(drafts: Option<PathBuf>) -> App {
        let dir = std::env::temp_dir().join(format!("ch-render-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("infra")).unwrap();
        std::fs::write(dir.join("infra/deploy.md"), "# Deploy\n\nuse `kubectl rollout`\n").unwrap();
        std::fs::write(dir.join("ideas.md"), "# Ideas\n\nfish shell tips\n").unwrap();
        let cfg = Config {
            notes: dir.clone(),
            repo: None,
            cwd: dir.clone(),
            agent: "amon".into(),
            picks: vec![],
            branch: None,
            prefix: "task".into(),
            copy_mode: false,
            logs: dir.join("logs"),
            drafts: drafts.clone().unwrap_or_else(|| std::env::temp_dir().join(format!("codehead-drafts-app-{}-{:?}", std::process::id(), std::thread::current().id()))),
        };
        if drafts.is_none() {
            let _ = std::fs::remove_dir_all(&cfg.drafts);
        }
        let mut a = App::new(cfg, "");
        let _ = std::fs::remove_file(&a.settings_path);
        // 1 fixed agent: the machine's agents must not change the tests
        a.agents = vec![Agent { name: "amon".into(), harness: "claude".into(), ..Default::default() }];
        a.picks = vec!["amon".into()];
        a.agent_reads = Reads::default();
        a.thread_reads = Reads::default();
        a.session_reads = Reads::default();
        a.worktrees = Some(Vec::new());
        a.sessions.clear();
        a
    }

    #[test]
    fn the_repo_setting_starts_the_where_rows_on_its_repo() {
        let base = std::env::temp_dir().join(format!("ch-default-repo-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let mk = |n: &str| {
            let d = base.join(n);
            std::fs::create_dir_all(&d).unwrap();
            assert!(std::process::Command::new("sh").arg("-c").arg("git init -q -b main && git config user.email t@t && git config user.name t && git commit -q --allow-empty -m i && mkdir sub").current_dir(&d).status().unwrap().success());
            d
        };
        let (start, other) = (mk("start"), mk("other"));
        let mut a = app();
        a.start_cwd = start.clone();
        a.set_repo(crate::repopick::Entry { name: "start".into(), path: start.clone(), git: true, clone: None });
        // a dir in the start repo: 0 change, the current branch stays first
        a.use_default_repo(&start.join("sub"));
        assert_eq!((a.cfg.repo.as_deref(), a.where_i), (Some(start.as_path()), 0));
        // a dir in another repo: that repo, a new worktree first
        a.use_default_repo(&other.join("sub"));
        assert_eq!(a.cfg.repo.as_deref(), Some(other.as_path()));
        assert_eq!(a.wheres.get(a.where_i), Some(&Where::NewWorktree));
        // codehead started outside of a git repo: the setting's repo all the same
        let mut b = app();
        b.start_cwd = base.clone();
        b.cfg.repo = None;
        b.use_default_repo(&other);
        assert_eq!((b.here_branch.as_str(), b.wheres.get(b.where_i)), ("main", Some(&Where::NewWorktree)));
        // 0 git repo: 0 change
        a.use_default_repo(&base);
        assert_eq!(a.cfg.repo.as_deref(), Some(other.as_path()));
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn repo_field_picks_a_repo_in_2_panes_and_a_missing_one_gets_cloned_first() {
        let base = std::env::temp_dir().join(format!("ch-repos-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let mk = |n: &str| {
            let d = base.join(n);
            std::fs::create_dir_all(&d).unwrap();
            assert!(std::process::Command::new("sh").arg("-c").arg("git init -q -b main && git config user.email t@t && git config user.name t && echo x > a && git add -A && git commit -qm i && git branch feat && mkdir -p sub/deep").current_dir(&d).status().unwrap().success());
            d
        };
        let (start, other) = (mk("start"), mk("other"));
        let mut a = app();
        a.start_cwd = start.clone();
        a.set_repo(crate::repopick::Entry { name: "start".into(), path: start.clone(), git: true, clone: None });
        a.repos = vec![crate::home::Repo { name: "gone".into(), path: base.join("gone"), url: Some("git@x:y/gone.git".into()) }];
        a.screen = Screen::Compose;
        a.title.set_text("fix it");
        // ↓ from the title: the repo row, then the branch row (0 field skipped)
        a.field = Field::Title;
        key(&mut a, KeyCode::Down, KeyModifiers::NONE);
        assert!(a.field == Field::Repo);
        key(&mut a, KeyCode::Down, KeyModifiers::NONE);
        assert!(a.field == Field::Where);
        // r on the branch row: 0 repo switch (the hotkey is gone)
        key(&mut a, KeyCode::Char('r'), KeyModifiers::NONE);
        assert_eq!(a.cfg.repo.as_deref(), Some(start.as_path()));
        key(&mut a, KeyCode::Up, KeyModifiers::NONE);
        // enter: the picker on start; left: base's dirs (gone: a clone), right: start's dirs
        key(&mut a, KeyCode::Enter, KeyModifiers::NONE);
        let out = dump(&mut a, 120, 40);
        assert_eq!(a.repo_pick.as_ref().and_then(|p| p.current()).map(|e| e.path.clone()), Some(start.clone()), "{out}");
        assert!(out.lines().any(|l| l.contains("start/") && l.contains("git")), "{out}");
        assert!(out.lines().any(|l| l.contains("gone/") && l.contains("│ sub/")), "{out}");
        assert!(out.lines().any(|l| l.contains("gone/") && l.contains("clone")), "{out}");
        println!("--- repo picker\n{out}");
        // ↑ (other) + enter: other, a new worktree first
        key(&mut a, KeyCode::Up, KeyModifiers::NONE);
        key(&mut a, KeyCode::Enter, KeyModifiers::NONE);
        assert!(a.repo_pick.is_none());
        assert_eq!(a.cfg.repo.as_deref(), Some(other.as_path()));
        assert_eq!(a.wheres.get(a.where_i), Some(&Where::NewWorktree), "another repo: a new worktree first");
        assert!(a.wheres.contains(&Where::Branch("feat".into())));
        assert!(a.clone_first().is_none());
        // a dir in a repo (→ into other, ↓ to sub): its repo
        key(&mut a, KeyCode::Enter, KeyModifiers::NONE);
        key(&mut a, KeyCode::Right, KeyModifiers::NONE);
        assert_eq!(a.repo_pick.as_ref().and_then(|p| p.current()).map(|e| e.name.clone()).as_deref(), Some("sub"));
        key(&mut a, KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(a.cfg.repo.as_deref(), Some(other.as_path()));
        // the picker on the repo (other, 0 sub), ↑ to gone: cloned first
        key(&mut a, KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(a.repo_pick.as_ref().and_then(|p| p.current()).map(|e| e.name.clone()).as_deref(), Some("other"));
        key(&mut a, KeyCode::Up, KeyModifiers::NONE);
        key(&mut a, KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(a.wheres, [Where::NewWorktree]);
        assert_eq!(a.clone_first(), Some(("git@x:y/gone.git".to_string(), base.join("gone"))));
        assert!(a.where_text().starts_with("clones git@x:y/gone.git into"), "{}", a.where_text());
        // a dir outside of a git repo: a toast, the repo stays
        key(&mut a, KeyCode::Enter, KeyModifiers::NONE);
        key(&mut a, KeyCode::Left, KeyModifiers::NONE);
        key(&mut a, KeyCode::Enter, KeyModifiers::NONE);
        assert!(a.repo_pick.is_some() && a.clone_first().is_some());
        key(&mut a, KeyCode::Esc, KeyModifiers::NONE);
        assert!(a.repo_pick.is_none() && a.outcome.is_none());
        // the start repo again: the start dir, current branch first
        key(&mut a, KeyCode::Enter, KeyModifiers::NONE);
        for _ in 0..2 {
            key(&mut a, KeyCode::Down, KeyModifiers::NONE);
        }
        key(&mut a, KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!((a.cfg.repo.as_deref(), a.where_i, a.cfg.cwd.clone()), (Some(start.as_path()), 0, start.clone()));
        // "where" a label; the repo row and the branch row below it, the boxes in their column
        let s = dump(&mut a, 120, 40);
        let at = |pat: &str| s.lines().position(|l| l.contains(pat)).unwrap_or_else(|| panic!("{pat}\n{s}"));
        let col = |pat: &str| s.lines().find_map(|l| l.find(pat).map(|b| l[..b].chars().count())).unwrap();
        assert!(s.lines().any(|l| l.trim() == "where"), "{s}");
        assert_eq!((at(" repo "), at(" branch ")), (at("where") + 1, at("where") + 2), "{s}");
        assert!(s.lines().any(|l| l.contains(" repo ") && l.contains(" start ")), "{s}");
        assert_eq!(col("repo"), col("branch"), "{s}");
        assert_eq!(col("checkout selected"), col("branch") + 2, "{s}");
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn outside_of_a_git_repo_the_repo_row_stays_in_the_field_order() {
        let mut a = app();
        a.cfg.repo = None;
        a.repo = None;
        a.wheres.clear();
        a.screen = Screen::Compose;
        a.field = Field::Title;
        key(&mut a, KeyCode::Down, KeyModifiers::NONE);
        assert!(a.field == Field::Repo);
        key(&mut a, KeyCode::Down, KeyModifiers::NONE);
        assert!(a.field == Field::Agents);
        let s = dump(&mut a, 120, 40);
        assert!(s.contains("0 git repo") && s.contains("enter: pick a repo"), "{s}");
    }

    #[test]
    fn where_row_branch_new_branch_worktree() {
        let dir = std::env::temp_dir().join(format!("codehead-where-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let g = |args: &[&str]| assert!(threads::git(&dir, args).0, "git {args:?}");
        g(&["init", "-q", "-b", "main"]);
        g(&["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-q", "--allow-empty", "-m", "a"]);
        g(&["branch", "feature/x"]);
        g(&["switch", "-q", "feature/x"]);
        let cfg = Config {
            notes: dir.join("notes"),
            repo: Some(dir.clone()),
            cwd: dir.clone(),
            agent: "amon".into(),
            picks: vec![],
            branch: None,
            prefix: "task".into(),
            copy_mode: false,
            logs: dir.join("logs"),
            drafts: dir.join("drafts"),
        };
        let mut a = App::new(cfg, "Fix deploy");
        a.agent_reads = Reads::default();
        // order: current branch (feature/x), new branch, new worktree, main, other branches
        assert_eq!(a.wheres, [Where::Current, Where::NewBranch, Where::NewWorktree, Where::Branch("main".into())]);
        assert!(matches!(a.target(), Ok(Target::Worktree { branch, .. }) if branch == "feature/x"));
        assert_eq!(a.checkout_box(), (true, false), "current branch: checked, disabled");
        assert!(!a.stash_box().1);
        a.field = Field::Where;
        // the dropdown, bottom up: main, new worktree, new branch, current (the cursor)
        let pick_up = |a: &mut App, ups: usize| {
            key(a, KeyCode::Enter, KeyModifiers::NONE);
            for _ in 0..ups {
                key(a, KeyCode::Up, KeyModifiers::NONE);
            }
            key(a, KeyCode::Enter, KeyModifiers::NONE);
            assert!(a.where_menu.is_none());
        };
        pick_up(&mut a, 1);
        assert!(matches!(a.target(), Ok(Target::NewBranch(b)) if b == "task/fix-deploy"));
        assert_eq!(a.checkout_box(), (false, false));
        pick_up(&mut a, 1);
        assert!(matches!(a.target(), Ok(Target::Branch(b)) if b == "task/fix-deploy"));
        assert_eq!(a.where_chip(), "+ new branch + worktree");
        pick_up(&mut a, 1);
        assert!(matches!(a.target(), Ok(Target::Checkout { branch, switch: true, stash: false }) if branch == "main"));
        assert!(a.where_text().starts_with("switches ") && a.where_text().ends_with("from feature/x to main"), "{}", a.where_text());
        // opts: 1 box per row, ↓ from where: checkout; ↓ again: stash (only with checkout)
        key(&mut a, KeyCode::Down, KeyModifiers::NONE);
        assert!(a.field == Field::Opts && a.opt_i == 0);
        key(&mut a, KeyCode::Down, KeyModifiers::NONE);
        assert!(a.field == Field::Opts && a.opt_i == 1);
        key(&mut a, KeyCode::Char(' '), KeyModifiers::NONE);
        assert!(matches!(a.target(), Ok(Target::Checkout { stash: true, .. })));
        key(&mut a, KeyCode::Up, KeyModifiers::NONE);
        assert!(a.field == Field::Opts && a.opt_i == 0);
        key(&mut a, KeyCode::Char(' '), KeyModifiers::NONE);
        assert!(matches!(a.target(), Ok(Target::Checkout { switch: false, stash: false, .. })));
        assert!(a.where_text().contains("0 checkout"), "{}", a.where_text());
        key(&mut a, KeyCode::Char(' '), KeyModifiers::NONE);
        key(&mut a, KeyCode::Up, KeyModifiers::NONE);
        assert!(a.field == Field::Where);
        // the cursor starts on main (the current choice): 3 rows down to current branch
        key(&mut a, KeyCode::Enter, KeyModifiers::NONE);
        for _ in 0..3 {
            key(&mut a, KeyCode::Down, KeyModifiers::NONE);
        }
        key(&mut a, KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(a.where_chip(), "current branch · feature/x");
        // a worktree from the threads tab wins until a pick in the dropdown
        a.target = Some(Target::Worktree { path: "/w".into(), branch: "b".into() });
        assert_eq!(a.where_chip(), "worktree b");
        pick_up(&mut a, 0);
        assert_eq!(a.where_chip(), "current branch · feature/x");
        // field order: title → repo → branch → opts (2 rows, also with disabled boxes) → agents
        a.field = Field::Title;
        key(&mut a, KeyCode::Down, KeyModifiers::NONE);
        assert!(a.field == Field::Repo);
        key(&mut a, KeyCode::Down, KeyModifiers::NONE);
        assert!(a.field == Field::Where);
        key(&mut a, KeyCode::Down, KeyModifiers::NONE);
        assert!(a.field == Field::Opts);
        key(&mut a, KeyCode::Down, KeyModifiers::NONE);
        assert!(a.field == Field::Opts && a.opt_i == 1);
        key(&mut a, KeyCode::Down, KeyModifiers::NONE);
        assert!(a.field == Field::Agents);
        key(&mut a, KeyCode::Up, KeyModifiers::NONE);
        assert!(a.field == Field::Opts && a.opt_i == 1);
        a.screen = Screen::Compose;
        a.agents = vec![
            Agent { name: "amon".into(), harness: "claude".into(), model: Some("opus".into()), color: Some("red".into()), ..Default::default() },
            Agent { name: "nova".into(), harness: "codex".into(), model: Some("gpt-5.5".into()), ..Default::default() },
        ];
        a.picks = vec!["amon".into()];
        let out = dump(&mut a, 110, 38);
        assert!(out.contains("branch  current branch · feature/x"), "{out}");
        assert!(out.contains("◼ checkout selected branch"), "{out}");
        // each box on its own row, below each other
        let lines: Vec<&str> = out.lines().collect();
        let row = |s: &str| lines.iter().position(|l| l.contains(s)).unwrap_or_else(|| panic!("{s}\n{out}"));
        let (co, st) = (row("checkout selected branch"), row("stash conflicting local changes"));
        assert_eq!((row("branch  current"), co), (co - 1, st - 1), "{out}");
        assert_eq!(lines[co].find("checkout"), lines[st].find("stash"), "{out}");
        assert!(out.contains("claude · Opus 5.5") && out.contains("codex · gpt-5.5"), "{out}");
        // 0 blank rows between the harness groups: the codex heading right below the claude card
        let codex = row("codex · 1");
        assert!(lines[codex - 1].contains('└'), "{out}");
        println!("--- where row\n{out}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn where_dropdown_orders_filters_picks_and_closes() {
        let dir = std::env::temp_dir().join(format!("codehead-where-dd-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let repo = dir.join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        // main (2020), alpha (2021, own worktree), beta (2022), feature/x (2023, checked out)
        let sh = "git init -q -b main && git config user.email t@t && git config user.name t \
            && GIT_COMMITTER_DATE=2020-01-01T00:00:00 git commit -q --allow-empty -m m \
            && git switch -q -c alpha && GIT_COMMITTER_DATE=2021-01-01T00:00:00 git commit -q --allow-empty -m a \
            && git switch -q -c beta && GIT_COMMITTER_DATE=2022-01-01T00:00:00 git commit -q --allow-empty -m b \
            && git switch -q -c feature/x && GIT_COMMITTER_DATE=2023-01-01T00:00:00 git commit -q --allow-empty -m x \
            && git worktree add -q ../wt-alpha alpha";
        assert!(std::process::Command::new("sh").arg("-c").arg(sh).current_dir(&repo).status().unwrap().success());
        let mut a = app();
        a.cfg.repo = Some(repo.clone());
        a.cfg.cwd = repo.clone();
        let c = where_choices(&repo, &repo);
        (a.wheres, a.checkouts, a.repo_branch, a.here_branch, a.here_dir) = (c.wheres, c.checkouts, c.repo_branch, c.here_branch, c.here_dir);
        a.where_i = 0;
        a.title.set_text("fix it");
        a.field = Field::Where;
        let names = |a: &App| a.regions.where_rows.iter().map(|(_, i)| a.where_label(&a.wheres[*i]).0).collect::<Vec<_>>();
        // enter: open; the rows bottom up: current, new branch, new worktree, main, then the newest
        key(&mut a, KeyCode::Enter, KeyModifiers::NONE);
        let out = dump(&mut a, 110, 40);
        assert_eq!(names(&a), ["alpha", "beta", "main", "+ new branch + worktree", "+ new branch", "current branch"], "{out}");
        assert!(out.contains("● current branch") && out.contains("feature/x"), "{out}");
        assert!(out.lines().any(|l| l.contains("alpha") && l.contains("worktree …") && l.contains("wt-alpha")), "alpha: its worktree\n{out}");
        println!("--- where dropdown\n{out}");
        // above the where row, the cursor on the current choice (bottom row)
        assert!(a.regions.where_rows.iter().all(|(r, _)| r.y < a.regions.where_row.y), "{out}");
        assert_eq!(a.where_menu.as_ref().map(|m| m.cursor), Some(5));
        // ↑↑↑: main; enter picks it and closes
        for _ in 0..3 {
            key(&mut a, KeyCode::Up, KeyModifiers::NONE);
        }
        key(&mut a, KeyCode::Enter, KeyModifiers::NONE);
        assert!(a.where_menu.is_none());
        assert_eq!(a.wheres[a.where_i], Where::Branch("main".into()));
        // reopened: the cursor on main; type: filter (r types too: 0 repo switch), best at the bottom
        key(&mut a, KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(a.where_menu.as_ref().map(|m| m.cursor), Some(2));
        type_text(&mut a, "alp");
        let out = dump(&mut a, 110, 40);
        assert_eq!(names(&a), ["alpha"], "{out}");
        assert!(out.contains("where › alp") && out.contains("1 of 6"), "{out}");
        key(&mut a, KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(a.wheres[a.where_i], Where::Branch("alpha".into()));
        assert_eq!(a.checkout_box(), (true, false), "alpha: checked out in its worktree");
        // 0 matches: enter keeps it open; backspaces back to every row, the cursor on alpha
        key(&mut a, KeyCode::Char(' '), KeyModifiers::NONE);
        type_text(&mut a, "zzz");
        let out = dump(&mut a, 110, 40);
        assert!(out.contains("0 matches"), "{out}");
        key(&mut a, KeyCode::Enter, KeyModifiers::NONE);
        assert!(a.where_menu.is_some());
        for _ in 0..3 {
            key(&mut a, KeyCode::Backspace, KeyModifiers::NONE);
        }
        assert_eq!(a.where_menu.as_ref().map(|m| (m.query.as_str(), m.cursor)), Some(("", 0)));
        // esc: closed, choice kept, 0 quit (esc of the dropdown: 0 double esc)
        key(&mut a, KeyCode::Down, KeyModifiers::NONE);
        key(&mut a, KeyCode::Esc, KeyModifiers::NONE);
        key(&mut a, KeyCode::Esc, KeyModifiers::NONE);
        assert!(a.where_menu.is_none() && a.outcome.is_none());
        assert_eq!(a.wheres[a.where_i], Where::Branch("alpha".into()));
        // mouse: a click on the where row opens it, on a row picks, next to it closes
        dump(&mut a, 110, 40);
        let w = a.regions.where_row;
        let click = |x: u16, y: u16| MouseEvent { kind: MouseEventKind::Down(MouseButton::Left), column: x, row: y, modifiers: KeyModifiers::NONE };
        a.mouse(click(w.x + 1, w.y));
        assert!(a.where_menu.is_some());
        dump(&mut a, 110, 40);
        let (r, i) = *a.regions.where_rows.iter().find(|(_, i)| a.wheres[*i] == Where::NewBranch).unwrap();
        a.mouse(click(r.x + 2, r.y));
        assert!(a.where_menu.is_none() && a.where_i == i);
        a.mouse(click(w.x + 1, w.y));
        a.mouse(click(0, 0));
        assert!(a.where_menu.is_none() && a.wheres[a.where_i] == Where::NewBranch);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn notes_field_has_a_blank_row_below() {
        let mut a = app();
        let h = 30;
        let out = dump(&mut a, 100, h);
        let lines: Vec<&str> = out.lines().collect();
        let notes = lines.iter().rposition(|l| l.contains("notes · 0 picked")).unwrap();
        // notes label, blank row, footer
        assert_eq!(notes, h as usize - 3, "{out}");
        assert!(lines[notes + 1].trim().is_empty(), "{out}");
    }

    #[test]
    fn prompt_list_items_indent_with_space_and_outdent_with_backspace() {
        let mut a = app();
        assert!(a.field == Field::Prompt);
        // typed: `- ` then space → nested, text goes after the marker
        type_text(&mut a, "- a");
        key(&mut a, KeyCode::Enter, KeyModifiers::NONE);
        type_text(&mut a, " b");
        assert_eq!(a.prompt.text(), "- a\n  - b");
        // text already there: cursor right after the marker → space indents, backspace outdents
        a.prompt.set_text("- a\n- foo");
        for _ in 0..3 {
            key(&mut a, KeyCode::Left, KeyModifiers::NONE);
        }
        key(&mut a, KeyCode::Char(' '), KeyModifiers::NONE);
        assert_eq!(a.prompt.text(), "- a\n  - foo");
        key(&mut a, KeyCode::Char(' '), KeyModifiers::NONE);
        assert_eq!(a.prompt.text(), "- a\n    - foo");
        key(&mut a, KeyCode::Backspace, KeyModifiers::NONE);
        assert_eq!(a.prompt.text(), "- a\n  - foo");
        key(&mut a, KeyCode::Backspace, KeyModifiers::NONE);
        assert_eq!(a.prompt.text(), "- a\n- foo");
        // indent 0: backspace as before
        key(&mut a, KeyCode::Backspace, KeyModifiers::NONE);
        assert_eq!(a.prompt.text(), "- a\n-foo");
        // undo: back through the steps
        key(&mut a, KeyCode::Char('z'), KeyModifiers::CONTROL);
        assert_eq!(a.prompt.text(), "- a\n- foo");
        key(&mut a, KeyCode::Char('z'), KeyModifiers::CONTROL);
        assert_eq!(a.prompt.text(), "- a\n  - foo");
        let out = dump(&mut a, 100, 20);
        assert!(out.contains("  • foo"), "{out}");
    }

    fn key(app: &mut App, code: KeyCode, m: KeyModifiers) {
        app.key(KeyEvent { code, modifiers: m, kind: ratatui::crossterm::event::KeyEventKind::Press, state: KeyEventState::NONE });
    }

    fn dump(app: &mut App, w: u16, h: u16) -> String {
        let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
        for _ in 0..30 {
            t.draw(|f| app.draw(f)).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(20));
            app.tick();
        }
        t.draw(|f| app.draw(f)).unwrap();
        let b = t.backend().buffer().clone();
        (0..h).map(|y| (0..w).map(|x| b[(x, y)].symbol().to_string()).collect::<String>().trim_end().to_string()).collect::<Vec<_>>().join("\n")
    }

    use ratatui::Terminal;

    #[test]
    fn starts_in_task_view() {
        let mut a = app();
        assert!(a.screen == Screen::Compose && a.field == Field::Prompt);
        println!("--- start\n{}", dump(&mut a, 100, 18));
    }

    #[test]
    fn enter_keeps_zero_notes() {
        let mut a = app();
        // prompt → title → repo → flow → agents → notes field (↓), enter opens the picker
        for _ in 0..5 {
            key(&mut a, KeyCode::Down, KeyModifiers::NONE);
        }
        assert!(a.field == Field::Notes);
        key(&mut a, KeyCode::Enter, KeyModifiers::NONE);
        assert!(a.screen == Screen::Pick && a.tab == Tab::Notes);
        key(&mut a, KeyCode::Down, KeyModifiers::NONE);
        key(&mut a, KeyCode::Enter, KeyModifiers::NONE);
        assert!(a.selected.is_empty());
        assert!(a.screen == Screen::Compose && a.field == Field::Notes);
        assert!(!a.message().contains("Read these notes first"));
    }

    #[test]
    fn cmd_or_ctrl_enter_starts() {
        for m in [KeyModifiers::CONTROL, KeyModifiers::SUPER] {
            let mut a = app();
            a.title.set_text("Fix deploy");
            // ctrl+s saves a draft (the form starts over), never starts
            key(&mut a, KeyCode::Char('s'), KeyModifiers::CONTROL);
            assert!(a.outcome.is_none(), "ctrl+s must not start");
            a.title.set_text("Fix deploy");
            key(&mut a, KeyCode::Enter, m);
            assert!(matches!(a.outcome, Some(Outcome::New { .. })));
        }
    }

    #[test]
    fn double_esc_quits() {
        let mut a = app();
        key(&mut a, KeyCode::Esc, KeyModifiers::NONE);
        assert!(a.outcome.is_none());
        key(&mut a, KeyCode::Esc, KeyModifiers::NONE);
        assert!(matches!(a.outcome, Some(Outcome::Quit)));
        // another key between the 2 esc: no quit
        let mut a = app();
        key(&mut a, KeyCode::Esc, KeyModifiers::NONE);
        key(&mut a, KeyCode::Down, KeyModifiers::NONE);
        key(&mut a, KeyCode::Esc, KeyModifiers::NONE);
        assert!(a.outcome.is_none());
    }

    #[test]
    fn escape_never_switches_main_tabs() {
        for tab in [Tab::New, Tab::Threads, Tab::Agents, Tab::Settings] {
            let mut a = app();
            a.switch_tab(tab);
            key(&mut a, KeyCode::Esc, KeyModifiers::NONE);
            assert!(a.cur_tab() == tab && a.outcome.is_none());
            key(&mut a, KeyCode::Esc, KeyModifiers::NONE);
            assert!(a.cur_tab() == tab);
            assert!(matches!(a.outcome, Some(Outcome::Quit)), "esc esc in {tab:?}: codehead closes");
        }
    }

    #[test]
    fn escape_closes_local_controls_without_switching_tabs() {
        let mut a = app();
        a.switch_tab(Tab::Agents);
        a.run(Act::NewAgent);
        key(&mut a, KeyCode::Esc, KeyModifiers::NONE);
        assert!(a.modal.is_none() && a.tab == Tab::Agents);
        key(&mut a, KeyCode::Esc, KeyModifiers::NONE);
        assert!(a.outcome.is_none() && a.tab == Tab::Agents);
        a.switch_tab(Tab::Threads);
        key(&mut a, KeyCode::Char('f'), KeyModifiers::CONTROL);
        a.paste("thread filter");
        key(&mut a, KeyCode::Esc, KeyModifiers::NONE);
        assert!(!a.tlist.filtering && a.tab == Tab::Threads);
        key(&mut a, KeyCode::Esc, KeyModifiers::NONE);
        assert!(a.outcome.is_none() && a.tab == Tab::Threads);
        a.open_notes();
        key(&mut a, KeyCode::Esc, KeyModifiers::NONE);
        key(&mut a, KeyCode::Esc, KeyModifiers::NONE);
        assert!(a.outcome.is_none() && a.screen == Screen::Compose);
        key(&mut a, KeyCode::Esc, KeyModifiers::NONE);
        key(&mut a, KeyCode::Esc, KeyModifiers::NONE);
        assert!(matches!(a.outcome, Some(Outcome::Quit)));
    }

    #[test]
    fn prompt_at_agents_and_attachments() {
        let mut a = app();
        a.agents = vec![Agent { name: "amon".into(), ..Default::default() }, Agent { name: "nova".into(), ..Default::default() }];
        a.title.set_text("t");
        a.field = Field::Prompt;
        for c in "ask @n".chars() {
            key(&mut a, KeyCode::Char(c), KeyModifiers::NONE);
        }
        println!("--- @ dropdown\n{}", dump(&mut a, 80, 24));
        key(&mut a, KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(a.prompt.text(), "ask @nova ");
        // esc closes the dropdown, 0 tab switch
        key(&mut a, KeyCode::Char('@'), KeyModifiers::NONE);
        key(&mut a, KeyCode::Esc, KeyModifiers::NONE);
        assert!(a.screen == Screen::Compose && a.prompt.text() == "ask @nova @");
        // a dropped image path → marker + attachment list
        let img = std::env::temp_dir().join(format!("codehead-att-{}.png", std::process::id()));
        std::fs::write(&img, b"\x89PNG").unwrap();
        a.paste(&img.display().to_string());
        assert!(a.prompt.text().ends_with("[Image #1]"));
        let m = a.message();
        assert!(m.ends_with(&format!("Attachments (read these files):\n- [Image #1]: {}", img.display())), "{m}");
        key(&mut a, KeyCode::Backspace, KeyModifiers::NONE);
        assert!(!a.prompt.text().contains("[Image #1]"));
        assert!(!a.message().contains(&img.display().to_string()));
        assert!(a.draft().files.is_empty());
        key(&mut a, KeyCode::Char('z'), KeyModifiers::CONTROL);
        assert!(a.message().contains(&img.display().to_string()));
        let _ = std::fs::remove_file(&img);
    }

    fn type_text(a: &mut App, text: &str) {
        for c in text.chars() {
            key(a, KeyCode::Char(c), KeyModifiers::NONE);
        }
    }

    #[test]
    fn ctrl_s_keeps_the_task_as_a_draft_and_starts_over() {
        let mut a = app();
        let dir = a.cfg.drafts.clone();
        // 0 text: 0 draft
        key(&mut a, KeyCode::Char('s'), KeyModifiers::CONTROL);
        assert!(drafts::load(&dir, "").is_empty());
        a.field = Field::Title;
        type_text(&mut a, "idea for later");
        key(&mut a, KeyCode::Char('s'), KeyModifiers::CONTROL);
        assert!(a.outcome.is_none());
        // the form: empty; the draft: kept, listed, marked
        assert!(a.title.text().is_empty() && a.field == Field::Prompt);
        let all = drafts::load(&dir, &a.draft_id);
        assert_eq!(all.iter().map(|d| (d.title.as_str(), d.kept)).collect::<Vec<_>>(), [("idea for later", true)]);
        a.field = Field::Drafts;
        key(&mut a, KeyCode::Enter, KeyModifiers::NONE);
        assert!(dump(&mut a, 120, 40).contains("★ saved"));
        // loaded and edited: it stays kept
        key(&mut a, KeyCode::Enter, KeyModifiers::NONE);
        type_text(&mut a, "!");
        let d = drafts::load(&dir, "").into_iter().find(|d| d.title.starts_with("idea")).unwrap();
        assert!(d.kept, "{d:?}");
    }

    #[test]
    fn save_as_draft_button_and_menu_new_starts_empty() {
        let mut a = app();
        let dir = a.cfg.drafts.clone();
        // 0 drafts: the save button only
        let s = dump(&mut a, 120, 40);
        assert!(s.contains("save as draft") && !s.contains("view drafts"), "{s}");
        type_text(&mut a, "first idea");
        a.field = Field::Drafts;
        // ← with 0 drafts: stays on save; enter: kept, the form empty
        key(&mut a, KeyCode::Left, KeyModifiers::NONE);
        key(&mut a, KeyCode::Enter, KeyModifiers::NONE);
        assert!(a.prompt.text().is_empty());
        assert_eq!(drafts::load(&dir, &a.draft_id).iter().map(|d| (d.prompt.as_str(), d.kept)).collect::<Vec<_>>(), [("first idea", true)]);
        assert!(dump(&mut a, 120, 40).contains("view drafts (1)"));
        // a loaded draft, then menu "n": the form empty, the draft kept in the list
        a.field = Field::Drafts;
        key(&mut a, KeyCode::Left, KeyModifiers::NONE);
        key(&mut a, KeyCode::Enter, KeyModifiers::NONE);
        key(&mut a, KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(a.prompt.text(), "first idea");
        type_text(&mut a, " more");
        a.go_home(crate::menu::Home::New);
        assert!(a.prompt.text().is_empty() && a.title.text().is_empty());
        assert_eq!(a.drafts.iter().map(|d| d.prompt.as_str()).collect::<Vec<_>>(), ["first idea more"]);
        // an empty form: "n" again, 0 new draft
        a.go_home(crate::menu::Home::New);
        assert_eq!(drafts::load(&dir, "").len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn drafts_backup_list_load_delete() {
        let mut a = app();
        let dir = a.cfg.drafts.clone();
        // 0 text: 0 draft file, 0 drafts field
        assert!(drafts::load(&dir, "").is_empty());
        assert!(!dump(&mut a, 120, 40).contains("view drafts"));
        // every key: the draft file has the text
        a.field = Field::Title;
        type_text(&mut a, "fix the build");
        assert_eq!(drafts::load(&dir, "").iter().map(|d| d.title.clone()).collect::<Vec<_>>(), ["fix the build"]);
        a.field = Field::Prompt;
        type_text(&mut a, "- a");
        // enter continues the list: "- " comes by itself
        key(&mut a, KeyCode::Enter, KeyModifiers::NONE);
        type_text(&mut a, "b");
        assert_eq!(drafts::load(&dir, "")[0].prompt, "- a\n- b");
        // its own draft: not in its list
        assert!(a.drafts.is_empty());
        // another codehead (the 1st one died): the draft shows, ↑ from the prompt reaches the field
        let mut b = app_with(Some(dir.clone()));
        assert!(dump(&mut b, 120, 40).contains("view drafts (1)"));
        assert!(b.field == Field::Prompt);
        key(&mut b, KeyCode::Up, KeyModifiers::NONE);
        assert!(b.field == Field::Drafts);
        // enter: the dropdown; esc: back to the task (0 quit)
        key(&mut b, KeyCode::Enter, KeyModifiers::NONE);
        let s = dump(&mut b, 120, 40);
        assert!(s.contains(" drafts ") && s.contains("fix the build") && s.contains("d d: delete"), "{s}");
        key(&mut b, KeyCode::Esc, KeyModifiers::NONE);
        assert!(b.drafts_menu.is_none() && b.outcome.is_none());
        // enter enter: loaded, the draft is this task's own now
        key(&mut b, KeyCode::Enter, KeyModifiers::NONE);
        key(&mut b, KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!((b.title.text().as_str(), b.prompt.text().as_str()), ("fix the build", "- a\n- b"));
        assert!(b.drafts.is_empty() && b.field == Field::Prompt);
        assert_eq!(drafts::load(&dir, "").len(), 1);
        // a 3rd codehead: d d deletes the draft, the field goes
        let mut c = app_with(Some(dir.clone()));
        key(&mut c, KeyCode::Up, KeyModifiers::NONE);
        key(&mut c, KeyCode::Enter, KeyModifiers::NONE);
        key(&mut c, KeyCode::Char('d'), KeyModifiers::NONE);
        assert_eq!(c.drafts.len(), 1, "a 1st d only asks");
        key(&mut c, KeyCode::Char('d'), KeyModifiers::NONE);
        assert!(c.drafts.is_empty() && c.drafts_menu.is_none() && c.field == Field::Prompt);
        assert!(drafts::load(&dir, "").is_empty());
        // a started thread drops its draft; an emptied task too
        type_text(&mut c, "x");
        assert_eq!(drafts::load(&dir, "").len(), 1);
        key(&mut c, KeyCode::Backspace, KeyModifiers::NONE);
        assert!(drafts::load(&dir, "").is_empty());
        type_text(&mut c, "y");
        c.drop_draft();
        assert!(drafts::load(&dir, "").is_empty());
        // a started thread clears the form: the next new task starts empty, 0 old draft
        c.field = Field::Prompt;
        type_text(&mut c, "the prompt");
        let old = c.draft_id.clone();
        c.clear_task();
        assert_eq!((c.title.text().as_str(), c.prompt.text().as_str()), ("", ""));
        assert!(c.selected.is_empty() && c.files.is_empty() && c.carry.is_empty() && c.field == Field::Prompt && c.draft_id != old);
        assert!(drafts::load(&dir, "").is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn prompt_command_dropdown_inserts_a_file() {
        let mut a = app();
        a.field = Field::Prompt;
        type_text(&mut a, "read >");
        let s = dump(&mut a, 120, 40);
        assert!(s.contains("┌ commands ") && s.contains("mention"), "{s}");
        // mention → files (of the test dir, 0 git) → the note file, by fuzzy text
        key(&mut a, KeyCode::Enter, KeyModifiers::NONE);
        let s = dump(&mut a, 120, 40);
        assert!(s.contains(" mention ") && s.contains("artifacts"), "{s}");
        key(&mut a, KeyCode::Enter, KeyModifiers::NONE);
        let s = dump(&mut a, 120, 40);
        assert!(s.contains(" mention › files ") && s.contains("ideas.md"), "{s}");
        type_text(&mut a, "dply");
        key(&mut a, KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(a.prompt.text(), "read infra/deploy.md ");
        // esc on the dropdown: closed, 0 quit, the text stays
        type_text(&mut a, ">");
        key(&mut a, KeyCode::Esc, KeyModifiers::NONE);
        assert!(a.outcome.is_none() && a.screen == Screen::Compose);
        let s = dump(&mut a, 120, 40);
        assert!(!s.contains("┌ commands "), "{s}");
    }

    #[test]
    fn up_from_the_task_top_reaches_the_tabs() {
        let mut a = app();
        assert!(a.screen == Screen::Compose && a.field == Field::Prompt && a.drafts.is_empty());
        // prompt → drafts row ("save as draft") → tabs
        key(&mut a, KeyCode::Up, KeyModifiers::NONE);
        assert!(a.field == Field::Drafts && !a.tabs_focus);
        key(&mut a, KeyCode::Up, KeyModifiers::NONE);
        assert!(a.tabs_focus);
    }

    #[test]
    fn task_reuses_agent_groups_with_selection_and_scroll() {
        let mut a = app();
        let ag = |name: &str, harness: &str| Agent { name: name.into(), harness: harness.into(), ..Default::default() };
        a.agents = vec![ag("paimon", "codex"), ag("lilith", "claude"), ag("amon", "claude"), ag("bael", "claude"), ag("zagan", "amp")];
        a.picks.clear();
        a.pick_cursor = 4;
        a.field = Field::Agents;
        // An agents-tab filter must not hide task choices.
        a.afilter = Some("paimon".into());
        let mut terminal = Terminal::new(TestBackend::new(200, 48)).unwrap();
        terminal.draw(|f| a.draw(f)).unwrap();
        assert_eq!(a.pick_places.iter().map(|p| p.0).collect::<Vec<_>>(), [4, 2, 3, 1, 0]);
        assert_eq!(a.pick_places, a.agent_grid(198).places);
        let claude: Vec<_> = a.pick_places.iter().filter(|(i, _, _)| a.agents[*i].harness == "claude").collect();
        assert!(claude.iter().all(|p| p.2 == claude[0].2));
        key(&mut a, KeyCode::Char(' '), KeyModifiers::NONE);
        key(&mut a, KeyCode::Down, KeyModifiers::NONE);
        assert_eq!(a.agents[a.pick_cursor].name, "amon");
        key(&mut a, KeyCode::Right, KeyModifiers::NONE);
        key(&mut a, KeyCode::Right, KeyModifiers::NONE);
        assert_eq!(a.agents[a.pick_cursor].name, "lilith");
        key(&mut a, KeyCode::Char(' '), KeyModifiers::NONE);
        key(&mut a, KeyCode::Down, KeyModifiers::NONE);
        assert_eq!(a.agents[a.pick_cursor].name, "paimon");
        key(&mut a, KeyCode::Char(' '), KeyModifiers::NONE);
        assert_eq!(a.picks, ["zagan", "lilith", "paimon"]);
        terminal.draw(|f| a.draw(f)).unwrap();
        assert!(a.pick_offset > 0);
        assert!(a.regions.menu_rows.iter().any(|(_, i)| *i == a.pick_cursor));
        // Mouse selection uses the displayed card's agent, not its sorted slot.
        let (r, _) = *a.regions.menu_rows.iter().find(|(_, i)| *i == 2).unwrap();
        a.mouse(MouseEvent { kind: MouseEventKind::Down(MouseButton::Left), column: r.x + 1, row: r.y + 1, modifiers: KeyModifiers::NONE });
        assert_eq!(a.picks, ["zagan", "lilith", "paimon", "amon"]);
        a.pick_cursor = 0;
        key(&mut a, KeyCode::Down, KeyModifiers::NONE);
        assert!(a.field == Field::Notes);
        key(&mut a, KeyCode::Up, KeyModifiers::NONE);
        assert!(a.field == Field::Agents);

        // A narrow terminal creates vertical card rows and retains the cursor.
        let mut narrow = Terminal::new(TestBackend::new(50, 30)).unwrap();
        narrow.draw(|f| a.draw(f)).unwrap();
        assert!(a.pick_places.iter().all(|p| p.1 == 0));
        assert!(a.regions.menu_rows.iter().any(|(_, i)| *i == a.pick_cursor));
        key(&mut a, KeyCode::Up, KeyModifiers::NONE);
        assert_eq!(a.agents[a.pick_cursor].name, "lilith");
        a.pick_cursor = 4;
        key(&mut a, KeyCode::Up, KeyModifiers::NONE);
        assert!(a.field == Field::Opts || a.field == Field::Repo, "↑ on the top row of cards: the field above");
        a.agents.clear();
        narrow.draw(|f| a.draw(f)).unwrap();
        assert!(a.pick_places.is_empty() && a.regions.menu_rows.is_empty());
    }

    #[test]
    fn agents_tab_groups_cards_by_harness_filters_and_moves_in_the_grid() {
        let mut a = app();
        // 1 draw, 0 tick: the machine's agents never replace the fixture
        let once = |a: &mut App, w: u16, h: u16| {
            let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
            t.draw(|f| a.draw(f)).unwrap();
            let b = t.backend().buffer().clone();
            (0..h).map(|y| (0..w).map(|x| b[(x, y)].symbol()).collect::<String>().trim_end().to_string()).collect::<Vec<_>>().join("\n")
        };
        let ag = |name: &str, harness: &str| Agent { name: name.into(), harness: harness.into(), ..Default::default() };
        a.agents = vec![ag("paimon", "codex"), ag("lilith", "claude"), ag("amon", "claude"), ag("bael", "claude"), ag("zagan", "amp")];
        a.switch_tab(Tab::Agents);
        // 0 read of the machine's agents: the fixture stays
        a.agent_reads = Reads::default();
        let order: Vec<(String, Vec<String>)> = a.avisible().into_iter().map(|(h, g)| (h, g.into_iter().map(|i| a.agents[i].name.clone()).collect())).collect();
        assert_eq!(order, vec![
            ("amp".to_string(), vec!["zagan".to_string()]),
            ("claude".into(), vec!["amon".into(), "bael".into(), "lilith".into()]),
            ("codex".into(), vec!["paimon".into()]),
        ]);
        let screen = once(&mut a, 200, 40);
        println!("--- agents\n{screen}");
        for head in ["amp · 1", "claude · 3", "codex · 1"] { assert!(screen.contains(head), "{head}\n{screen}"); }
        // a wide list: the claude group takes 1 row of 3 cards
        let claude: Vec<_> = a.aplace.iter().filter(|(i, _, _)| a.agents[*i].harness == "claude").collect();
        assert!(claude.iter().all(|(_, _, y)| *y == claude[0].2), "{claude:?}");
        let name = |a: &App| a.agents[a.acursor].name.clone();
        a.acursor = 4;
        // ↓ zagan → the claude row (nearest x), → → along the row, ↓ → paimon, ↑↑↑ → tabs
        key(&mut a, KeyCode::Down, KeyModifiers::NONE);
        assert_eq!(name(&a), "amon");
        key(&mut a, KeyCode::Right, KeyModifiers::NONE);
        key(&mut a, KeyCode::Right, KeyModifiers::NONE);
        assert_eq!(name(&a), "lilith");
        key(&mut a, KeyCode::Down, KeyModifiers::NONE);
        assert_eq!(name(&a), "paimon");
        for _ in 0..2 { key(&mut a, KeyCode::Up, KeyModifiers::NONE); }
        assert_eq!(name(&a), "zagan");
        key(&mut a, KeyCode::Up, KeyModifiers::NONE);
        assert!(a.tabs_focus);
        // narrow: 1 card per row
        a.tabs_focus = false;
        let _ = once(&mut a, 60, 60);
        let claude: Vec<_> = a.aplace.iter().filter(|(i, _, _)| a.agents[*i].harness == "claude").map(|p| p.1).collect();
        assert!(claude.iter().all(|&x| x == 0), "{claude:?}");
        // the filter: f, type, enter; esc clears it
        key(&mut a, KeyCode::Char('f'), KeyModifiers::CONTROL);
        assert!(a.afilter_editing);
        a.paste("lil");
        key(&mut a, KeyCode::Enter, KeyModifiers::NONE);
        assert!(!a.afilter_editing);
        assert_eq!(a.avisible().iter().map(|(_, g)| g.len()).sum::<usize>(), 1);
        assert_eq!(name(&a), "lilith");
        let screen = once(&mut a, 120, 30);
        assert!(screen.contains("filter › lil") && screen.contains("1 of 5"), "{screen}");
        key(&mut a, KeyCode::Esc, KeyModifiers::NONE);
        assert!(a.afilter.is_none() && a.outcome.is_none());
    }

    #[test]
    fn hover_paints_clickable_items() {
        let mut a = app();
        let mut t = Terminal::new(TestBackend::new(100, 20)).unwrap();
        t.draw(|f| a.draw(f)).unwrap();
        let bg = |t: &Terminal<TestBackend>, x: u16, y: u16| t.backend().buffer()[(x, y)].bg;
        let moved = |x: u16, y: u16| MouseEvent { kind: MouseEventKind::Moved, column: x, row: y, modifiers: KeyModifiers::NONE };
        // tab "threads" in the header
        let tab = a.regions.tabs.iter().find(|(_, t)| *t == Tab::Threads).unwrap().0;
        a.mouse(moved(tab.x + 1, tab.y));
        t.draw(|f| a.draw(f)).unwrap();
        assert_eq!(bg(&t, tab.x + 1, tab.y), theme::pal().surface1);
        // the repo row of the task view
        let w = a.regions.repo_row;
        a.mouse(moved(w.x + 2, w.y));
        t.draw(|f| a.draw(f)).unwrap();
        assert_eq!(bg(&t, w.x + 2, w.y), theme::pal().surface1);
        assert_ne!(bg(&t, tab.x + 1, tab.y), theme::pal().surface1, "1 item at a time");
        // agents tab: a card takes the hover, the free room right of it and its heading do not
        a.switch_tab(Tab::Agents);
        a.agents.truncate(1);
        t.draw(|f| a.draw(f)).unwrap();
        let card = a.acards[0].0;
        a.mouse(moved(card.right() + 1, card.y + 1));
        t.draw(|f| a.draw(f)).unwrap();
        assert_ne!(bg(&t, card.right() + 1, card.y + 1), theme::pal().surface1);
        a.mouse(moved(card.x + 1, card.y - 1));
        t.draw(|f| a.draw(f)).unwrap();
        assert_ne!(bg(&t, card.x + 1, card.y - 1), theme::pal().surface1);
        a.mouse(moved(card.x + 2, card.y + 1));
        t.draw(|f| a.draw(f)).unwrap();
        assert_eq!(bg(&t, card.x + 2, card.y + 1), theme::pal().surface1);
    }

    /// Type "hi", undo (ctrl+z / ⌘z), redo (ctrl+shift+z / ⌘⇧z); TEXT reads the field.
    fn undo_redo(a: &mut App, text: impl Fn(&App) -> String) {
        let before = text(a);
        for c in "hi".chars() {
            key(a, KeyCode::Char(c), KeyModifiers::NONE);
        }
        let after = text(a);
        assert_eq!(after, format!("{before}hi"));
        key(a, KeyCode::Char('z'), KeyModifiers::CONTROL);
        assert_eq!(text(a), before);
        key(a, KeyCode::Char('Z'), KeyModifiers::CONTROL | KeyModifiers::SHIFT);
        assert_eq!(text(a), after);
        key(a, KeyCode::Char('z'), KeyModifiers::SUPER);
        assert_eq!(text(a), before);
        key(a, KeyCode::Char('z'), KeyModifiers::SUPER | KeyModifiers::SHIFT);
        assert_eq!(text(a), after);
    }

    #[test]
    fn undo_redo_in_all_fields() {
        let mut a = app();
        a.field = Field::Title;
        undo_redo(&mut a, |a| a.title.text());
        a.field = Field::Prompt;
        undo_redo(&mut a, |a| a.prompt.text());
        a.run(Act::NewAgent);
        for field in [F_NAME, F_HARNESS, F_VOICE, F_PROMPT] {
            if let Some(Modal::Agent(f)) = &mut a.modal {
                f.field = field;
            }
            undo_redo(&mut a, |a| match &a.modal {
                Some(Modal::Agent(f)) => [&f.name, &f.harness, &f.voice, &f.prompt][[F_NAME, F_HARNESS, F_VOICE, F_PROMPT].iter().position(|x| *x == f.field).unwrap()].text(),
                _ => panic!("agent form closed"),
            });
        }
        a.modal = None;
        a.worktrees = Some(Vec::new());
        a.switch_tab(Tab::Threads);
        key(&mut a, KeyCode::Char('f'), KeyModifiers::CONTROL);
        undo_redo(&mut a, |a| a.tlist.query());
        a.switch_tab(Tab::Notes);
        key(&mut a, KeyCode::Char('f'), KeyModifiers::CONTROL);
        undo_redo(&mut a, |a| a.filter.clone().unwrap_or_default());
        assert_eq!(a.rows.len(), 0, "filter \"hi\" matches 0 notes");
        key(&mut a, KeyCode::Char('z'), KeyModifiers::CONTROL);
        assert!(!a.rows.is_empty(), "undo rebuilds the list");
    }

    #[test]
    fn agent_form_enter_opens_the_avatar_picker() {
        let mut a = app();
        a.run(Act::NewAgent);
        let form = |a: &App| match &a.modal {
            Some(Modal::Agent(f)) => (f.picker.is_some(), f.sprite.avatar),
            _ => panic!("agent form closed"),
        };
        if let Some(Modal::Agent(f)) = &mut a.modal {
            f.field = F_SPRITE;
            f.sprite = sprites::Sprite::of(crate::avatar::Avatar::default());
        }
        key(&mut a, KeyCode::Enter, KeyModifiers::NONE);
        assert!(form(&a).0, "enter: the picker");
        dump(&mut a, 120, 42);
        // tab: geo; → : the 2nd; f: half speed; enter: taken
        key(&mut a, KeyCode::Tab, KeyModifiers::NONE);
        key(&mut a, KeyCode::Right, KeyModifiers::NONE);
        key(&mut a, KeyCode::Char('f'), KeyModifiers::NONE);
        key(&mut a, KeyCode::Enter, KeyModifiers::NONE);
        let (open, av) = form(&a);
        assert!(!open);
        assert_eq!((av.cat, av.idx, av.speed), (crate::avatar::Cat::Geo, 1, 0.5));
        // esc in the picker: the avatar stays
        key(&mut a, KeyCode::Enter, KeyModifiers::NONE);
        key(&mut a, KeyCode::Right, KeyModifiers::NONE);
        key(&mut a, KeyCode::Esc, KeyModifiers::NONE);
        assert_eq!(form(&a), (false, av));
        for _ in 0..10 {
            key(&mut a, KeyCode::Char('r'), KeyModifiers::NONE);
            assert_eq!(form(&a).1.cat, crate::avatar::Cat::Geo, "r: random in the category");
        }
    }

    #[test]
    fn agent_form_arrows_leave_text_boxes() {
        let mut a = app();
        a.run(Act::NewAgent);
        let field = |a: &App| match &a.modal {
            Some(Modal::Agent(f)) => f.field,
            _ => usize::MAX,
        };
        if let Some(Modal::Agent(f)) = &mut a.modal {
            f.field = F_VOICE;
            f.voice.set_text("line one\nline two");
        }
        dump(&mut a, 120, 42);
        // cursor on the last line: ↑ moves inside first, then leaves to the sprite
        key(&mut a, KeyCode::Up, KeyModifiers::NONE);
        assert_eq!(field(&a), F_VOICE);
        key(&mut a, KeyCode::Up, KeyModifiers::NONE);
        assert_eq!(field(&a), F_SPRITE);
        key(&mut a, KeyCode::Down, KeyModifiers::NONE);
        assert_eq!(field(&a), F_VOICE);
        // ↓ inside the box first, then out to the system prompt
        key(&mut a, KeyCode::Down, KeyModifiers::NONE);
        assert_eq!(field(&a), F_VOICE);
        key(&mut a, KeyCode::Down, KeyModifiers::NONE);
        assert_eq!(field(&a), F_PROMPT);
        key(&mut a, KeyCode::Down, KeyModifiers::NONE);
        assert_eq!(field(&a), F_BUTTONS, "empty system prompt: 1 row, ↓ leaves");
    }

    #[test]
    fn agent_form_harness_and_model_dropdowns() {
        use crate::models::{Entry, Model};
        let mut a = app();
        a.run(Act::NewAgent);
        fn form(a: &mut App) -> &mut AgentForm {
            match &mut a.modal {
                Some(Modal::Agent(f)) => f,
                _ => panic!("0 agent form"),
            }
        }
        let m = |id: &str, name: &str| Model { id: id.into(), name: name.into() };
        // fixed installed harnesses; codex: a fresh cached list; reads: a fake, 0 harness spawned
        form(&mut a).harnesses = [("claude", "Claude Code", "claude"), ("codex", "Codex", "codex"), ("opencode", "OpenCode", "opencode")]
            .map(|(n, l, b)| (n.to_string(), l.to_string(), b.to_string()))
            .to_vec();
        let at = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
        a.models = Some(HashMap::from([("codex".to_string(), Entry { at, models: vec![m("gpt-6.1-sol", "GPT-6.1-Sol"), m("gpt-6.1-terra", "GPT-6.1 Terra")] })]));
        a.probe = |h| if h == "opencode" { Ok(vec![Model { id: "opencode/big-pickle".into(), name: String::new() }]) } else { Err("probe failed".into()) };
        type_text(&mut a, "nova");
        key(&mut a, KeyCode::Down, KeyModifiers::NONE);
        assert_eq!(form(&mut a).field, F_HARNESS);
        // enter: the installed harnesses, the current one marked
        key(&mut a, KeyCode::Enter, KeyModifiers::NONE);
        let out = dump(&mut a, 120, 44);
        assert!(out.contains("harnesses · installed") && out.contains("● Claude Code") && out.contains("Codex") && out.contains("OpenCode"), "{out}");
        // type: filter; enter picks → the model list of codex (from the cache: 0 read)
        type_text(&mut a, "codex");
        let out = dump(&mut a, 120, 44);
        assert!(out.contains("harnesses › codex") && out.contains("1 of 3") && !out.contains("OpenCode"), "{out}");
        key(&mut a, KeyCode::Enter, KeyModifiers::NONE);
        let f = form(&mut a);
        assert_eq!((f.harness.text(), f.field), ("codex".to_string(), F_MODEL));
        assert!(f.pick.as_ref().is_some_and(|p| p.model && p.harness == "codex"));
        assert_eq!(a.model_reads.pending(), 0, "fresh cache: 0 read");
        let out = dump(&mut a, 120, 44);
        assert!(out.contains("models · codex") && out.contains("● default") && out.contains("gpt-6.1-sol") && out.contains("GPT-6.1 Terra"), "{out}");
        println!("--- model dropdown\n{out}");
        // ↓↓ + enter: gpt-6.1-terra → harness/model
        key(&mut a, KeyCode::Down, KeyModifiers::NONE);
        key(&mut a, KeyCode::Down, KeyModifiers::NONE);
        key(&mut a, KeyCode::Enter, KeyModifiers::NONE);
        let f = form(&mut a);
        assert!(f.pick.is_none());
        assert_eq!(f.harness.text(), "codex/gpt-6.1-terra");
        assert_eq!(f.spec(&[], None).harness, "codex/gpt-6.1-terra", "the saved value");
        let out = dump(&mut a, 120, 44);
        assert!(out.contains(" gpt-6.1-terra   codex models: enter"), "{out}");
        // enter on the model field: the list again, the cursor on the current model; esc closes
        key(&mut a, KeyCode::Enter, KeyModifiers::NONE);
        let out = dump(&mut a, 120, 44);
        assert!(out.contains("● gpt-6.1-terra"), "{out}");
        assert_eq!(form(&mut a).pick.as_ref().map(|p| p.cursor), Some(2));
        // r: read again (the fake fails: the note says so, the cached list stays)
        key(&mut a, KeyCode::Char('r'), KeyModifiers::NONE);
        for _ in 0..100 {
            if a.model_reads.pending() == 0 {
                break;
            }
            a.tick();
            std::thread::sleep(Duration::from_millis(10));
        }
        let out = dump(&mut a, 120, 44);
        assert!(out.contains("models: probe failed") && out.contains("gpt-6.1-sol"), "{out}");
        // 0 match: enter takes the typed id
        type_text(&mut a, "gpt-7");
        let out = dump(&mut a, 120, 44);
        assert!(out.contains("0 matches · enter: use as typed"), "{out}");
        key(&mut a, KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(form(&mut a).harness.text(), "codex/gpt-7");
        key(&mut a, KeyCode::Enter, KeyModifiers::NONE);
        key(&mut a, KeyCode::Esc, KeyModifiers::NONE);
        assert!(form(&mut a).pick.is_none() && a.modal.is_some(), "esc: only the dropdown");
        assert_eq!(form(&mut a).harness.text(), "codex/gpt-7");
        // opencode: 0 cache → reading models…, then the read list
        form(&mut a).field = F_HARNESS;
        key(&mut a, KeyCode::Enter, KeyModifiers::NONE);
        type_text(&mut a, "open");
        key(&mut a, KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(form(&mut a).harness.text(), "opencode");
        let mut t = Terminal::new(TestBackend::new(120, 44)).unwrap();
        t.draw(|f| a.draw(f)).unwrap();
        let b = t.backend().buffer().clone();
        let first = (0..44).map(|y| (0..120).map(|x| b[(x, y)].symbol().to_string()).collect::<String>()).collect::<Vec<_>>().join("\n");
        assert!(first.contains("reading models…"), "{first}");
        let out = dump(&mut a, 120, 44);
        assert!(out.contains("opencode/big-pickle"), "{out}");
        // a click on a row picks it
        let (r, _) = a.regions.form_rows.iter().copied().find(|(_, j)| *j == 1).unwrap();
        a.mouse(MouseEvent { kind: MouseEventKind::Down(MouseButton::Left), column: r.x + 2, row: r.y, modifiers: KeyModifiers::NONE });
        assert_eq!(form(&mut a).harness.text(), "opencode/opencode/big-pickle");
        // claude: aliases with their exact ids; a pick → claude/<alias>
        form(&mut a).field = F_HARNESS;
        key(&mut a, KeyCode::Enter, KeyModifiers::NONE);
        type_text(&mut a, "claude");
        key(&mut a, KeyCode::Enter, KeyModifiers::NONE);
        let out = dump(&mut a, 120, 44);
        assert!(out.contains("models · claude") && out.contains("claude-opus-5-5") && out.contains("● default"), "{out}");
        type_text(&mut a, "sonnet");
        key(&mut a, KeyCode::Enter, KeyModifiers::NONE);
        let f = form(&mut a);
        assert_eq!((f.harness.text(), f.harness_model()), ("claude".to_string(), "claude/sonnet".to_string()));
    }

    #[test]
    fn fuzzy_ranks() {
        assert!(fuzzy("lci", "local ci v1").is_some());
        assert!(fuzzy("xyz", "local ci v1").is_none());
        assert!(fuzzy("local", "local ci v1") > fuzzy("local", "l o c a l"));
    }

    #[test]
    fn thread_filter_keys() {
        let mut a = app();
        a.worktrees = Some(Vec::new());
        a.sessions = vec![
            Session { title: "local ci v1".into(), cwd: "/p".into(), last: "x".into(), ..Default::default() },
            Session { title: "Ampcode ideas".into(), cwd: "/p".into(), last: "x".into(), ..Default::default() },
        ];
        a.switch_tab(Tab::Threads);
        a.tlist.set_view(crate::threadlist::View::Claude);
        // f and /: 0 filter; ctrl+f: the only key to it
        key(&mut a, KeyCode::Char('f'), KeyModifiers::NONE);
        key(&mut a, KeyCode::Char('/'), KeyModifiers::NONE);
        assert!(!a.tlist.filtering);

        key(&mut a, KeyCode::Char('f'), KeyModifiers::CONTROL);
        assert!(a.tlist.filtering);
        for c in "amp".chars() {
            key(&mut a, KeyCode::Char(c), KeyModifiers::NONE);
        }
        assert_eq!(a.n_threads(), 1);
        assert_eq!(a.current_session().unwrap().title, "Ampcode ideas");
        key(&mut a, KeyCode::Enter, KeyModifiers::NONE);
        assert!(!a.tlist.filtering && a.tlist.query() == "amp");
        key(&mut a, KeyCode::Char('f'), KeyModifiers::CONTROL);
        key(&mut a, KeyCode::Esc, KeyModifiers::NONE);
        assert!(!a.tlist.filtering);
        println!("--- thread filter\n{}", dump(&mut a, 110, 10));
    }

    #[test]
    fn settings_tab_entry_keeps_paths_unfocused() {
        let mut a = app();
        let config = format!("notes = {:?}\n", format!("{}/infra/", a.cfg.cwd.display()));
        std::fs::write(&a.settings_path, &config).unwrap();

        a.switch_tab(Tab::Agents);
        key(&mut a, KeyCode::Char('6'), KeyModifiers::ALT);
        assert!(a.tab == Tab::Settings && a.tabs_focus);
        let screen = dump(&mut a, 100, 36);
        assert!(!screen.contains("deploy.md"), "{screen}");
        key(&mut a, KeyCode::Char('1'), KeyModifiers::ALT);
        assert!(a.screen == Screen::Compose);
        assert_eq!(std::fs::read_to_string(&a.settings_path).unwrap(), config);

        key(&mut a, KeyCode::Char('6'), KeyModifiers::ALT);
        assert!(a.tab == Tab::Settings && a.tabs_focus);
        key(&mut a, KeyCode::Down, KeyModifiers::NONE);
        assert!(!a.tabs_focus);
        let screen = dump(&mut a, 100, 36);
        assert!(!screen.contains("deploy.md"), "{screen}");
        key(&mut a, KeyCode::End, KeyModifiers::NONE);
        let screen = dump(&mut a, 100, 36);
        assert!(screen.contains("deploy.md"), "{screen}");

        // Re-entry must remove focus even after the path field was active.
        a.switch_tab(Tab::Agents);
        key(&mut a, KeyCode::Char('6'), KeyModifiers::ALT);
        assert!(a.tab == Tab::Settings && a.tabs_focus);
        let screen = dump(&mut a, 100, 36);
        assert!(!screen.contains("deploy.md"), "{screen}");
        key(&mut a, KeyCode::Char('4'), KeyModifiers::ALT);
        assert!(a.tab == Tab::Agents);
        assert_eq!(std::fs::read_to_string(&a.settings_path).unwrap(), config);
    }

    #[test]
    fn settings_path_escape_closes_dropdown_before_tab_navigation() {
        let mut a = app();
        a.switch_tab(Tab::Settings);
        key(&mut a, KeyCode::Down, KeyModifiers::NONE);
        key(&mut a, KeyCode::Char('a'), KeyModifiers::CONTROL);
        a.paste("local");
        key(&mut a, KeyCode::Down, KeyModifiers::NONE);
        a.paste(&format!("{}/infra/", a.cfg.cwd.display()));
        let screen = dump(&mut a, 100, 100);
        assert!(screen.contains("deploy.md"), "{screen}");
        key(&mut a, KeyCode::Esc, KeyModifiers::NONE);
        assert!(a.tab == Tab::Settings && a.outcome.is_none());
        assert!(a.last_esc.is_none(), "dropdown Escape must not arm double Escape");
        let screen = dump(&mut a, 100, 100);
        assert!(!screen.contains("deploy.md"), "{screen}");
        // tab: the next field (the alias box locked), still settings; alt+1: the task tab
        key(&mut a, KeyCode::Tab, KeyModifiers::NONE);
        assert!(a.screen == Screen::Pick && a.tab == Tab::Settings);
        key(&mut a, KeyCode::Char('1'), KeyModifiers::ALT);
        assert!(a.screen == Screen::Compose);
    }

    #[test]
    fn settings_sub_tabs_reach_the_mcp_servers_and_its_add_form() {
        let _env = crate::engine::ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let s = crate::engine::servers::Server { name: "docs-x".into(), kind: "stdio".into(), command: Some("npx".into()), args: vec!["-y".into(), "docs".into()], auto_allow: true, ..Default::default() };
        crate::engine::servers::save(&[s]).unwrap();
        let mut a = app();
        key(&mut a, KeyCode::Char('6'), KeyModifiers::ALT);
        key(&mut a, KeyCode::Down, KeyModifiers::NONE);
        // ↑ from the 1st field: the sub-tab bar; → →: mcp servers
        key(&mut a, KeyCode::Up, KeyModifiers::NONE);
        assert!(a.settings.as_ref().unwrap().bar && !a.tabs_focus);
        key(&mut a, KeyCode::Right, KeyModifiers::NONE);
        let screen = dump(&mut a, 100, 30);
        assert!(screen.contains("harnesses") && screen.contains("model"), "{screen}");
        key(&mut a, KeyCode::Right, KeyModifiers::NONE);
        key(&mut a, KeyCode::Down, KeyModifiers::NONE);
        let screen = dump(&mut a, 100, 30);
        assert!(screen.contains("codehead · every harness") && screen.contains("docs-x") && screen.contains("0 agents · auto-allow"), "{screen}");
        assert!(screen.contains("a add"), "help bar: {screen}");
        // a: the add form; a typed name; esc: asks, esc again: gone, 0 write
        key(&mut a, KeyCode::Char('a'), KeyModifiers::NONE);
        a.paste("new-one");
        let screen = dump(&mut a, 100, 30);
        assert!(screen.contains("add MCP server") && screen.contains("new-one"), "{screen}");
        // 1 read time for both escs: a slow CI runner never puts 2 s between them
        crate::textarea::stamp(Some(std::time::Instant::now()));
        key(&mut a, KeyCode::Esc, KeyModifiers::NONE);
        assert!(dump(&mut a, 100, 30).contains("add MCP server"), "1 esc: the edits stay");

        key(&mut a, KeyCode::Esc, KeyModifiers::NONE);
        crate::textarea::stamp(None);
        let screen = dump(&mut a, 100, 30);
        assert!(!screen.contains("add MCP server"), "{screen}");
        assert_eq!(crate::engine::servers::load().unwrap().len(), 1);
        // the filter: ctrl+f then text, a miss shows 0 rows
        key(&mut a, KeyCode::Char('f'), KeyModifiers::CONTROL);
        a.paste("zzqq");
        let screen = dump(&mut a, 100, 30);
        assert!(screen.contains("0 servers match the filter"), "{screen}");
        crate::engine::servers::save(&[]).unwrap();
    }

    #[test]
    fn settings_tab_keeps_form_edits_and_task_text() {
        let mut a = app();
        a.title.set_text("task title");
        a.prompt.set_text("task prompt");
        key(&mut a, KeyCode::Char('6'), KeyModifiers::ALT);
        assert!(a.tab == Tab::Settings);
        key(&mut a, KeyCode::Down, KeyModifiers::NONE);
        key(&mut a, KeyCode::Char('a'), KeyModifiers::CONTROL);
        a.paste("project:platform");
        key(&mut a, KeyCode::Down, KeyModifiers::NONE);
        a.paste("~/notes/platform/");
        let screen = dump(&mut a, 70, 18);
        assert!(screen.lines().next().unwrap().contains("settings") && screen.contains("mcp servers"), "{screen}");
        assert!(screen.contains("not applied"), "{screen}");
        assert!(screen.contains("~/notes/platform/"), "{screen}");
        // alt+1 / alt+4: away and back, the form edits and the task text stay
        key(&mut a, KeyCode::Char('1'), KeyModifiers::ALT);
        assert!(a.screen == Screen::Compose);
        assert_eq!(a.title.text(), "task title");
        assert_eq!(a.prompt.text(), "task prompt");
        key(&mut a, KeyCode::Char('6'), KeyModifiers::ALT);
        let screen = dump(&mut a, 100, 24);
        assert!(screen.contains("~/notes/platform/"), "{screen}");
        key(&mut a, KeyCode::Char('4'), KeyModifiers::ALT);
        assert!(a.tab == Tab::Agents);
    }

    #[test]
    fn settings_esc_undoes_the_field_edit_and_never_applies() {
        let mut a = app();
        let before = std::fs::read_to_string(&a.settings_path).unwrap_or_default();
        key(&mut a, KeyCode::Char('6'), KeyModifiers::ALT);
        key(&mut a, KeyCode::Down, KeyModifiers::NONE);
        assert!(a.tab == Tab::Settings && !a.tabs_focus);
        // a plain field: typed, esc → the text at entry, 0 write
        let s = a.settings.as_mut().unwrap();
        let start = s.form.fields[0].text();
        a.paste("/tmp/elsewhere/");
        assert_ne!(a.settings.as_ref().unwrap().form.fields[0].text(), start);
        key(&mut a, KeyCode::Esc, KeyModifiers::NONE);
        assert_eq!(a.settings.as_ref().unwrap().form.fields[0].text(), start);
        assert_eq!(std::fs::read_to_string(&a.settings_path).unwrap_or_default(), before);
        // an alias: unlocked, typed, esc → 0 alias, locked, 0 write
        // the menu → add path alias (its keys end the esc esc window)
        a.last_esc = None;
        a.run(Act::AddAlias);
        a.paste("project:x");
        key(&mut a, KeyCode::Esc, KeyModifiers::NONE);
        let s = a.settings.as_ref().unwrap();
        assert!(s.form.editing.is_none() && s.form.fields[crate::settings::PLAIN].text().is_empty());
        assert_eq!(std::fs::read_to_string(&a.settings_path).unwrap_or_default(), before);
        // tab / shift+tab: the next / previous field, settings stays
        let at = a.settings.as_ref().unwrap().form.focus;
        key(&mut a, KeyCode::BackTab, KeyModifiers::SHIFT);
        assert_eq!(a.settings.as_ref().unwrap().form.focus, at - 1);
        key(&mut a, KeyCode::Tab, KeyModifiers::NONE);
        assert_eq!(a.settings.as_ref().unwrap().form.focus, at);
        assert!(a.tab == Tab::Settings);
    }

    #[test]
    fn alt_digits_switch_tabs_and_tab_walks_the_fields() {
        let mut a = app();
        let alt = |a: &mut App, d: char| key(a, KeyCode::Char(d), KeyModifiers::ALT);
        alt(&mut a, '2');
        assert!(a.screen == Screen::Pick && a.tab == Tab::Threads && !a.tabs_focus);
        key(&mut a, KeyCode::Esc, KeyModifiers::NONE);
        assert!(a.screen == Screen::Pick && a.tab == Tab::Threads);
        // tab on a list: 0 tab switch
        key(&mut a, KeyCode::Tab, KeyModifiers::NONE);
        assert!(a.tab == Tab::Threads);
        alt(&mut a, '1');
        assert!(a.screen == Screen::Compose && a.field == Field::Prompt);
        // alt+2..4: threads, agents, settings
        for (t, d) in [(Tab::Threads, '2'), (Tab::Merge, '3'), (Tab::Agents, '4'), (Tab::Flows, '5'), (Tab::Settings, '6')] {
            alt(&mut a, d);
            assert!(a.screen == Screen::Pick && a.tab == t);
        }
        key(&mut a, KeyCode::Esc, KeyModifiers::NONE);
        assert!(a.tab == Tab::Settings);
        alt(&mut a, '1');
        assert!(a.screen == Screen::Compose);
        // the agents tab loaded the machine's agents: 1 card again (↓ leaves the cards)
        a.agents.truncate(1);
        a.field = Field::Prompt;
        // tab: prompt → title → repo → agents → notes → prompt; shift+tab back
        for f in [Field::Title, Field::Repo, Field::Agents, Field::Notes, Field::Prompt] {
            key(&mut a, KeyCode::Tab, KeyModifiers::NONE);
            assert!(a.field == f && a.screen == Screen::Compose);
        }
        key(&mut a, KeyCode::BackTab, KeyModifiers::SHIFT);
        assert!(a.field == Field::Notes);
        // ↑↓ walk the fields: ↓ on the last stays, ↑ on the prompt's 1st row: the tabs
        key(&mut a, KeyCode::Down, KeyModifiers::NONE);
        assert!(a.field == Field::Notes);
        for f in [Field::Agents, Field::Repo, Field::Title, Field::Prompt, Field::Drafts] {
            key(&mut a, KeyCode::Up, KeyModifiers::NONE);
            assert!(a.field == f);
        }
        key(&mut a, KeyCode::Up, KeyModifiers::NONE);
        assert!(a.tabs_focus);
        key(&mut a, KeyCode::Down, KeyModifiers::NONE);
        // alt+digit in the prompt: tabs; ctrl / ⌥+←→ (alt on macOS; off macOS alt+←: row start): word jumps
        key(&mut a, KeyCode::Down, KeyModifiers::NONE);
        type_text(&mut a, "one two");
        let word = if cfg!(target_os = "macos") { KeyModifiers::ALT } else { KeyModifiers::CONTROL };
        key(&mut a, KeyCode::Left, word);
        type_text(&mut a, "X");
        assert_eq!(a.prompt.text(), "one Xtwo");
        // ctrl+←: a word jump too, 0 tab switch
        key(&mut a, KeyCode::Left, KeyModifiers::CONTROL);
        key(&mut a, KeyCode::Left, KeyModifiers::CONTROL);
        type_text(&mut a, "Y");
        assert!(a.screen == Screen::Compose && a.prompt.text() == "Yone Xtwo", "{}", a.prompt.text());
        alt(&mut a, '2');
        assert!(a.tab == Tab::Threads && a.screen == Screen::Pick);
    }

    #[test]
    fn title_is_optional_the_prompt_line_1_names_the_thread() {
        let mut a = app();
        a.agents.truncate(1);
        a.picks = vec![a.agents[0].name.clone()];
        // 0 prompt, 0 title: 0 start
        key(&mut a, KeyCode::Enter, KeyModifiers::CONTROL);
        assert!(a.outcome.is_none() && a.field == Field::Prompt);
        type_text(&mut a, "  ");
        key(&mut a, KeyCode::Enter, KeyModifiers::CONTROL);
        assert!(a.outcome.is_none());
        a.prompt.set_text("\nFix the deploy script\nit fails on arm64");
        assert_eq!(a.task_title(), "Fix the deploy script");
        // the message: the prompt first (line 1 = the title), 0 line twice
        assert_eq!(a.message(), "Fix the deploy script\nit fails on arm64");
        a.selected = vec!["infra/deploy.md".into()];
        let m = a.message();
        assert!(m.starts_with("Fix the deploy script\nit fails on arm64\n\nRead these notes first:\n- "), "{m}");
        key(&mut a, KeyCode::Enter, KeyModifiers::CONTROL);
        match &a.outcome {
            Some(Outcome::New { message, .. }) => assert_eq!(message.lines().next(), Some("Fix the deploy script")),
            _ => panic!("0 start"),
        }
        // a title: line 1, the prompt after it
        let mut b = app();
        b.title.set_text("Deploy");
        b.prompt.set_text("fix it");
        assert_eq!(b.message(), "Deploy\n\nfix it");
        // a long 1st line: the title (branch name) cut at 80 chars
        b.title.set_text("");
        b.prompt.set_text(&"x".repeat(120));
        assert_eq!(b.task_title().chars().count(), 80);
    }

    #[test]
    fn esc_on_an_edited_agent_form_asks_and_no_keeps_the_form() {
        let mut a = app();
        a.run(Act::NewAgent);
        assert!(matches!(a.modal, Some(Modal::Agent(_))));
        // 0 edits: esc closes at once
        key(&mut a, KeyCode::Esc, KeyModifiers::NONE);
        assert!(a.modal.is_none());
        a.run(Act::NewAgent);
        type_text(&mut a, "nova");
        // esc: a confirm; n: the form back with its text; esc esc fast: still asked, 0 drop
        key(&mut a, KeyCode::Esc, KeyModifiers::NONE);
        assert!(matches!(a.modal, Some(Modal::Confirm { .. })));
        key(&mut a, KeyCode::Char('n'), KeyModifiers::NONE);
        match &a.modal {
            Some(Modal::Agent(f)) => assert_eq!(f.name.text(), "nova"),
            _ => panic!("0 form"),
        }
        key(&mut a, KeyCode::Esc, KeyModifiers::NONE);
        key(&mut a, KeyCode::Esc, KeyModifiers::NONE);
        assert!(matches!(a.modal, Some(Modal::Agent(_))), "esc on the confirm: no, the form stays");
        // y: dropped
        key(&mut a, KeyCode::Esc, KeyModifiers::NONE);
        key(&mut a, KeyCode::Char('y'), KeyModifiers::NONE);
        assert!(a.modal.is_none() && a.outcome.is_none());
    }

    #[test]
    fn reset_asks_first() {
        let mut a = app();
        a.selected = vec!["ideas.md".into()];
        // ctrl+r: 0 reset (ctrl+r reads again on every screen); the menu command asks first
        key(&mut a, KeyCode::Char('r'), KeyModifiers::CONTROL);
        assert!(a.modal.is_none());
        a.run(Act::Reset);
        assert!(matches!(a.modal, Some(Modal::Confirm { .. })));
        key(&mut a, KeyCode::Esc, KeyModifiers::NONE);
        assert_eq!(a.selected, vec!["ideas.md".to_string()]);
        a.run(Act::Reset);
        key(&mut a, KeyCode::Char('y'), KeyModifiers::NONE);
        assert!(a.selected.is_empty() && a.modal.is_none());
    }

    #[test]
    fn several_contexts_go_into_message() {
        let mut a = app();
        a.pick_session(Session { id: "s1".into(), cwd: "/tmp/x".into(), title: "Old work".into(), last: "DONE. 3 files".into(), ..Default::default() });
        a.carry.push(Session { id: "s2".into(), thread: Some("abcdef0123456789".into()), title: "Thread work".into(), last: "OK. shipped".into(), ..Default::default() });
        // same session twice: 1 entry
        a.pick_session(Session { id: "s1".into(), cwd: "/tmp/x".into(), title: "Old work".into(), last: "DONE. 3 files".into(), ..Default::default() });
        assert_eq!(a.carry.len(), 2);
        let m = a.message();
        assert!(m.contains("Claude Code session s1"));
        assert!(m.contains("codehead thread \"Thread work\" (abcdef012345"));
        assert!(m.ends_with("OK. shipped"));
    }

    #[test]
    fn carry_goes_into_message() {
        let mut a = app();
        a.pick_session(Session { id: "s1".into(), cwd: "/tmp/x".into(), title: "Old work".into(), last: "DONE. 3 files".into(), ..Default::default() });
        assert!(matches!(a.target(), Ok(Target::Dir(d)) if d == "/tmp/x"));
        let m = a.message();
        assert!(m.starts_with("Old work\n\nContext: the last response of Claude Code session s1"));
        assert!(m.ends_with("DONE. 3 files"));
    }

    fn card(id: &str, f: impl FnOnce(&mut crate::nav::Card)) -> crate::nav::Card {
        let mut c = crate::nav::Card { id: id.into(), title: format!("thread {id}"), state: "open".into(), ..Default::default() };
        f(&mut c);
        c
    }

    /// The start screen's threads tab: the menu's cards in sections, the view tabs; a asks
    /// first, esc says no (0 quit, 0 tab change), y archives.
    #[test]
    fn start_threads_tab_shows_cards_in_sections_and_archives_after_a_yes() {
        let mut a = app();
        let t = crate::engine::store::Thread::new("archive me", "task", &std::env::temp_dir(), vec!["ada".into()]);
        t.save().unwrap();
        a.worktrees = Some(Vec::new());
        a.tlist.set_cards(vec![
            card("settled", |_| {}),
            card("asking", |c| c.blocked = true),
            card("working", |c| c.busy = true),
            card(&t.id, |c| {
                c.land = Some(crate::landed::Land::Landed);
                c.title = "archive me".into();
            }),
        ]);
        a.switch_tab(Tab::Threads);
        a.tabs_focus = false;
        let s = dump(&mut a, 120, 40);
        for l in ["needs you · 1", "working · 1", "shipped: archive? · 1", "threads · 1", "show", "active 4", "archived 0", "worktrees 0"] {
            assert!(s.contains(l), "{l}: {s}");
        }
        // the shipped card: 3rd (needs you, working, shipped)
        key(&mut a, KeyCode::Down, KeyModifiers::NONE);
        key(&mut a, KeyCode::Down, KeyModifiers::NONE);
        assert_eq!(a.tlist.current().map(|c| c.id.clone()), Some(t.id.clone()));
        key(&mut a, KeyCode::Char('a'), KeyModifiers::NONE);
        let s = dump(&mut a, 120, 40);
        assert!(s.contains("archive 1 thread(s)?"), "{s}");
        key(&mut a, KeyCode::Esc, KeyModifiers::NONE);
        assert!(a.outcome.is_none() && a.tab == Tab::Threads);
        assert_eq!(crate::engine::store::load(&t.id).unwrap().state, "open");
        key(&mut a, KeyCode::Char('a'), KeyModifiers::NONE);
        key(&mut a, KeyCode::Char('y'), KeyModifiers::NONE);
        assert_eq!(crate::engine::store::load(&t.id).unwrap().state, "archived");
        let _ = crate::engine::store::delete(&t.id);
    }

    /// e on a card: the edit form; ctrl+s saves the title; the palette's command runs it too.
    #[test]
    fn start_threads_tab_edits_a_thread() {
        let mut a = app();
        let t = crate::engine::store::Thread::new("before", "task", &std::env::temp_dir(), vec!["ada".into()]);
        t.save().unwrap();
        a.worktrees = Some(Vec::new());
        a.tlist.set_cards(vec![card(&t.id, |c| c.title = "before".into())]);
        a.switch_tab(Tab::Threads);
        a.tabs_focus = false;
        a.run(Act::Thread(crate::threadlist::Act::Edit));
        let s = dump(&mut a, 120, 40);
        assert!(s.contains("edit thread") && s.contains("linked repos") && s.contains("agents"), "{s}");
        // esc in the form: the form closes, 0 quit
        key(&mut a, KeyCode::Esc, KeyModifiers::NONE);
        assert!(a.outcome.is_none() && !a.tlist.modal());
        key(&mut a, KeyCode::Char('e'), KeyModifiers::NONE);
        for _ in 0.."before".len() {
            key(&mut a, KeyCode::Backspace, KeyModifiers::NONE);
        }
        for c in "after".chars() {
            key(&mut a, KeyCode::Char(c), KeyModifiers::NONE);
        }
        key(&mut a, KeyCode::Char('s'), KeyModifiers::CONTROL);
        assert_eq!(crate::engine::store::load(&t.id).unwrap().title, "after");
        let _ = crate::engine::store::delete(&t.id);
    }

    /// Repo tabs over cards and this screen's rows; the claude view: the sessions of the repo.
    #[test]
    fn threads_tab_repo_tabs_and_the_claude_view() {
        let mut a = app();
        a.worktrees = Some(Vec::new());
        a.sessions = vec![
            Session { id: "s1".into(), title: "claude work".into(), cwd: "/x/alpha".into(), repo: Some("/x/alpha".into()), ..Default::default() },
            Session { id: "s2".into(), title: "beta work".into(), cwd: "/x/beta".into(), repo: Some("/x/beta".into()), ..Default::default() },
        ];
        a.tlist.set_cards(vec![card("one", |c| c.repo = Some("/x/alpha".into())), card("two", |c| c.repo = Some("/x/beta".into())), card("three", |c| c.repo = Some("/x/beta".into()))]);
        a.switch_tab(Tab::Threads);
        a.tabs_focus = false;
        let s = dump(&mut a, 120, 40);
        let row = s.lines().find(|l| l.contains("repo ")).expect("repo tabs");
        assert!(row.contains(" beta ") && row.contains(" alpha ") && row.contains(" all "), "{row}");
        // →: the 1st repo (beta: most rows), →: alpha
        key(&mut a, KeyCode::Right, KeyModifiers::NONE);
        assert_eq!(a.tlist.visible().len(), 2);
        key(&mut a, KeyCode::Right, KeyModifiers::NONE);
        assert_eq!(a.tlist.visible().len(), 1);
        // ↑: the view tabs; →→→: claude; ↓: its rows (alpha's session only)
        key(&mut a, KeyCode::Up, KeyModifiers::NONE);
        for _ in 0..3 {
            key(&mut a, KeyCode::Right, KeyModifiers::NONE);
        }
        key(&mut a, KeyCode::Down, KeyModifiers::NONE);
        assert_eq!(a.tlist.view, crate::threadlist::View::Claude);
        assert_eq!(a.n_threads(), 1);
        assert_eq!(a.current_session().map(|s| s.id.clone()), Some("s1".to_string()));
        let s = dump(&mut a, 120, 40);
        assert!(s.contains("claude work") && !s.contains("beta work"), "{s}");
        // ↑ on its top row: the view tabs again
        key(&mut a, KeyCode::Up, KeyModifiers::NONE);
        assert_eq!(a.tlist.focus, crate::threadlist::Focus::Views);
    }

    /// The preview of a card: the full read of its thread (title, not the dir).
    #[test]
    fn a_card_preview_uses_the_threads_full_read() {
        let mut a = app();
        let w = Worktree {
            path: "/project".into(), branch: "in place".into(), linked: false,
            unpushed: 0, when: "1m".into(), repo: None,
            thread: Some(threads::Thread {
                id: "a1".into(), title: "fix latency".into(), state: "open".into(),
                live: 1, updated: 0, worktree: None, branch: None, dir: Some("/project".into()), ..Default::default()
            }),
        };
        a.tpreview.insert(w.key(), threads::preview_header(&w));
        a.worktrees = Some(vec![w]);
        a.tlist.set_cards(vec![card("a1", |c| c.title = "fix latency".into())]);
        a.switch_tab(Tab::Threads);
        let mut t = Terminal::new(TestBackend::new(100, 36)).unwrap();
        t.draw(|f| a.draw(f)).unwrap();
        let buffer = t.backend().buffer();
        let line = |y| (0..100).map(|x| buffer[(x, y)].symbol()).collect::<String>();
        assert!(line(a.regions.preview.y).contains("fix latency"));
        assert!(!line(a.regions.preview.y).contains("in place"));
    }

    #[test]
    fn the_flows_tab_draws_the_rail_with_its_arcs() {
        let mut a = app();
        key(&mut a, KeyCode::Char('5'), KeyModifiers::ALT);
        assert!(a.tab == Tab::Flows);
        key(&mut a, KeyCode::Down, KeyModifiers::NONE);
        key(&mut a, KeyCode::Up, KeyModifiers::NONE);
        key(&mut a, KeyCode::Enter, KeyModifiers::NONE);
        let screen = dump(&mut a, 130, 30);
        println!("--- flows\n{screen}");
        assert!(screen.contains(" flows ") && screen.contains("new feature") && screen.contains("plan approval"), "{screen}");
        assert!(screen.contains("◀") && screen.contains("changes") && screen.contains("↺"), "{screen}");
        key(&mut a, KeyCode::Char('a'), KeyModifiers::NONE);
        for c in "rev".chars() {
            key(&mut a, KeyCode::Char(c), KeyModifiers::NONE);
        }
        let screen = dump(&mut a, 130, 30);
        println!("--- picker\n{screen}");
        assert!(screen.contains("one-off: rev") && screen.contains("peer-review"), "{screen}");
    }

    #[test]
    fn screens() {
        let mut a = app();
        a.field = Field::Title;
        for c in "Fix deploy".chars() {
            key(&mut a, KeyCode::Char(c), KeyModifiers::NONE);
        }
        // title → repo → flow → agents → notes, enter: the notes picker
        for _ in 0..4 {
            key(&mut a, KeyCode::Down, KeyModifiers::NONE);
        }
        key(&mut a, KeyCode::Enter, KeyModifiers::NONE);
        println!("--- notes picker\n{}", dump(&mut a, 130, 16));
        key(&mut a, KeyCode::Down, KeyModifiers::NONE);
        key(&mut a, KeyCode::Char(' '), KeyModifiers::NONE);
        key(&mut a, KeyCode::Char('p'), KeyModifiers::CONTROL);
        println!("--- menu\n{}", dump(&mut a, 100, 22));
        key(&mut a, KeyCode::Esc, KeyModifiers::NONE);
        key(&mut a, KeyCode::Enter, KeyModifiers::NONE);
        // tab from the notes field: round to the prompt
        key(&mut a, KeyCode::Tab, KeyModifiers::NONE);
        assert!(a.field == Field::Prompt);
        a.paste("line one\nline two");
        println!("--- compose\n{}", dump(&mut a, 100, 22));
        println!("--- message\n{}", a.message());
        key(&mut a, KeyCode::Char('2'), KeyModifiers::ALT);
        println!("--- threads\n{}", dump(&mut a, 120, 14));
        key(&mut a, KeyCode::Char('3'), KeyModifiers::NONE);
        assert!(a.tab == Tab::Threads, "0 number keys for tabs");
        key(&mut a, KeyCode::Char('4'), KeyModifiers::ALT);
        println!("--- agents\n{}", dump(&mut a, 120, 14));
        key(&mut a, KeyCode::Char('n'), KeyModifiers::NONE);
        println!("--- new agent\n{}", dump(&mut a, 120, 42));
        for _ in 0..F_SPRITE {
            key(&mut a, KeyCode::Tab, KeyModifiers::NONE);
        }
        key(&mut a, KeyCode::Char('r'), KeyModifiers::NONE);
        println!("--- sprite field\n{}", dump(&mut a, 120, 42));
        // an edited form: esc asks first
        key(&mut a, KeyCode::Esc, KeyModifiers::NONE);
        assert!(matches!(a.modal, Some(Modal::Confirm { .. })));
        key(&mut a, KeyCode::Char('y'), KeyModifiers::NONE);
        assert!(a.modal.is_none());
        assert!(a.message().starts_with("Fix deploy\n\nRead these notes first:\n- "));
        assert!(a.message().ends_with("\nline one\nline two"));
        let _ = std::fs::remove_dir_all(&a.cfg.notes);
    }
}


#[cfg(test)]
mod nav_keys {
    use super::*;
    use super::render_tests::app_with;

    fn press(a: &mut App, code: KeyCode) {
        a.key(KeyEvent::new(code, KeyModifiers::NONE));
    }

    /// The tabs row in focus: a hotkey of the tab works (ctrl+f: filter), the focus goes to the content.
    #[test]
    fn a_tab_hotkey_works_with_the_tabs_row_in_focus() {
        let mut a = app_with(None);
        a.switch_tab(Tab::Threads);
        a.tabs_focus = true;
        a.key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL));
        assert!(a.tlist.filtering && !a.tabs_focus);
        press(&mut a, KeyCode::Esc);
        a.switch_tab(Tab::Agents);
        a.tabs_focus = true;
        a.key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL));
        assert!(a.afilter_editing && !a.tabs_focus);
        // the tabs row keys stay: ←→ tabs, ↑ 0 effect, q quits
        a.afilter_editing = false;
        a.tabs_focus = true;
        press(&mut a, KeyCode::Up);
        assert!(a.tabs_focus && a.tab == Tab::Agents);
    }
}

/// The start screen's preview: mouse selection + copy (as the chat's).
#[cfg(test)]
mod preview_select {
    use super::*;
    use super::render_tests::app_with;
    use ratatui::backend::TestBackend;

    fn screen(a: &mut App) -> Vec<String> {
        let mut t = ratatui::Terminal::new(TestBackend::new(120, 30)).unwrap();
        t.draw(|f| a.draw(f)).unwrap();
        let b = t.backend().buffer().clone();
        (0..30).map(|y| (0..120).map(|x| b[(x, y)].symbol().to_string()).collect::<String>()).collect()
    }

    fn mouse(a: &mut App, kind: MouseEventKind, x: u16, y: u16) {
        a.mouse(MouseEvent { kind, column: x, row: y, modifiers: KeyModifiers::NONE });
    }

    #[test]
    fn the_preview_selects_and_copies() {
        let mut a = app_with(None);
        a.agents = vec![Agent { name: "zagan".into(), harness: "amp".into(), mode: "plan".into(), ..Default::default() }];
        a.switch_tab(Tab::Agents);
        a.agent_reads = Reads::default();
        a.tabs_focus = false;
        let s = screen(&mut a);
        let p = a.regions.preview;
        let y = (p.y..p.bottom()).find(|&y| s[y as usize].contains("mode ")).unwrap_or_else(|| panic!("{s:#?}"));
        let x = s[y as usize].find("plan").map(|b| s[y as usize][..b].chars().count()).unwrap() as u16;
        assert!(p.contains(Position { x, y }));
        // a drag over "plan"
        mouse(&mut a, MouseEventKind::Down(MouseButton::Left), x, y);
        mouse(&mut a, MouseEventKind::Drag(MouseButton::Left), x + 3, y);
        mouse(&mut a, MouseEventKind::Up(MouseButton::Left), x + 3, y);
        assert_eq!(crate::clip::LAST.with_borrow(|l| l.clone()), "plan");
        // drawn: accent cells
        let mut t = ratatui::Terminal::new(TestBackend::new(120, 30)).unwrap();
        t.draw(|f| a.draw(f)).unwrap();
        assert_eq!(t.backend().buffer()[(x, y)].bg, theme::pal().accent);
    }
}

/// The flow field of a new thread: enter opens the flows, a flow opens its steps (2nd dropdown),
/// a step picks the flow and the step the thread starts at.
#[cfg(test)]
mod flow_menu_tests {
    use super::*;
    use super::render_tests::app_with;
    use ratatui::backend::TestBackend;

    fn press(a: &mut App, code: KeyCode) {
        a.key(KeyEvent::new(code, KeyModifiers::NONE));
    }

    fn screen(a: &mut App) -> String {
        let mut t = ratatui::Terminal::new(TestBackend::new(140, 50)).unwrap();
        t.draw(|f| a.draw(f)).unwrap();
        let b = t.backend().buffer().clone();
        (0..50).map(|y| (0..140).map(|x| b[(x, y)].symbol().to_string()).collect::<String>()).collect::<Vec<_>>().join("\n")
    }

    /// `f` on an agent card: that agent's flows, a flow its steps (the start step); the card shows
    /// the flow under its effort row; each agent keeps its own flow.
    #[test]
    fn f_on_a_card_opens_its_flows_and_a_flow_its_steps() {
        let mut a = app_with(None);
        a.flow_lib = crate::engine::flows::Library::load(None);
        a.agents.push(Agent { name: "zed".into(), harness: "claude".into(), ..Default::default() });
        a.flow_default = None;
        assert!(a.flow_lib.flows.len() > 1 && a.agents.len() > 1);
        a.screen = Screen::Compose;
        a.tabs_focus = false;
        a.field = Field::Agents;
        let _ = screen(&mut a);
        a.pick_cursor = 1;
        let (agent, other) = (a.agents[1].name.clone(), a.agents[0].name.clone());
        // f: the flows of the card under the cursor
        press(&mut a, KeyCode::Char('f'));
        assert!(a.flow_menu.as_ref().is_some_and(|m| m.sub.is_none() && m.cursor == 0 && m.agent == agent));
        let s = screen(&mut a);
        let first = a.flow_lib.flows[0].name.clone();
        assert!(s.contains(&format!("flow of {agent}")) && s.contains(&format!("◆ {first}")) && s.contains("0 flow"), "{s}");
        // ↓ to the 1st flow: its preview beside the flows at once
        press(&mut a, KeyCode::Down);
        let steps = a.flow_steps(1);
        let s = screen(&mut a);
        assert!(s.contains("preview") && s.contains(&format!("{first} · {} steps", steps.len())), "{s}");
        // enter: the rail takes the keys; ← back, → in, ↓ enter: flow 1 from step 2
        press(&mut a, KeyCode::Enter);
        assert_eq!(a.flow_menu.as_ref().and_then(|m| m.sub).map(|s| s.0), Some(0));
        press(&mut a, KeyCode::Left);
        assert!(a.flow_menu.as_ref().is_some_and(|m| m.sub.is_none()));
        press(&mut a, KeyCode::Right);
        press(&mut a, KeyCode::Down);
        press(&mut a, KeyCode::Enter);
        assert!(a.flow_menu.is_none());
        assert_eq!(a.flow_at(&agent), (1, 1));
        assert!(a.picks.contains(&agent), "a flow picks the agent");
        assert_eq!(a.flow_at(&other), (0, 0), "the other card keeps 0 flow");
        let s = screen(&mut a);
        println!("--- cards with flows\n{s}");
        assert!(s.contains(&format!("◆ {first} › 2")) && s.contains("0 flow"), "{s}");
        // esc in the dropdown closes it only
        press(&mut a, KeyCode::Char('f'));
        press(&mut a, KeyCode::Esc);
        assert!(a.flow_menu.is_none() && a.outcome.is_none());
        // a click on a flow opens its steps, a click on a step picks it
        press(&mut a, KeyCode::Char('f'));
        let _ = screen(&mut a);
        let (r, _) = *a.regions.flow_rows.iter().find(|(_, i)| *i == 2).unwrap();
        a.mouse(MouseEvent { kind: MouseEventKind::Down(MouseButton::Left), column: r.x + 2, row: r.y, modifiers: KeyModifiers::NONE });
        assert!(a.flow_menu.as_ref().is_some_and(|m| m.cursor == 2 && m.sub.is_some()));
        let _ = screen(&mut a);
        let (r, _) = *a.regions.flow_steps.iter().find(|(_, i)| *i == 0).unwrap();
        a.mouse(MouseEvent { kind: MouseEventKind::Down(MouseButton::Left), column: r.x + 2, row: r.y, modifiers: KeyModifiers::NONE });
        assert_eq!((a.flow_at(&agent), a.flow_menu.is_none()), ((2, 0), true));
        // 0: 0 flow at once
        press(&mut a, KeyCode::Char('f'));
        press(&mut a, KeyCode::Char('0'));
        assert_eq!(a.flow_at(&agent), (0, 0));
    }

    /// The lead's default: the repo's last flow, until a pick; other agents: 0 flow.
    #[test]
    fn the_repos_last_flow_is_the_leads_default_only() {
        let mut a = app_with(None);
        a.flow_lib = crate::engine::flows::Library::load(None);
        a.agents.push(Agent { name: "zed".into(), harness: "claude".into(), ..Default::default() });
        let slug = a.flow_lib.flows[0].slug.clone();
        a.flow_default = Some(slug.clone());
        a.picks = vec![a.agents[1].name.clone(), a.agents[0].name.clone()];
        let (lead, other) = (a.agents[0].name.clone(), a.agents[1].name.clone());
        assert_eq!(a.task_lead().as_deref(), Some(lead.as_str()), "the 1st pick in the cards' order");
        assert_eq!(a.flow_of(&lead).0, slug);
        assert!(a.flow_of(&other).0.is_empty());
        a.agent_flows.insert(lead.clone(), (String::new(), 0));
        assert!(a.flow_of(&lead).0.is_empty(), "0 flow picked: 0 default");
    }
}

/// Key fixes of the UX audit on the start screen: 0 q quit, ctrl+c as in the thread view, a
/// click on No keeps the agent form, D is not d.
#[cfg(test)]
mod key_fix_tests {
    use super::render_tests::app_with;
    use super::*;
    use ratatui::backend::TestBackend;

    fn key(a: &mut App, code: KeyCode, m: KeyModifiers) {
        a.key(KeyEvent::new(code, m));
    }

    #[test]
    fn q_on_the_tabs_row_never_quits() {
        let mut a = app_with(None);
        a.tabs_focus = true;
        key(&mut a, KeyCode::Char('q'), KeyModifiers::NONE);
        assert!(a.outcome.is_none());

        // esc on the tabs row: 0 move into the content
        a.tabs_focus = true;
        a.last_esc = None;
        key(&mut a, KeyCode::Esc, KeyModifiers::NONE);
        assert!(a.tabs_focus && a.outcome.is_none());
    }

    #[test]
    fn ctrl_c_clears_the_field_then_quits_on_a_2nd_press() {
        let mut a = app_with(None);
        a.field = Field::Prompt;
        a.prompt.set_text("a task");
        key(&mut a, KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert_eq!(a.prompt.text(), "");
        assert!(a.outcome.is_none());

        key(&mut a, KeyCode::Char('z'), KeyModifiers::CONTROL);
        assert_eq!(a.prompt.text(), "a task", "ctrl+z: the text back");

        a.prompt.set_text("");
        key(&mut a, KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert!(a.outcome.is_none(), "1st ctrl+c on an empty field: a hint");

        key(&mut a, KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert!(matches!(a.outcome, Some(Outcome::Quit)));
    }

    #[test]
    fn a_click_on_no_brings_the_agent_form_back() {
        let mut a = app_with(None);
        let form = AgentForm::new(None, &a.styles, None);
        a.modal = Some(Modal::Confirm { title: "discard".into(), question: "drop the new agent?".into(), yes: false, action: ConfirmAction::DiscardAgent(Box::new(form)) });
        let mut t = ratatui::Terminal::new(TestBackend::new(110, 40)).unwrap();
        t.draw(|f| a.draw(f)).unwrap();
        let (r, _) = *a.regions.buttons.iter().find(|(_, yes)| !yes).expect("a no button");

        a.mouse(MouseEvent { kind: MouseEventKind::Down(MouseButton::Left), column: r.x + 1, row: r.y, modifiers: KeyModifiers::NONE });
        assert!(matches!(a.modal, Some(Modal::Agent(_))), "no: the form with its edits");
    }

    #[test]
    fn a_capital_letter_is_not_its_small_command() {
        let shift_d = KeyEvent::new(KeyCode::Char('D'), KeyModifiers::SHIFT);
        assert_eq!(key_name(&shift_d), "D");
        assert_eq!(key_name(&KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE)), "d");
    }
}
