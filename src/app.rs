//! Screens: new (compose, the start) | threads | agents; notes = sub-screen of task. Modals: F2 menu, y/n confirm, info.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
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
const CARD_H: u16 = crate::sprites::H as u16 + 2;

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
    New { message: String, target: Target, agents: Vec<String> },
    Resume(Worktree),
}

/// $EDITOR on: a note, the compose prompt, the sprite of the open agent form.
pub enum Suspend {
    Note(PathBuf),
    Prompt,
    Sprite,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Tab {
    /// The compose screen (Screen::Compose).
    New,
    /// Sub-screen of task: enter on the notes field.
    Notes,
    Threads,
    Agents,
    Settings,
}

impl Tab {
    fn next(self) -> Tab {
        match self {
            Tab::New | Tab::Notes => Tab::Threads,
            Tab::Threads => Tab::Agents,
            Tab::Agents => Tab::Settings,
            Tab::Settings => Tab::New,
        }
    }
    fn prev(self) -> Tab {
        match self {
            Tab::New | Tab::Notes => Tab::Settings,
            Tab::Threads => Tab::New,
            Tab::Agents => Tab::Threads,
            Tab::Settings => Tab::Agents,
        }
    }
}

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

#[derive(Clone, Copy, PartialEq, Eq)]
enum Field {
    /// "view drafts (n)" above the title (only with 1+ drafts).
    Drafts,
    Title,
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
    /// Index into sprites::sets(). ←→ on the sprite field + r stay in this set.
    set: usize,
    sprite: Sprite,
    field: usize,
    save: bool,
    error: Option<String>,
    areas: [Rect; FORM_FIELDS - 1],
}

const F_NAME: usize = 0;
const F_HARNESS: usize = 1;
const F_MODEL: usize = 2;
const F_EFFORT: usize = 3;
const F_COLOR: usize = 4;
const F_MODE: usize = 5;
const F_STYLE: usize = 6;
const F_SET: usize = 7;
const F_SPRITE: usize = 8;
const F_VOICE: usize = 9;
const F_PROMPT: usize = 10;
const F_BUTTONS: usize = 11;
const FORM_FIELDS: usize = 12;

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
            color = a.color.as_deref().and_then(|c| agents::COLORS.iter().position(|x| *x == c)).unwrap_or(4);
            mode = agents::MODES.iter().position(|m| *m == a.mode).unwrap_or(0);
        } else {
            harness.set_text("claude");
        }
        voice.set_text(&profile.voice);
        prompt.set_text(&profile.prompt);
        let style = opts.iter().position(|(id, _)| *id == profile.style).unwrap_or(0);
        // saved, else what the agent tab shows (stable pick), else (new agent) random of DEFAULT_SET
        let sprite = profile.sprite.clone().unwrap_or_else(|| match &edit {
            Some(a) => sprites::default_for(&a.name),
            None => sprites::pool()[sprites::random(Some(sprites::DEFAULT_SET), None)].clone(),
        });
        let set = sprites::set_of(&sprite.id).and_then(|s| sprites::sets().iter().position(|x| x == s)).unwrap_or(0);
        AgentForm {
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
            set,
            sprite,
            field: 0,
            save: true,
            error: None,
            areas: [Rect::default(); FORM_FIELDS - 1],
        }
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

    fn set_name(&self) -> &'static str {
        sprites::sets().get(self.set).map_or("", |s| s.as_str())
    }

    /// ←→ on the set field: next / previous set, its 1st sprite.
    fn step_set(&mut self, d: isize) {
        let n = sprites::sets().len().max(1) as isize;
        self.set = (self.set as isize + d).rem_euclid(n) as usize;
        if let Some(&i) = sprites::members(self.set_name()).first() {
            self.sprite = sprites::pool()[i].clone();
        }
    }

    /// ←→ on the sprite field: next / previous sprite of the set (a custom one or 1 of another set: the set start).
    fn step_sprite(&mut self, d: isize) {
        let m = sprites::members(self.set_name());
        if m.is_empty() {
            return;
        }
        let k = match sprites::index_of(&self.sprite.id).and_then(|i| m.iter().position(|&x| x == i)) {
            Some(k) => (k as isize + d).rem_euclid(m.len() as isize) as usize,
            None => 0,
        };
        self.sprite = sprites::pool()[m[k]].clone();
    }

    /// r: random sprite of the set.
    fn random_sprite(&mut self) {
        self.sprite = sprites::pool()[sprites::random(Some(self.set_name()), sprites::index_of(&self.sprite.id))].clone();
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
    task_agents: Rect,
    /// Checkboxes of the opts row: (area, 0 checkout / 1 stash).
    opts: Vec<(Rect, usize)>,
    /// "view drafts (n)" field, and the rows of its open dropdown (area, draft).
    drafts: Rect,
    draft_rows: Vec<(Rect, usize)>,
}

type AgentSnapshot = (Vec<Agent>, String, HashMap<String, Vec<Use>>);

struct AgentGrid {
    card_width: u16,
    heads: Vec<(usize, String)>,
    places: Vec<(usize, u16, usize)>,
    height: usize,
}

pub struct App {
    pub cfg: Config,
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
    /// Input of the notes filter (undo / redo, cursor); FILTER holds its text.
    filter_ta: TextArea,
    tab: Tab,
    settings: Option<crate::settings::Settings>,
    tabs_focus: bool,
    worktrees: Option<Vec<Worktree>>,
    tcursor: usize,
    toffset: usize,
    tsel: HashSet<String>,
    /// Threads tab filter (fuzzy, thread names). Some = row shows text.
    tfilter: Option<String>,
    tfilter_editing: bool,
    /// Input of the threads filter; TFILTER holds its text.
    tfilter_ta: TextArea,
    tpreview: HashMap<String, Vec<Line<'static>>>,
    thread_reads: Reads<(), Vec<Worktree>>,
    session_reads: Reads<(), Vec<Session>>,
    agent_reads: Reads<(), AgentSnapshot>,
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
    where_i: usize,
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
    /// Prompt pastes: images, long texts (files, listed in the message).
    paster: Paster,
    files: Vec<(String, PathBuf)>,
    mention: Mention,
    /// `>` dropdown of the prompt: commands (mention a file / note / thread / artifact).
    command: Command,
    /// Path dropdown of the prompt: the entries of the directory a typed path names.
    paths: crate::paths::Paths,
    /// This task's draft file (id), and its content at the last write.
    draft_id: String,
    draft_saved: String,
    /// The other drafts (oldest first), and the open drafts dropdown: cursor, 1st row in view.
    drafts: Vec<Draft>,
    drafts_menu: Option<(usize, usize)>,
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
    pub suspend: Option<Suspend>,
    /// Time of the last plain esc, and whether it came on the start screen (a main tab, 0 local
    /// controls). A 2nd esc within DOUBLE_ESC: start screen → quit, else → close local controls.
    last_esc: Option<(Instant, bool)>,
    /// A key or paste (not esc) in the settings tab since it opened: its path suggestions came
    /// from typing, so their esc does not count for esc esc.
    settings_typed: bool,
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
    };
    Worktree { path, branch: if c.branch.is_empty() { "in place".into() } else { c.branch.clone() }, thread: Some(thread), unpushed: 0, when: String::new(), linked: !c.branch.is_empty() }
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
        KeyCode::Char(c) => s.push(c.to_ascii_lowercase()),
        KeyCode::Esc => s.push_str("esc"),
        KeyCode::Enter => s.push_str("enter"),
        _ => s.push('?'),
    }
    s
}

fn hint(key: &str, label: &str) -> Vec<Span<'static>> {
    vec![Span::styled(key.to_string(), theme::accent()), Span::styled(format!(" {label}  "), theme::muted())]
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

/// A row of the threads list.
enum TRow {
    Gap,
    Label(String),
    /// Position in `tvisible`.
    Item(usize),
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
        let notes = Notes::load(&cfg.notes);
        let mut t = TextArea::new(true, "what the thread is about (also names the worktree)");
        t.set_text(title);
        let p = TextArea::new(false, "free text for the agents, sent below the list of notes");
        let styles = styles::load(&styles::dir());
        let global_style = styles::global_name();
        // Profile synchronization writes prompts: use current profiles for this step.
        let mut agents = agents::load();
        let synced = agents::sync(&agents, &styles, global_style.as_deref());
        if !synced.is_empty() {
            agents = agents::load();
        }
        let mut picks: Vec<String> = cfg.picks.iter().filter(|n| agents.iter().any(|a| &a.name == *n)).cloned().collect();
        if picks.is_empty() && agents.iter().any(|a| a.name == cfg.agent) {
            picks.push(cfg.agent.clone());
        }
        // where: current branch, new branch, new branch + worktree, default branch (main, else master),
        // the other branches (newest commit first)
        let (mut wheres, mut checkouts, mut repo_branch) = (Vec::new(), HashMap::new(), String::new());
        let (mut here_branch, mut here_dir) = (String::new(), String::new());
        if let Some(repo) = &cfg.repo {
            let bs = threads::branches(repo);
            repo_branch = threads::current_branch(repo);
            here_branch = threads::current_branch(&cfg.cwd);
            here_dir = threads::git(&cfg.cwd, &["rev-parse", "--show-toplevel"]).1.trim().to_string();
            if here_dir.is_empty() {
                here_dir = repo.display().to_string();
            }
            let default = ["main", "master"].into_iter().find(|d| bs.iter().any(|b| b == d)).map(String::from);
            wheres.extend([Where::Current, Where::NewBranch, Where::NewWorktree]);
            if let Some(d) = default.as_ref().filter(|d| **d != here_branch) {
                wheres.push(Where::Branch(d.clone()));
            }
            wheres.extend(bs.into_iter().filter(|b| *b != here_branch && Some(b) != default.as_ref()).map(Where::Branch));
            checkouts = threads::checkouts(repo);
        }
        let home = PathBuf::from(std::env::var("HOME").unwrap_or_default());
        let paster = Paster::new(home.join(".cache/codehead/paste").join(format!("new-{}", (crate::engine::now_ms() / 1000))));
        // -b: new worktree on that branch
        let where_i = if cfg.branch.is_some() { wheres.iter().position(|w| *w == Where::NewWorktree).unwrap_or(0) } else { 0 };
        let mut app = App {
            wheres,
            where_i,
            checkouts,
            repo_branch,
            here_branch,
            here_dir,
            checkout: true,
            stash: false,
            opt_i: 0,
            paster,
            files: Vec::new(),
            mention: Mention::default(),
            command: Command::new(&cfg.cwd),
            paths: crate::paths::Paths::new(&cfg.cwd),
            draft_id: drafts::new_id(),
            draft_saved: String::new(),
            drafts: Vec::new(),
            drafts_menu: None,
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
            tab: if cfg.copy_mode { Tab::Notes } else { Tab::New },
            settings: None,
            tabs_focus: false,
            worktrees: Some(background::snapshot(&format!("threads-{}", background::scope(cfg.repo.as_deref().unwrap_or(&cfg.cwd)))).unwrap_or_default()),
            tcursor: 0,
            toffset: 0,
            tsel: HashSet::new(),
            tfilter: None,
            tfilter_editing: false,
            tfilter_ta: TextArea::new(true, ""),
            tpreview: HashMap::new(),
            thread_reads: Reads::default(),
            session_reads: Reads::default(),
            agent_reads: Reads::default(),
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
            field: if title.trim().is_empty() { Field::Title } else { Field::Prompt },
            target: None,
            modal: None,
            menu: None,
            menu_cmds: Vec::new(),
            menu_help: Vec::new(),
            toast: None,
            regions: Regions::default(),
            outcome: None,
            suspend: None,
            last_esc: None,
            settings_typed: false,
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

    /// Threads tab items after the filter: indices into worktrees ++ sessions.
    /// Order: section (threads, worktrees without thread, Claude Code), then match score.
    fn tvisible(&self) -> Vec<usize> {
        let wts = self.worktrees.as_deref().unwrap_or(&[]);
        let nw = wts.len();
        let q = self.tfilter.as_deref().unwrap_or("").trim().to_lowercase();
        let name = |i: usize| -> String {
            if i < nw {
                let w = &wts[i];
                format!("{} {}", w.thread.as_ref().map_or("", |t| t.title.lines().next().unwrap_or_default()), w.label())
            } else {
                let c = &self.sessions[i - nw];
                format!("{} {}", c.title, c.place())
            }
        };
        let section = |i: usize| -> u8 {
            match wts.get(i) {
                Some(w) if w.thread.is_some() => 0,
                Some(_) => 1,
                None => 2,
            }
        };
        let mut v: Vec<(u8, i64, usize)> = (0..nw + self.sessions.len())
            .filter_map(|i| if q.is_empty() { Some((section(i), 0, i)) } else { fuzzy(&q, &name(i)).map(|sc| (section(i), -sc, i)) })
            .collect();
        v.sort();
        v.into_iter().map(|x| x.2).collect()
    }

    /// Rows of the threads list: a label above each non-empty section, then its items (positions in tvisible).
    fn trows(&self, vis: &[usize]) -> Vec<TRow> {
        let wts = self.worktrees.as_deref().unwrap_or(&[]);
        let sec = |i: usize| -> u8 {
            match wts.get(i) {
                Some(w) if w.thread.is_some() => 0,
                Some(_) => 1,
                None => 2,
            }
        };
        let mut rows = Vec::new();
        let mut last = None;
        for (pos, &i) in vis.iter().enumerate() {
            let s = sec(i);
            if last != Some(s) {
                if last.is_some() { rows.push(TRow::Gap); }
                let n = vis.iter().filter(|&&j| sec(j) == s).count();
                rows.push(TRow::Label(match s {
                    0 => format!("threads · {n}"),
                    1 => format!("worktrees without thread · {n}"),
                    _ => format!("claude code sessions · {n} · enter: new thread, last response as context"),
                }));
                last = Some(s);
            }
            rows.push(TRow::Item(pos));
        }
        rows
    }

    fn n_threads(&self) -> usize {
        self.tvisible().len()
    }

    fn current_wt(&self) -> Option<&Worktree> {
        let i = *self.tvisible().get(self.tcursor)?;
        self.worktrees.as_ref()?.get(i)
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
        let unsent = !self.title.text().trim().is_empty() || !self.prompt.text().trim().is_empty();
        if !unsent || self.last_ctrl_c.is_some_and(|t| t.elapsed() < Duration::from_secs(1)) {
            self.outcome = Some(Outcome::Quit);
            return;
        }
        self.last_ctrl_c = Some(Instant::now());
        self.toast("ctrl+c again: quit (the task text is not sent)", false);
    }

    fn toast(&mut self, msg: impl Into<String>, err: bool) {
        self.toast = Some((msg.into(), Instant::now(), err));
    }

    /// Background reads in. True: something on screen may change (a redraw is due).
    pub fn tick(&mut self) -> bool {
        let mut dirty = self.previews.poll();
        let keep = self.current_wt().map(|w| w.key()).or_else(|| self.current_session().map(|s| s.key()));
        let mut changed = false;
        for (_, rows) in self.thread_reads.poll() {
            self.worktrees = Some(rows);
            changed = true;
            let keys: HashSet<String> = self.worktrees.iter().flatten().map(|w| w.key()).collect();
            self.tsel.retain(|k| keys.contains(k));
        }
        for (_, rows) in self.session_reads.poll() {
            self.sessions = rows;
            changed = true;
        }
        dirty |= changed;
        if changed {
            let vis = self.tvisible();
            let nw = self.n_worktrees();
            self.tcursor = keep.and_then(|key| vis.iter().position(|&i| {
                if i < nw { self.worktrees.as_ref().unwrap()[i].key() == key } else { self.sessions[i - nw].key() == key }
            })).unwrap_or_else(|| self.tcursor.min(vis.len().saturating_sub(1)));
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
        self.field = if self.title.is_empty() { Field::Title } else { Field::Prompt };
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
        if self.tcursor + 1 < self.n_threads() {
            self.tcursor += 1;
            self.pscroll = 0;
        }
    }

    fn in_context(&self, key: &str) -> bool {
        self.carry.iter().any(|c| c.key() == key)
    }

    fn open_thread(&mut self) {
        if let Some(s) = self.current_session().cloned() {
            return self.pick_session(s);
        }
        let Some(w) = self.current_wt().cloned() else { return };
        if w.thread.is_some() {
            self.outcome = Some(Outcome::Resume(w));
        } else {
            self.target = Some(Target::Worktree { path: w.path, branch: w.branch });
            self.open_compose();
        }
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
        let s = slug(&self.title.text());
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

    /// ←→ on the where row. A target from the threads tab gives way to the choices.
    fn step_where(&mut self, d: isize) {
        let n = self.wheres.len();
        if n == 0 {
            return;
        }
        if self.target.take().is_some() {
            return;
        }
        self.where_i = (self.where_i as isize + d).rem_euclid(n as isize) as usize;
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

    /// Thread message: title (line 1 = thread title), the notes to read, the Claude Code context, then the prompt.
    pub fn message(&self) -> String {
        let mut parts = vec![self.title.text().trim().to_string()];
        if !self.selected.is_empty() {
            let list: Vec<String> = self.selected.iter().map(|p| format!("- {}", self.notes.abs(p).display())).collect();
            parts.push(format!("Read these notes first:\n{}", list.join("\n")));
        }
        parts.extend(self.carry.iter().map(claude::context));
        let prompt = self.prompt.text();
        if !prompt.trim().is_empty() {
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
        if self.title.text().trim().is_empty() {
            self.field = Field::Title;
            self.toast("title required", true);
            return;
        }
        match self.target() {
            Ok(_) if self.picks.is_empty() => {
                self.field = Field::Agents;
                self.toast("pick 1+ agents (space)", true);
            }
            Ok(target) => {
                let agents = self.agents.iter().filter(|a| self.picks.contains(&a.name)).map(|a| a.name.clone()).collect();
                self.outcome = Some(Outcome::New { message: self.message(), target, agents })
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
        self.draft_saved = self.draft().content();
        self.field = Field::Title;
    }

    fn open_drafts(&mut self) {
        self.drafts = drafts::load(&self.cfg.drafts, &self.draft_id);
        if self.drafts.is_empty() {
            self.field = Field::Title;
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
        match k.code {
            KeyCode::Esc => self.drafts_menu = None,
            KeyCode::Up | KeyCode::Char('k') => set(self, (cur + n - 1) % n),
            KeyCode::Down | KeyCode::Char('j') => set(self, (cur + 1) % n),
            KeyCode::Enter => self.load_draft(cur),
            KeyCode::Char('d') | KeyCode::Delete => {
                let d = self.drafts.remove(cur.min(n - 1));
                match trash(&drafts::path(&self.cfg.drafts, &d.id)) {
                    Ok(how) => self.toast(format!("draft {how}: {}", d.name()), false),
                    Err(e) => self.toast(format!("draft not deleted: {e}"), true),
                }
                if self.drafts.is_empty() {
                    self.drafts_menu = None;
                    self.field = Field::Title;
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
        let count = |pre: &str| self.files.iter().filter(|(m, _)| m.starts_with(pre)).count();
        self.paster.images = self.paster.images.max(count("[Image #"));
        self.paster.texts = self.paster.texts.max(count("[Pasted text #"));
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
        let rows: Vec<(String, String)> = self.drafts.iter().map(|d| (d.name(), format!("{} · {}", d.place, crate::notes::ago(std::time::UNIX_EPOCH + Duration::from_secs(d.saved))))).collect();
        let w = rows.iter().map(|(a, b)| a.width().min(60) + b.width() + 5).max().unwrap_or(0).clamp(44, 96) as u16;
        let Some(r) = dropdown::place(self.regions.drafts, w, n, bounds, false) else { return };
        let shown = (r.height as usize).saturating_sub(2);
        let off = dropdown::fit(off, cur, n, shown);
        self.drafts_menu = Some((cur, off));
        let inner = dropdown::frame(f, r, "drafts", "enter: load · d: delete · esc: back", off > 0, off + shown < n, true);
        for (i, (name, detail)) in rows.iter().enumerate().skip(off).take(shown) {
            let row = Rect { y: inner.y + (i - off) as u16, height: 1, ..inner };
            let hl = i == cur;
            let st = if hl { theme::pill().add_modifier(Modifier::BOLD) } else { Style::default().fg(theme::TEXT) };
            let dim = if hl { theme::pill() } else { theme::muted() };
            let room = (row.width as usize).saturating_sub(detail.width() + 4);
            let name: String = if name.width() > room { name.chars().take(room.saturating_sub(1)).chain(['…']).collect() } else { name.clone() };
            let pad = room.saturating_sub(name.width());
            let line = Line::from(vec![Span::styled(format!(" {name}"), st), Span::styled(" ".repeat(pad + 2), st), Span::styled(format!("{detail} "), dim)]);
            f.render_widget(Paragraph::new(line).style(st), row);
            self.regions.draft_rows.push((row, i));
        }
    }

    pub fn set_prompt(&mut self, text: &str) {
        self.prompt.set_text(text);
        self.backup();
    }

    /// Sprite rows of the open agent form (for $EDITOR).
    pub fn sprite_text(&self) -> String {
        match &self.modal {
            Some(Modal::Agent(form)) => form.sprite.rows.iter().map(|r| r.trim_end()).collect::<Vec<_>>().join("\n") + "\n",
            _ => String::new(),
        }
    }

    /// Edited sprite back from $EDITOR: first 4 lines, each cut / padded to 7 cells.
    pub fn set_sprite(&mut self, text: &str) {
        if let Some(Modal::Agent(form)) = &mut self.modal {
            let s = sprites::from_text("custom", text);
            form.sprite = sprites::pool().iter().find(|p| p.rows == s.rows).cloned().unwrap_or(s);
        }
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
                    c("ctrl+r", "reset: where from the where row, 0 notes, 0 Claude Code context", Act::Reset),
                    c("esc esc", "quit", Act::Quit),
                ],
                vec![
                    ("↑↓", "field (↑ on the prompt's 1st row leaves it)"),
                    ("tab / shift+tab", "next / previous tab"),
                    ("enter", "title: next field · notes: pick · prompt: new line"),
                    ("x on notes", "drop notes + Claude Code context"),
                    ("←→", "where: current branch · new branch · new worktree · main · recent branches"),
                    ("←→ space", "below where: checkout · git stash boxes"),
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
                    c("f", "filter by name + content", Act::Filter),
                    c("d", "delete the note / folder under the cursor", Act::DeleteNote),
                    c("e", "edit the note in $EDITOR", Act::EditNote),
                    c("y", "copy the picked note paths", Act::CopyPaths),
                    c("x", "clear the selection", Act::ClearSelection),
                    c("c", "collapse all folders", Act::CollapseAll),
                    c("o", "expand all folders", Act::ExpandAll),
                    c("r", "reload from disk", Act::Reload),
                    c("q", "quit", Act::Quit),
                ],
                vec![
                    ("↑↓", "move · ↑ on top row: tabs"),
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
                    c("f", "filter thread names (fuzzy)", Act::ThreadFilter),
                    c("d", "delete thread + worktree (+ branch, if pushed)", Act::DeleteThreads),
                    c("r", "reload", Act::Reload),
                    c("q", "quit", Act::Quit),
                ],
                vec![
                    ("↑↓", "move · ↑ on top row: tabs"),
                    ("space", "select thread / worktree"),
                    ("enter", "open thread (new task if none)"),
                    ("f", "filter: enter / esc back to the list"),
                    ("enter on claude", "new thread, last response as context"),
                    ("c", "thread / claude session: last response in / out of the task context"),
                    ("tab / shift+tab", "switch tab"),
                    ("pgup pgdn", "scroll preview"),
                    ("esc", "clear filter"),
                ],
            ),
            (Screen::Pick, Tab::Agents | Tab::New) => (
                vec![
                    c("f", "filter agents (fuzzy: name, harness, model)", Act::AgentFilter),
                    c("n", "new agent", Act::NewAgent),
                    c("e", "edit agent (also enter)", Act::EditAgent),
                    c("d", "delete agent: its threads move to the default agent", Act::DeleteAgent),
                    c("r", "reload", Act::Reload),
                    c("q", "quit", Act::Quit),
                ],
                vec![
                    ("↑↓←→", "move in the card grid · ↑ on top row: tabs"),
                    ("enter", "edit agent"),
                    ("f", "filter: enter / esc back to the cards"),
                    ("tab / shift+tab", "switch tab"),
                    ("pgup pgdn", "scroll preview"),
                    ("esc", "close local controls"),
                ],
            ),
            (Screen::Pick, Tab::Settings) => (
                vec![c("ctrl+n", "add path alias", Act::AddAlias), c("ctrl+r", "reload file", Act::Reload), c("q", "quit", Act::Quit)],
                vec![("↑↓", "field / alias box"), ("enter", "alias box: edit / done"), ("d", "delete the highlighted alias"), ("tab / shift+tab", "switch tab"), ("esc", "close path suggestions"), ("auto", "changes apply on leaving a field")],
            ),
        }
    }

    fn open_menu(&mut self) {
        let (cmds, help) = self.commands();
        let mut items: Vec<crate::menu::Item> =
            cmds.iter().enumerate().map(|(i, c)| crate::menu::Item { label: c.label.to_string(), key: c.key.to_string(), id: i }).collect();
        items.push(crate::menu::Item { label: "keys of this screen".into(), key: String::new(), id: KEYS_ITEM });
        self.menu = Some(crate::menu::Menu::new(items, None, Vec::new(), self.agents.clone(), Vec::new()));
        self.menu_cmds = cmds;
        self.menu_help = help;
    }

    /// A start screen tab, from the menu (here or in the thread view).
    pub fn go_home(&mut self, h: crate::menu::Home) {
        use crate::menu::Home;
        self.modal = None;
        self.drafts_menu = None;
        self.switch_tab(match h {
            Home::New => Tab::New,
            Home::Settings => Tab::Settings,
            Home::Agents => Tab::Agents,
        });
        if h != Home::Settings {
            self.tabs_focus = false;
        }
    }

    fn menu_action(&mut self, a: crate::menu::Action) {
        use crate::menu::Action;
        match a {
            Action::None => return,
            Action::Close | Action::Remove(_) | Action::Add(_) => {}
            Action::Home(h) => self.go_home(h),
            Action::Open(c) => self.outcome = Some(Outcome::Resume(worktree_of(&c))),
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
            Act::AddAlias => self.settings_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::CONTROL)),
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
            Act::ThreadFilter => {
                self.switch_tab(Tab::Threads);
                self.tabs_focus = false;
                if self.tfilter.is_none() {
                    self.tfilter = Some(String::new());
                }
                if self.tfilter_ta.text() != self.tfilter.as_deref().unwrap_or("") {
                    self.tfilter_ta.set_text(self.tfilter.as_deref().unwrap_or(""));
                }
                self.tfilter_ta.move_end();
                self.tfilter_editing = true;
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
                    Tab::Agents => self.load_agents(),
                    Tab::Notes | Tab::New => self.reload_notes(),
                    Tab::Settings => { self.settings_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL)); return; }
                }
                self.toast("reloaded", false);
            }
            Act::DeleteThreads => {
                let Some(all) = &self.worktrees else { return };
                let items: Vec<Worktree> = if self.tsel.is_empty() {
                    self.current_wt().cloned().into_iter().collect()
                } else {
                    all.iter().filter(|w| self.tsel.contains(&w.key())).cloned().collect()
                };
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
            Act::Reset => {
                self.target = None;
                self.carry.clear();
                self.selected.clear();
                self.toast("reset: where row, 0 notes, 0 context", false);
            }
            Act::Quit => self.outcome = Some(Outcome::Quit),
        }
    }

    fn confirmed(&mut self, action: ConfirmAction) {
        match action {
            ConfirmAction::Note(path) => match trash(&self.notes.abs(&path)) {
                Ok(how) => {
                    self.selected.retain(|s| !(s == &path || (path.ends_with('/') && s.starts_with(&path))));
                    self.reload_notes();
                    self.toast(format!("{path}: {how}"), false);
                }
                Err(e) => self.toast(format!("{path}: {e}"), true),
            },
            ConfirmAction::Threads(items) => {
                let repo = self.cfg.repo.clone().unwrap_or_else(|| self.cfg.cwd.clone());
                let lines = threads::remove(&repo, &items, &self.cfg.logs);
                self.tsel.clear();
                self.load_threads();
                self.modal = Some(Modal::Info { title: "delete threads".into(), lines, scroll: 0 });
            }
            ConfirmAction::Agent(a) => {
                let Some(fallback) = self.agents.iter().find(|x| x.name == self.cfg.agent).cloned() else { return };
                let uses = self.uses(&a);
                let lines = agents::delete(&a, &fallback, &uses, &self.cfg.logs);
                self.load_agents();
                self.worktrees = None;
                self.modal = Some(Modal::Info { title: "delete agent".into(), lines, scroll: 0 });
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
                let mut msg = format!("{} {}", spec.name, if old_name.is_some() { "saved" } else { "created" });
                // live sessions keep the profile they started with: restart them on the new one
                if let Some(old) = &form.edit
                    && let Some(new) = self.agents.iter().find(|a| a.name == spec.name).cloned()
                    && agents::profile_changed(old, &new)
                {
                    let uses = agents::usage().remove(&new.name).unwrap_or_default();
                    let r = agents::restart(&new, old.harness == new.harness, &uses, &self.cfg.logs);
                    msg.push_str(&restart_note(&r));
                    self.usage = None;
                    self.load_agents();
                }
                self.toast(msg, false);
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
        // F2 menu: over everything, its keys first (1 esc closes it)
        if let Some(m) = self.menu.as_mut() {
            self.last_esc = None;
            let a = m.key(k);
            return self.menu_action(a);
        }
        if k.code == KeyCode::F(2) {
            self.last_esc = None;
            return self.open_menu();
        }
        // drafts dropdown: its keys first (esc: back to the task, 0 double esc)
        if self.drafts_menu.is_some() && self.modal.is_none() {
            self.last_esc = None;
            return self.drafts_key(k);
        }
        // @ and > dropdowns of the prompt: their keys first (esc closes them, 0 double esc)
        if self.mention_on() {
            let names = self.agent_names();
            // skills: of the picked agents' harnesses (0 picked: of every agent's)
            let picked = |a: &&Agent| self.picks.is_empty() || self.picks.contains(&a.name);
            let mut harnesses: Vec<String> = self.agents.iter().filter(picked).map(|a| a.harness.clone()).collect();
            harnesses.sort();
            harnesses.dedup();
            self.command.harnesses = harnesses;
            if self.mention.key(k, &mut self.prompt, &names) || self.command.key(k, &mut self.prompt) || self.paths.key(k, &mut self.prompt) {
                self.last_esc = None;
                return;
            }
        }
        if k.code == KeyCode::Esc && k.modifiers.is_empty() {
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
        if self.tab == Tab::Settings && k.code != KeyCode::Esc { self.settings_typed = true; }
        if self.modal.is_some() {
            self.modal_key(k);
            return;
        }
        if ctrl && matches!(k.code, KeyCode::Char('c')) {
            return self.ctrl_c();
        }
        match self.screen {
            Screen::Compose if self.tabs_focus => self.tabs_key(k),
            Screen::Compose => self.compose_key(k),
            Screen::Pick => {
                if self.tab == Tab::Settings && !self.tabs_focus {
                    self.settings_key(k);
                    return;
                }
                if self.preview_key(k) {
                    return;
                }
                if self.tabs_focus {
                    self.tabs_key(k);
                } else {
                    match self.tab {
                        Tab::Notes => self.notes_key(k),
                        Tab::Threads => self.threads_key(k),
                        Tab::Agents | Tab::New => self.agents_key(k),
                        Tab::Settings => self.settings_key(k),
                    }
                }
            }
        }
    }

    /// The start screen: 1 of its main tabs (task, threads, agents, settings), 0 local controls
    /// for an Esc to close (modal, filter input, thread filter, the notes list of the task).
    fn at_start(&self) -> bool {
        let notes = self.screen == Screen::Pick && self.tab == Tab::Notes;
        self.modal.is_none() && self.drafts_menu.is_none() && !notes
            && !self.filter_editing && !self.tfilter_editing && !self.afilter_editing
            && !(self.screen == Screen::Pick && self.tab == Tab::Threads && self.tfilter.as_deref().is_some_and(|q| !q.is_empty()))
            && !(self.screen == Screen::Pick && self.tab == Tab::Agents && self.afilter.as_deref().is_some_and(|q| !q.is_empty()))
    }

    /// Close local controls without changing the current tab.
    fn dismiss_controls(&mut self) {
        self.modal = None;
        self.drafts_menu = None;
        self.filter_editing = false;
        self.tfilter_editing = false;
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
        let mut groups: std::collections::BTreeMap<String, Vec<usize>> = Default::default();
        for (i, a) in self.agents.iter().enumerate() {
            if !q.is_empty() && fuzzy(&q, &format!("{} {} {}", a.name, a.harness, agents::model_of(a))).is_none() {
                continue;
            }
            groups.entry(a.harness.clone()).or_default().push(i);
        }
        groups.into_iter().map(|(h, mut g)| {
            g.sort_by_key(|&i| self.agents[i].name.to_lowercase());
            (h, g)
        }).collect()
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

    fn sync_tfilter(&mut self) {
        let q = self.tfilter_ta.text();
        if self.tfilter.as_deref() != Some(q.as_str()) {
            self.tfilter = Some(q);
            self.tcursor = 0;
        }
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
            Pasted::Text(t) => self.prompt.insert(&t),
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
        // leaving the settings: their last edit applies
        if tab != Tab::Settings && self.screen == Screen::Pick && self.tab == Tab::Settings {
            if let Some(s) = self.settings.as_mut() { s.commit(); }
            self.apply_settings();
        }
        if tab == Tab::New {
            self.screen = Screen::Compose;
            return;
        }
        self.screen = Screen::Pick;
        if tab == Tab::Threads {
            self.refresh_threads();
        }
        if tab == Tab::Agents {
            self.refresh_agents();
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
            Action::NextTab => self.switch_tab(Tab::New),
            Action::PrevTab => self.switch_tab(Tab::Agents),
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
        if (self.filter_editing || self.tfilter_editing || self.afilter_editing) && !matches!(k.code, KeyCode::PageUp | KeyCode::PageDown) {
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
            KeyCode::Right | KeyCode::Char('l') | KeyCode::Tab => self.switch_tab(self.cur_tab().next()),
            KeyCode::Left | KeyCode::Char('h') | KeyCode::BackTab => self.switch_tab(self.cur_tab().prev()),
            KeyCode::Down | KeyCode::Enter | KeyCode::Char('j') | KeyCode::Esc => {
                self.tabs_focus = false;
                if self.screen == Screen::Compose {
                    self.field = Field::Title;
                } else if self.tab == Tab::Settings
                    && let Some(settings) = &mut self.settings { settings.focus_input(); }
            }
            KeyCode::Char('q') => self.outcome = Some(Outcome::Quit),
            _ => {}
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
            KeyCode::Char('/') => self.run(Act::Filter),
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
            KeyCode::Tab => self.switch_tab(Tab::Threads),
            KeyCode::BackTab => self.switch_tab(Tab::Settings),
            _ => {
                let name = key_name(&k);
                if let Some(c) = self.commands().0.into_iter().find(|c| c.key == name) {
                    self.run(c.act);
                }
            }
        }
    }

    fn threads_key(&mut self, k: KeyEvent) {
        let n = self.n_threads();
        if self.tfilter_editing {
            match k.code {
                KeyCode::Esc | KeyCode::Enter => self.tfilter_editing = false,
                KeyCode::Up => self.tcursor = self.tcursor.saturating_sub(1),
                KeyCode::Down => self.tcursor = (self.tcursor + 1).min(n.saturating_sub(1)),
                _ => {
                    if matches!(self.tfilter_ta.key(k), Input::Handled) {
                        self.sync_tfilter();
                    }
                }
            }
            self.pscroll = 0;
            return;
        }
        match k.code {
            KeyCode::Up | KeyCode::Char('k') => {
                if self.tcursor == 0 {
                    self.tabs_focus = true;
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
            KeyCode::Char(' ') => {
                if let Some(w) = self.current_wt() {
                    let p = w.key();
                    if !self.tsel.remove(&p) {
                        self.tsel.insert(p);
                    }
                    if self.tcursor + 1 < n {
                        self.tcursor += 1;
                    }
                }
            }
            KeyCode::Enter => self.open_thread(),
            KeyCode::Char('c') => self.toggle_context(),
            KeyCode::Tab => self.switch_tab(Tab::Agents),
            KeyCode::Esc if self.tfilter.as_deref().is_some_and(|q| !q.is_empty()) => {
                self.tfilter = None;
                self.tcursor = 0;
            }
            KeyCode::BackTab => self.switch_tab(Tab::New),
            _ => {
                let name = key_name(&k);
                if let Some(c) = self.commands().0.into_iter().find(|c| c.key == name) {
                    self.run(c.act);
                }
            }
        }
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
            KeyCode::Tab => self.switch_tab(Tab::Settings),
            KeyCode::BackTab => self.switch_tab(Tab::Threads),
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

    fn compose_key(&mut self, k: KeyEvent) {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        let alt = k.modifiers.contains(KeyModifiers::ALT);
        match k.code {
            KeyCode::Esc => return,
            KeyCode::Tab => return self.switch_tab(Tab::Threads),
            KeyCode::BackTab => return self.switch_tab(Tab::Settings),
            // cmd+enter (SUPER) needs a terminal that reports it (kitty keyboard protocol)
            KeyCode::Enter if ctrl || alt || k.modifiers.contains(KeyModifiers::SUPER) => return self.start(),
            KeyCode::Char('o') if ctrl => return self.run(Act::OpenNotes),
            KeyCode::Char('r') if ctrl => return self.run(Act::Reset),
            // ctrl+v / ⌘v in the prompt, like Claude Code: the clipboard image, else its text
            KeyCode::Char('v' | 'V') if self.field == Field::Prompt && (ctrl || k.modifiers.contains(KeyModifiers::SUPER)) => {
                let p = self.paster.clipboard();
                return self.paste_prompt(p);
            }
            KeyCode::Enter | KeyCode::Down if self.field == Field::Title => {
                self.field = if self.wheres.is_empty() { Field::Agents } else { Field::Where };
                return;
            }
            // ↑ from the top field: the drafts row (if any), then the tabs
            KeyCode::Up if self.field == Field::Title => {
                if self.drafts.is_empty() {
                    self.tabs_focus = true;
                } else {
                    self.field = Field::Drafts;
                }
                return;
            }
            KeyCode::Up if self.field == Field::Prompt && !k.modifiers.contains(KeyModifiers::SHIFT) && self.prompt.on_first_row() => {
                self.field = Field::Notes;
                return;
            }
            _ => {}
        }
        if self.field == Field::Drafts {
            match k.code {
                KeyCode::Enter | KeyCode::Char(' ') => self.open_drafts(),
                KeyCode::Down => self.field = Field::Title,
                KeyCode::Up => self.tabs_focus = true,
                _ => {}
            }
            return;
        }
        if self.field == Field::Where {
            match k.code {
                KeyCode::Left | KeyCode::Char('h') => self.step_where(-1),
                KeyCode::Right | KeyCode::Char('l') | KeyCode::Char(' ') => self.step_where(1),
                KeyCode::Up => self.field = Field::Title,
                KeyCode::Enter | KeyCode::Down => self.field = Field::Opts,
                _ => {}
            }
            return;
        }
        if self.field == Field::Opts {
            match k.code {
                KeyCode::Left | KeyCode::Char('h') => self.opt_i = 0,
                KeyCode::Right | KeyCode::Char('l') => self.opt_i = 1,
                KeyCode::Char(' ') => self.toggle_opt(self.opt_i),
                KeyCode::Up => self.field = Field::Where,
                KeyCode::Enter | KeyCode::Down => self.field = Field::Agents,
                _ => {}
            }
            return;
        }
        if self.field == Field::Notes {
            match k.code {
                KeyCode::Enter | KeyCode::Char(' ') => self.open_notes(),
                KeyCode::Up => self.field = Field::Agents,
                KeyCode::Down => self.field = Field::Prompt,
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
                KeyCode::Up | KeyCode::Char('k') => if !step(self, 0, -1) { self.field = if self.wheres.is_empty() { Field::Title } else { Field::Opts }; },
                KeyCode::Down | KeyCode::Char('j') => if !step(self, 0, 1) { self.field = Field::Notes; },
                KeyCode::Enter => self.field = Field::Notes,
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
                KeyCode::Char('n') | KeyCode::Esc => {}
                KeyCode::Enter => {
                    if yes {
                        self.confirmed(action)
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
                match k.code {
                    KeyCode::Esc => return,
                    KeyCode::Char('s') if ctrl => return self.save_agent(form),
                    KeyCode::Enter if ctrl => return self.save_agent(form),
                    KeyCode::Tab => form.field = next(form.field),
                    KeyCode::BackTab => form.field = prev(form.field),
                    KeyCode::Down if !form.multiline() => form.field = next(form.field),
                    KeyCode::Up if !form.multiline() => form.field = prev(form.field),
                    // voice, system prompt: ↑ on the 1st row / ↓ on the last leaves the box (shift: selects)
                    KeyCode::Down if k.modifiers.is_empty() && form.text().is_some_and(|t| t.on_last_row()) => form.field = next(form.field),
                    KeyCode::Up if k.modifiers.is_empty() && form.text().is_some_and(|t| t.on_first_row()) => form.field = prev(form.field),
                    KeyCode::Char('r') if ctrl || matches!(form.field, F_SET | F_SPRITE) => form.random_sprite(),
                    KeyCode::Char('e') if form.field == F_SPRITE && !ctrl => {
                        self.modal = Some(Modal::Agent(form));
                        self.suspend = Some(Suspend::Sprite);
                        return;
                    }
                    KeyCode::Enter if form.field == F_BUTTONS => {
                        if form.save {
                            self.save_agent(form);
                        }
                        return;
                    }
                    KeyCode::Enter if !form.multiline() => form.field = next(form.field),
                    KeyCode::Left | KeyCode::Right if matches!(form.field, F_MODEL | F_EFFORT | F_COLOR | F_MODE | F_STYLE | F_SET | F_SPRITE | F_BUTTONS) => {
                        let d = if k.code == KeyCode::Left { -1 } else { 1 };
                        let cycle = |i: usize, n: usize| ((i as isize + d).rem_euclid(n as isize)) as usize;
                        match form.field {
                            F_MODEL if form.is_claude() => form.model = cycle(form.model, form.models.len()),
                            F_MODEL => {}
                            F_EFFORT => form.effort = cycle(form.effort, agents::EFFORTS.len()),
                            F_COLOR => form.color = cycle(form.color, agents::COLORS.len()),
                            F_MODE => form.mode = cycle(form.mode, agents::MODES.len()),
                            F_STYLE => form.style = cycle(form.style, form.styles.len()),
                            F_SET => form.step_set(d),
                            F_SPRITE => form.step_sprite(d),
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
        self.settings_typed |= self.tab == Tab::Settings;
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
        match self.screen {
            Screen::Compose if self.field == Field::Title => self.title.insert(text),
            // like the thread view: a paste on any other field goes to the prompt
            Screen::Compose => {
                let p = self.paster.paste(text);
                self.paste_prompt(p);
            }
            Screen::Pick if self.tfilter_editing => {
                self.tfilter_ta.insert(text);
                self.sync_tfilter();
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
        self.mouse_inner(m);
        if !matches!(m.kind, MouseEventKind::Moved) {
            self.backup();
        }
    }

    fn mouse_inner(&mut self, m: MouseEvent) {
        let at = Position { x: m.column, y: m.row };
        self.hover = Some(at);
        if let Some(menu) = self.menu.as_mut() {
            let a = menu.mouse(m);
            return self.menu_action(a);
        }
        let down = matches!(m.kind, MouseEventKind::Down(MouseButton::Left));
        if let Some(Modal::Agent(form)) = &mut self.modal {
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
            if let Some(&(_, yes)) = self.regions.buttons.iter().find(|(r, _)| r.contains(at))
                && let Some(Modal::Confirm { action, .. }) = self.modal.take()
                && yes
            {
                self.confirmed(action);
            }
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
            if down && self.regions.drafts.contains(at) {
                self.field = Field::Drafts;
                return self.open_drafts();
            }
            if self.mention_on() {
                let names = self.agent_names();
                if down && self.mention.mouse(m, &mut self.prompt, &names) {
                    return;
                }
                if self.command.mouse(m, &mut self.prompt) || self.paths.mouse(m, &mut self.prompt) {
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
                } else if self.regions.where_row.contains(at) {
                    if self.field == Field::Where {
                        self.step_where(1);
                    }
                    self.field = Field::Where;
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
                } else if let Some(TRow::Item(i)) = {
                    let vis = self.tvisible();
                    self.trows(&vis).into_iter().nth(self.toffset + i)
                } {
                    {
                        if i == self.tcursor {
                            self.open_thread();
                        } else {
                            self.tcursor = i;
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
            let names = self.agent_names();
            self.mention.render(f, &self.prompt, &names, area);
            let avoid = self.prompt.area();
            self.command.render(f, &self.prompt, area, avoid);
            self.paths.render(f, &self.prompt, area, avoid);
        }
        if self.screen == Screen::Compose {
            self.draw_drafts(f, area);
        }
        self.draw_modal(f, area);
        if let Some(m) = self.menu.as_mut() {
            m.render(f, area);
        }
        if let Some(r) = self.hover_rect() {
            theme::hover(f.buffer_mut(), r);
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
            if self.mention_on()
                && let Some(r) = self.mention.row_at(at).or_else(|| self.command.row_at(at)).or_else(|| self.paths.row_at(at))
            {
                return Some(r);
            }
            if self.regions.drafts.contains(at) {
                return Some(self.regions.drafts);
            }
            return find_rows(&self.regions.opts, at)
                .or_else(|| find_rows(&self.regions.menu_rows, at))
                .or_else(|| [self.regions.where_row, self.regions.notes_label].into_iter().find(|r| r.contains(at)));
        }
        let list = self.regions.list;
        if !list.contains(at) {
            return None;
        }
        if self.tab == Tab::Agents {
            return self.acards.iter().find(|(r, _)| r.contains(at)).map(|(r, _)| *r);
        }
        let i = (at.y - list.y) as usize;
        let item = match self.tab {
            Tab::Notes => self.offset + i < self.rows.len(),
            Tab::Agents => false,
            _ => {
                let vis = self.tvisible();
                matches!(self.trows(&vis).get(self.toffset + i), Some(TRow::Item(_)))
            }
        };
        item.then_some(Rect { y: at.y, height: 1, ..list })
    }

    fn draw_header(&mut self, f: &mut Frame, area: Rect) {
        let mut spans = vec![Span::styled(BRAND, theme::accent_bold()), Span::raw(" ")];
        let mut x = area.x + BRAND.width() as u16 + 1;
        if self.screen == Screen::Pick && self.tab == Tab::Notes {
            spans.push(Span::styled(" new › notes ", theme::pill()));
        } else {
            for (tab, label) in [(Tab::New, "new"), (Tab::Threads, "threads"), (Tab::Agents, "agents"), (Tab::Settings, "settings")] {
                let active = self.cur_tab() == tab;
                let focused = active && self.tabs_focus;
                let (bg, fg) = match (active, focused) {
                    (true, true) => (Some(theme::ACCENT), theme::ON_ACCENT),
                    (true, false) => (Some(theme::PANEL), theme::ACCENT),
                    _ => (None, theme::MUTED),
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
        let place = match &self.cfg.repo {
            Some(r) => r.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
            None => self.cfg.cwd.display().to_string(),
        };
        let right = Line::from(vec![Span::styled(self.cfg.agent.clone(), theme::accent()), Span::styled(format!(" · {place} "), theme::muted())]).right_aligned();
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
        if self.tab == Tab::Settings {
            if let Some(settings) = &mut self.settings { settings.draw(f, area, !self.tabs_focus); }
            return;
        }
        let mut area = area;
        if self.tab == Tab::Notes
            && let Some(q) = &self.filter
        {
            let [row, _, rest] = Layout::vertical([Constraint::Length(1), Constraint::Length(1), Constraint::Min(1)]).areas(area);
            let label = Style::default().fg(if self.filter_editing { theme::ACCENT } else { theme::MUTED });
            let count = format!("{} of {} ", self.rows.len(), self.notes.files.len());
            if self.filter_editing {
                f.render_widget(Paragraph::new(Span::styled("filter › ", label)).style(theme::panel()), row);
                self.filter_ta.render(f, filter_input(row, count.width()), true);
            } else {
                let line = Line::from(vec![Span::styled("filter › ", label), Span::styled(q.clone(), theme::text())]);
                f.render_widget(Paragraph::new(line).style(theme::panel()), row);
            }
            f.render_widget(Paragraph::new(Line::styled(count, theme::panel().fg(theme::MUTED)).right_aligned()), row);
            area = rest;
        }
        if self.tab == Tab::Threads {
            let [row, _, rest] = Layout::vertical([Constraint::Length(1), Constraint::Length(1), Constraint::Min(1)]).areas(area);
            let label = Style::default().fg(if self.tfilter_editing { theme::ACCENT } else { theme::MUTED });
            let q = self.tfilter.clone().unwrap_or_default();
            let total = self.n_worktrees() + self.sessions.len();
            let count = if q.is_empty() { String::new() } else { format!("{} of {total} ", self.n_threads()) };
            if self.tfilter_editing {
                f.render_widget(Paragraph::new(Span::styled("filter › ", label)).style(theme::panel()), row);
                self.tfilter_ta.render(f, filter_input(row, count.width()), true);
            } else {
                let mut spans = vec![Span::styled("filter › ", label), Span::styled(q.clone(), theme::text())];
                if q.is_empty() {
                    spans.push(Span::styled("f: fuzzy search thread names", theme::panel().fg(theme::LINE)));
                }
                f.render_widget(Paragraph::new(Line::from(spans)).style(theme::panel()), row);
            }
            if !count.is_empty() {
                f.render_widget(Paragraph::new(Line::styled(count, theme::panel().fg(theme::MUTED)).right_aligned()), row);
            }
            area = rest;
        }
        if matches!(self.tab, Tab::Agents | Tab::New) {
            let [row, _, rest] = Layout::vertical([Constraint::Length(1), Constraint::Length(1), Constraint::Min(1)]).areas(area);
            let label = Style::default().fg(if self.afilter_editing { theme::ACCENT } else { theme::MUTED });
            let q = self.afilter.clone().unwrap_or_default();
            let shown: usize = self.avisible().iter().map(|(_, g)| g.len()).sum();
            let count = if q.is_empty() { String::new() } else { format!("{shown} of {} ", self.agents.len()) };
            if self.afilter_editing {
                f.render_widget(Paragraph::new(Span::styled("filter › ", label)).style(theme::panel()), row);
                self.afilter_ta.render(f, filter_input(row, count.width()), true);
            } else {
                let mut spans = vec![Span::styled("filter › ", label), Span::styled(q.clone(), theme::text())];
                if q.is_empty() {
                    spans.push(Span::styled("f: fuzzy search name, harness, model", theme::panel().fg(theme::LINE)));
                }
                f.render_widget(Paragraph::new(Line::from(spans)).style(theme::panel()), row);
            }
            if !count.is_empty() {
                f.render_widget(Paragraph::new(Line::styled(count, theme::panel().fg(theme::MUTED)).right_aligned()), row);
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
        match self.tab {
            Tab::Notes => self.draw_notes(f, list),
            Tab::Threads => self.draw_threads(f, list),
            Tab::Agents | Tab::New => self.draw_agents(f, list),
            Tab::Settings => {}
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
            let Some(w) = self.current_wt().cloned() else {
                let msg = if self.cfg.repo.is_some() { "0 worktrees" } else { "not in a git repo: 0 worktrees" };
                return ("threads".into(), vec![Line::styled(msg, theme::muted())], true);
            };
            let key = w.key();
            let read = w.clone();
            self.detail_reads.request(key.clone(), move || threads::preview(&read));
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
            let mut lines = vec![Line::from(vec![Span::styled(row.path().to_string(), theme::accent_bold()), Span::styled(format!("  {} notes", files.len()), theme::muted())]), Line::raw("")];
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
        match (hl, self.tabs_focus || self.filter_editing || self.tfilter_editing) {
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
            let fg = |s: Style| if on_pill { s.fg(theme::ON_ACCENT) } else { s };
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

    fn draw_threads(&mut self, f: &mut Frame, area: Rect) {
        let h = area.height as usize;
        let Some(items) = &self.worktrees else { return };
        let vis = self.tvisible();
        if vis.is_empty() {
            let msg = if self.tfilter.as_deref().is_some_and(|q| !q.is_empty()) { "0 matches" } else { "0 threads · 0 worktrees · 0 Claude Code sessions" };
            f.render_widget(Paragraph::new(Line::styled(msg, theme::muted())), area);
            return;
        }
        self.tcursor = self.tcursor.min(vis.len() - 1);
        let rows = self.trows(&vis);
        let cur = rows.iter().position(|r| matches!(r, TRow::Item(p) if *p == self.tcursor)).unwrap_or(0);
        if cur < self.toffset {
            // keep the section label above the 1st item in view
            self.toffset = cur.saturating_sub(usize::from(cur > 0 && matches!(rows[cur - 1], TRow::Label(_))));
        } else if cur >= self.toffset + h {
            self.toffset = cur + 1 - h;
        }
        self.toffset = self.toffset.min(rows.len().saturating_sub(h));
        let nw = items.len();
        let dim = self.tabs_focus || self.tfilter_editing;
        for (r, row) in rows.iter().enumerate().skip(self.toffset).take(h) {
            let rect = Rect { y: area.y + (r - self.toffset) as u16, height: 1, ..area };
            let pos = match row {
                TRow::Gap => continue,
                TRow::Label(text) => {
                    f.render_widget(Paragraph::new(Line::styled(format!(" {text}"), theme::muted().add_modifier(Modifier::BOLD))), rect);
                    continue;
                }
                TRow::Item(p) => *p,
            };
            let i = vis[pos];
            let hl = pos == self.tcursor;
            let on_pill = hl && !dim;
            let style = self.row_style(hl);
            let fg = |s: Style| if on_pill { s.fg(theme::ON_ACCENT) } else { s };
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
                let on = self.tsel.contains(&w.key());
                let mut spans = vec![
                    Span::raw(" "),
                    if on { Span::styled("◼ ", fg(theme::accent())) } else { Span::styled("◻ ", fg(theme::line())) },
                ];
                match &w.thread {
                    Some(t) => {
                        spans.push(Span::styled(t.title.lines().next().unwrap_or_default().chars().take(60).collect::<String>(), fg(theme::bold())));
                        spans.push(Span::styled(format!("  {}", w.label()), muted));
                        if t.live > 0 {
                            spans.push(Span::styled(" live", fg(theme::accent_bold())));
                        }
                        if self.in_context(&t.id) {
                            spans.push(Span::styled(" ↳ context", fg(theme::accent())));
                        }
                        spans.push(Span::styled(format!(" {}", t.state), muted));
                    }
                    None => {
                        spans.push(Span::styled(w.branch.clone(), fg(theme::bold())));
                        spans.push(Span::styled("  no thread", muted));
                    }
                }
                if w.unpushed > 0 {
                    spans.push(Span::styled(format!(" {} unpushed", w.unpushed), fg(Style::default().fg(theme::DANGER))));
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
            let live = if u.live.is_empty() { Span::raw("") } else { Span::styled(" live", theme::accent_bold()) };
            out.push(Line::from(vec![Span::raw(format!("  {}", u.title)), live, Span::styled(format!(" {}", u.state), theme::muted())]));
        }
        out
    }

    /// Shared by the agents tab and task selection.
    fn agent_grid(&self, width: u16) -> AgentGrid {
        let groups = self.avisible();
        let text_w = self.agents.iter().flat_map(|a| self.agent_card_text(a)).map(|t| t.width()).max().unwrap_or(0).clamp(10, 24);
        let card_w = ((text_w + crate::sprites::W + 6) as u16).min(width.max(1));
        let per_row = ((width + 1) / (card_w + 1)).max(1) as usize;
        // virtual layout: per group a heading row, its card rows, 1 blank row
        let mut heads = Vec::new();
        let mut places = Vec::new();
        let mut y = 0usize;
        for (harness, g) in &groups {
            heads.push((y, format!("{harness} · {}", g.len())));
            y += 1;
            for (slot, &i) in g.iter().enumerate() {
                let x = (slot % per_row) as u16 * (card_w + 1);
                places.push((i, x, y + slot / per_row * CARD_H as usize));
            }
            y += g.len().div_ceil(per_row) * CARD_H as usize + 1;
        }
        AgentGrid { card_width: card_w, heads, places, height: y.saturating_sub(1) }
    }

    /// 1 card grid per harness (A→Z), as many columns as fit.
    fn draw_agents(&mut self, f: &mut Frame, area: Rect) {
        self.draw_agent_grid(f, area, false);
    }

    fn draw_agent_grid(&mut self, f: &mut Frame, area: Rect, task: bool) {
        let AgentGrid { card_width: card_w, heads, places, height } = self.agent_grid(area.width);
        let mut cursor = if task { self.pick_cursor } else { self.acursor };
        let mut offset = if task { self.pick_offset } else { self.aoffset };
        if !places.iter().any(|&(i, _, _)| i == cursor) {
            cursor = places.first().map_or(0, |p| p.0);
        }
        if task { self.pick_cursor = cursor; } else { self.acursor = cursor; }
        let view = area.height as usize;
        if let Some(&(_, _, top)) = places.iter().find(|&&(i, _, _)| i == cursor) {
            // the group heading of the 1st card row stays in view with it
            let top = if heads.iter().any(|&(h, _)| h + 1 == top) { top - 1 } else { top };
            if top < offset {
                offset = top;
            } else if top + CARD_H as usize + 1 > offset + view {
                offset = (top + CARD_H as usize + 1).saturating_sub(view);
            }
        }
        offset = offset.min(height.saturating_sub(view));
        if task { self.pick_offset = offset; } else { self.aoffset = offset; }
        let mut cards = Vec::new();
        let shown = |vy: usize, h: usize| vy >= offset && vy + h <= offset + view;
        for (vy, text) in heads {
            if shown(vy, 1) {
                let r = Rect { y: area.y + (vy - offset) as u16, height: 1, ..area };
                f.render_widget(Paragraph::new(Line::styled(text, theme::bold())), r);
            }
        }
        for &(i, x, vy) in &places {
            if !shown(vy, CARD_H as usize) { continue; }
            let r = Rect { x: area.x + x, y: area.y + (vy - offset) as u16, width: card_w.min(area.width.saturating_sub(x)), height: CARD_H };
            let a = self.agents[i].clone();
            let focused = !self.tabs_focus && if task { self.field == Field::Agents } else { !self.afilter_editing };
            if task { self.draw_card(f, r, &a, focused && i == cursor); }
            else { self.draw_agent_card(f, r, &a, focused && i == cursor); }
            cards.push((r, i));
        }
        if places.is_empty() {
            let msg = if self.agents.is_empty() { "0 agents. F2 → n: new agent" } else { "0 agents match the filter" };
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
        let color = agents::color(a.color.as_deref());
        let mut block = Block::new()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(if cur { theme::accent_bold() } else { theme::line() });
        if a.name == self.cfg.agent {
            block = block.title(Span::styled(" default ", theme::accent()));
        }
        let inner = block.inner(r);
        f.render_widget(block, r);
        let [name, tech, effort, threads] = self.agent_card_text(a);
        let side = vec![
            vec![Span::styled(format!("  {name}"), if cur { Style::default().fg(color).add_modifier(Modifier::BOLD) } else { theme::bold() })],
            vec![Span::styled(format!("  {tech}"), theme::muted())],
            vec![Span::styled(format!("  {effort}"), theme::muted())],
            vec![Span::styled(format!("  {threads}"), theme::muted())],
        ];
        let sprite = agents::sprite(a);
        let inner = Rect { x: inner.x + 1, width: inner.width.saturating_sub(1), ..inner };
        f.render_widget(Paragraph::new(sprites::lines(&sprite, if cur { color } else { theme::MUTED }, side)), inner);
    }

    fn draw_compose(&mut self, f: &mut Frame, area: Rect) {
        let label = |text: &str, focused: bool| Line::styled(text.to_string(), if focused { theme::accent_bold() } else { theme::muted() });
        let title_rows = (self.title.rows_for(area.width.saturating_sub(2)) as u16).min(3);
        let n_notes = self.selected.len();
        let note_rows = n_notes.min(5) as u16 + u16::from(n_notes > 5) + self.carry.len().min(5) as u16;
        let opts_h = u16::from(!self.wheres.is_empty());
        // "view drafts (n)": only with 1+ drafts, with a blank row below
        let drafts_h = if self.drafts.is_empty() { 0 } else { 2 };
        let available = area.height.saturating_sub(drafts_h + title_rows + opts_h + note_rows + 12);
        let agents_h = if self.agents.is_empty() { 1 } else {
            self.agent_grid(area.width).height.min(2 * (CARD_H as usize + 2)).min(available.max(CARD_H + 1) as usize) as u16
        };
        if drafts_h == 0 && self.field == Field::Drafts {
            self.field = Field::Title;
        }
        let [r_drafts, l_title, i_title, _, info, opts, _, l_agents, r_agents, _, l_notes, notes, _, l_prompt, i_prompt] = Layout::vertical([
            Constraint::Length(drafts_h),
            Constraint::Length(1),
            Constraint::Length(title_rows),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(opts_h),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(agents_h),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(note_rows),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(3),
        ])
        .areas(area);
        if drafts_h > 0 {
            let on = self.field == Field::Drafts;
            let text = format!(" view drafts ({}) ", self.drafts.len());
            let r = Rect { width: (text.width() as u16).min(r_drafts.width), height: 1, ..r_drafts };
            let st = if on { theme::pill().add_modifier(Modifier::BOLD) } else { theme::panel().fg(theme::ACCENT) };
            f.render_widget(Paragraph::new(text).style(st), r);
            self.regions.drafts = r;
            if on && self.drafts_menu.is_none() {
                let hint = Rect { x: r.right() + 2, width: r_drafts.width.saturating_sub(r.width + 2), height: 1, ..r_drafts };
                f.render_widget(Paragraph::new(Line::styled("enter: list of unsent tasks", theme::muted())), hint);
            }
        }
        f.render_widget(Paragraph::new(label("title", self.field == Field::Title)), l_title);
        f.render_widget(Paragraph::new("").style(theme::panel()), i_title);
        self.title.render(f, Rect { x: i_title.x + 1, width: i_title.width.saturating_sub(2), ..i_title }, self.field == Field::Title);
        // where: ‹ choice › + what it does
        let on = self.field == Field::Where;
        let mut spans = vec![Span::styled("where ", if on { theme::accent_bold() } else { theme::muted() })];
        if !self.wheres.is_empty() {
            let chip = Style::default().fg(theme::TEXT).add_modifier(Modifier::BOLD);
            spans.push(Span::styled(if on { "‹" } else { " " }, theme::accent()));
            spans.push(Span::styled(format!(" {} ", self.where_chip()), if on { chip.bg(theme::PANEL) } else { chip }));
            spans.push(Span::styled(if on { "›" } else { " " }, theme::accent()));
            spans.push(Span::raw("  "));
        }
        spans.push(Span::styled(self.where_text(), theme::muted()));
        f.render_widget(Paragraph::new(Line::from(spans)), info);
        self.regions.where_row = info;
        // opts: ◼ checkout · ◻ stash, under the where chip
        if opts_h > 0 {
            let on = self.field == Field::Opts;
            let mut x = opts.x + 6;
            for (i, (checked, enabled), label) in [(0, self.checkout_box(), "checkout selected branch"), (1, self.stash_box(), "stash conflicting local changes (git stash)")] {
                let text = format!(" {} {label} ", if checked { "◼" } else { "◻" });
                let w = (text.width() as u16).min((opts.x + opts.width).saturating_sub(x));
                // the box under the cursor: accent, like the other fields (disabled: dim on accent)
                let style = match (enabled, on && self.opt_i == i) {
                    (true, true) => theme::pill().add_modifier(Modifier::BOLD),
                    (false, true) => theme::pill().add_modifier(Modifier::DIM),
                    (false, false) => Style::default().fg(theme::LINE),
                    (true, false) if checked => Style::default().fg(theme::TEXT),
                    (true, false) => theme::muted(),
                };
                let r = Rect { x, y: opts.y, width: w, height: 1 };
                f.render_widget(Paragraph::new(text).style(style), r);
                self.regions.opts.push((r, i));
                x += w + 2;
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
        let model = agents::model_of(a);
        let tech = if model.is_empty() { a.harness.clone() } else { format!("{} · {model}", a.harness) };
        // effort: Claude Code's global effortLevel; other harnesses: their own
        let effort = if a.harness == "claude" { format!("effort {}", self.effort) } else { "effort: harness".into() };
        [a.name.clone(), tech, effort]
    }

    /// Agent card: rounded box, sprite left, text right. Picked: border in the agent color, ◼ (+ order).
    /// Cursor: accent border.
    fn draw_card(&self, f: &mut Frame, r: Rect, a: &Agent, cur: bool) {
        let color = agents::color(a.color.as_deref());
        let order = self.picks.iter().position(|p| *p == a.name);
        let border = match (cur, order.is_some()) {
            (true, _) => theme::accent_bold(),
            (false, true) => Style::default().fg(color),
            (false, false) => theme::line(),
        };
        let mark = match order {
            Some(n) if self.picks.len() > 1 => format!(" ◼ {} ", n + 1),
            Some(_) => " ◼ ".to_string(),
            None => " ◻ ".to_string(),
        };
        let block = Block::new()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(border)
            .title(Span::styled(mark, if order.is_some() { Style::default().fg(color).add_modifier(Modifier::BOLD) } else { theme::muted() }));
        let inner = block.inner(r);
        f.render_widget(block, r);
        let [name, tech, effort] = self.card_text(a);
        let name_st = if order.is_some() || cur { Style::default().fg(color).add_modifier(Modifier::BOLD) } else { theme::bold() };
        let side = vec![
            vec![Span::styled(format!("  {name}"), name_st)],
            vec![Span::styled(format!("  {tech}"), theme::muted())],
            vec![Span::styled(format!("  {effort}"), theme::muted())],
            vec![],
        ];
        let sprite = agents::sprite(a);
        let inner = Rect { x: inner.x + 1, width: inner.width.saturating_sub(1), ..inner };
        f.render_widget(Paragraph::new(sprites::lines(&sprite, if order.is_some() || cur { color } else { theme::MUTED }, side)), inner);
    }

    fn draw_footer(&mut self, f: &mut Frame, area: Rect) {
        let mut spans = vec![Span::raw(" ")];
        let hints: &[(&str, &str)] = match self.screen {
            _ if self.tabs_focus => &[("←→", "tab"), ("↓ / esc", "back"), ("q", "quit")],
            Screen::Compose if self.field == Field::Agents => &[("↑↓←→", "move"), ("space", "pick"), ("enter", "notes"), ("⌘/ctrl+⏎", "start"), ("tab", "threads")],
            Screen::Compose if self.field == Field::Where => &[("←→", "current · new branch · new worktree · main · recent"), ("↑↓", "field"), ("⌘/ctrl+⏎", "start"), ("tab", "threads")],
            Screen::Compose if self.field == Field::Opts => &[("←→", "box"), ("space", "check / uncheck"), ("↑↓", "field"), ("⌘/ctrl+⏎", "start")],
            Screen::Compose if self.drafts_menu.is_some() => &[("↑↓", "move"), ("enter", "load"), ("d", "delete"), ("esc", "back")],
            Screen::Compose if self.field == Field::Drafts => &[("enter", "list drafts"), ("↓", "title"), ("⌘/ctrl+⏎", "start"), ("tab", "threads")],
            Screen::Compose if self.field == Field::Prompt => &[("@", "agents"), (">", "commands"), ("dir/", "paths"), ("ctrl+v", "image"), ("↑↓", "field"), ("⌘/ctrl+⏎", "start"), ("esc esc", "quit")],
            Screen::Compose if self.field == Field::Notes => &[("enter", "pick notes"), ("x", "drop"), ("↑↓", "field"), ("⌘/ctrl+⏎", "start"), ("tab", "threads")],
            Screen::Compose => &[("↑↓", "field"), ("⌘/ctrl+⏎", "start"), ("tab", "threads"), ("esc esc", "quit")],
            Screen::Pick if self.tfilter_editing => &[("type", "filter"), ("↑↓", "move"), ("enter / esc", "back to list")],
            Screen::Pick if self.filter_editing => &[("type", "filter"), ("↑↓", "move"), ("enter", "done"), ("esc", "clear")],
            Screen::Pick if self.tab == Tab::Notes && self.cfg.copy_mode => &[("↑↓", "move"), ("←→", "fold"), ("space", "select"), ("enter", "copy"), ("esc", "quit")],
            Screen::Pick if self.tab == Tab::Notes => &[("↑↓", "move"), ("←→", "fold"), ("space", "select"), ("enter", "confirm"), ("esc", "new"), ("tab", "threads")],
            Screen::Pick if self.tab == Tab::Threads => &[("↑↓", "move"), ("f", "filter"), ("space", "select"), ("c", "context"), ("enter", "open"), ("tab", "agents")],
            Screen::Pick if self.tab == Tab::Settings => &[("↑↓", "field / alias"), ("enter", "edit / done"), ("d", "delete alias"), ("ctrl+n", "add alias"), ("tab", "complete / new"), ("esc", "close paths")],
            Screen::Pick if self.afilter_editing => &[("type", "filter"), ("↑↓", "move"), ("enter / esc", "back to cards")],
            Screen::Pick => &[("↑↓←→", "move"), ("f", "filter"), ("enter", "edit"), ("tab", "settings")],
        };
        for (k, l) in hints {
            spans.extend(hint(k, l));
        }
        f.render_widget(Paragraph::new(Line::from(spans)), area);
        let mut right = Vec::new();
        if let Some((msg, _, err)) = &self.toast {
            right.push(Span::styled(format!("{msg}   "), Style::default().fg(if *err { theme::DANGER } else { theme::OK })));
        }
        right.push(Span::styled("F2", theme::accent()));
        right.push(Span::styled(" menu ", theme::muted()));
        f.render_widget(Paragraph::new(Line::from(right).right_aligned()), area);
    }

    fn frame(f: &mut Frame, area: Rect, title: &str, w: u16, h: u16) -> Rect {
        let w = w.min(area.width.saturating_sub(2));
        let h = h.min(area.height.saturating_sub(2));
        let r = Rect { x: area.x + (area.width - w) / 2, y: area.y + (area.height - h) / 2, width: w, height: h };
        f.render_widget(Clear, r);
        let block = Block::new()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
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
                        Style::default().bg(theme::ACCENT).fg(theme::ON_ACCENT).add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().bg(theme::PANEL).fg(theme::TEXT)
                    };
                    let r = Rect { x, y: b.y, width: l.width() as u16, height: 1 };
                    f.render_widget(Paragraph::new(l).style(style), r);
                    buttons.push((r, value));
                    x += r.width + 2;
                }
                self.regions.buttons = buttons;
            }
            Modal::Agent(_) => self.draw_agent_form(f, area),
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
        let inner = Self::frame(f, area, title, 90, area.height.saturating_sub(2).min(41));
        // 0-6 single-line fields, 7 note, 9 sprite set, 10 sprite, 12-13 voice, 15-16 system prompt, 18 error, 19 buttons
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
                    f.render_widget(Paragraph::new(Line::styled(" set in the harness field: harness/model", theme::muted())), vr);
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
                            if on { st.bg(theme::PANEL) } else { st }
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
        } else {
            form.style_note(global.as_deref())
        };
        f.render_widget(Paragraph::new(Line::styled(note_text, theme::muted())), Rect { x: note.x + 11, width: note.width.saturating_sub(11), ..note });
        // sprite set: 1st the set, then a sprite of it
        let on = form.field == F_SET;
        let tr = rows[9];
        form.areas[F_SET] = tr;
        let mut spans = vec![lab("sprites", on), Span::styled(if on { "‹" } else { " " }, theme::accent())];
        for (j, name) in sprites::sets().iter().enumerate() {
            let st = if j == form.set { if on { theme::bold().bg(theme::PANEL) } else { theme::bold() } } else { theme::muted() };
            spans.push(Span::styled(format!(" {name} "), st));
        }
        spans.push(Span::styled(if on { "›" } else { "" }, theme::accent()));
        f.render_widget(Paragraph::new(Line::from(spans)), tr);
        // sprite: label, portrait in the agent color, id + keys
        let on = form.field == F_SPRITE;
        let sr = rows[10];
        form.areas[F_SPRITE] = sr;
        f.render_widget(Paragraph::new(Line::from(lab("sprite", on))), sr);
        let color = agents::color(Some(agents::COLORS[form.color]));
        let arrow = |t: &'static str| Span::styled(if on { t } else { " " }, theme::accent());
        let id = Span::styled(format!(" {} ", form.sprite.id), if on { theme::bold().bg(theme::PANEL) } else { theme::muted() });
        let members = sprites::members(form.set_name());
        let at = sprites::index_of(&form.sprite.id).and_then(|i| members.iter().position(|&x| x == i));
        let count = match at {
            Some(k) => format!("   {}/{} in {}", k + 1, members.len(), form.set_name()),
            None => format!("   {} in {}", members.len(), form.set_name()),
        };
        let side = vec![
            vec![Span::raw("   "), arrow("‹"), id, arrow("›")],
            vec![Span::styled(count, theme::muted())],
            vec![],
            vec![Span::styled("   ←→ browse · r random in set · e edit in $EDITOR", if on || form.field == F_SET { theme::muted() } else { Style::default().fg(theme::LINE) })],
        ];
        f.render_widget(Paragraph::new(sprites::lines(&form.sprite, color, side)), Rect { x: sr.x + 11, width: sr.width.saturating_sub(11), ..sr });
        for (field, label_row, box_row, head) in [(F_VOICE, rows[12], rows[13], "voice · own tone on top of the style"), (F_PROMPT, rows[15], rows[16], "system prompt · standing instructions")] {
            let on = form.field == field;
            f.render_widget(Paragraph::new(Line::styled(head, if on { theme::accent_bold() } else { theme::muted() })), label_row);
            f.render_widget(Paragraph::new("").style(theme::panel()), box_row);
            form.areas[field] = box_row;
            let ta = if field == F_VOICE { &mut form.voice } else { &mut form.prompt };
            ta.render(f, Rect { x: box_row.x + 1, width: box_row.width.saturating_sub(2), ..box_row }, on);
        }
        if let Some(e) = &form.error {
            f.render_widget(Paragraph::new(Line::styled(e.clone(), Style::default().fg(theme::DANGER))), rows[18]);
        }
        let b = rows[19];
        let labels = [(true, if form.edit.is_some() { "  save (ctrl+s)  " } else { "  create (ctrl+s)  " }), (false, "  cancel (esc)  ")];
        let total: u16 = labels.iter().map(|(_, l)| l.width() as u16).sum::<u16>() + 2;
        let mut x = b.x + b.width.saturating_sub(total) / 2;
        let mut buttons = Vec::new();
        for (value, l) in labels {
            let focused = form.field == F_BUTTONS && value == form.save;
            let style = if focused { Style::default().bg(theme::ACCENT).fg(theme::ON_ACCENT).add_modifier(Modifier::BOLD) } else { Style::default().bg(theme::PANEL).fg(theme::TEXT) };
            let r = Rect { x, y: b.y, width: l.width() as u16, height: 1 };
            f.render_widget(Paragraph::new(l).style(style), r);
            buttons.push((r, value));
            x += r.width + 2;
        }
        self.regions.buttons = buttons;
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
    fn app_with(drafts: Option<PathBuf>) -> App {
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
        key(&mut a, KeyCode::Right, KeyModifiers::NONE);
        assert!(matches!(a.target(), Ok(Target::NewBranch(b)) if b == "task/fix-deploy"));
        assert_eq!(a.checkout_box(), (false, false));
        key(&mut a, KeyCode::Right, KeyModifiers::NONE);
        assert!(matches!(a.target(), Ok(Target::Branch(b)) if b == "task/fix-deploy"));
        assert_eq!(a.where_chip(), "+ new branch + worktree");
        key(&mut a, KeyCode::Right, KeyModifiers::NONE);
        assert!(matches!(a.target(), Ok(Target::Checkout { branch, switch: true, stash: false }) if branch == "main"));
        assert!(a.where_text().starts_with("switches ") && a.where_text().ends_with("from feature/x to main"), "{}", a.where_text());
        // opts row: checkout off → 0 switch; stash only with checkout
        key(&mut a, KeyCode::Down, KeyModifiers::NONE);
        assert!(a.field == Field::Opts);
        key(&mut a, KeyCode::Right, KeyModifiers::NONE);
        key(&mut a, KeyCode::Char(' '), KeyModifiers::NONE);
        assert!(matches!(a.target(), Ok(Target::Checkout { stash: true, .. })));
        key(&mut a, KeyCode::Left, KeyModifiers::NONE);
        key(&mut a, KeyCode::Char(' '), KeyModifiers::NONE);
        assert!(matches!(a.target(), Ok(Target::Checkout { switch: false, stash: false, .. })));
        assert!(a.where_text().contains("0 checkout"), "{}", a.where_text());
        key(&mut a, KeyCode::Char(' '), KeyModifiers::NONE);
        key(&mut a, KeyCode::Up, KeyModifiers::NONE);
        assert!(a.field == Field::Where);
        key(&mut a, KeyCode::Right, KeyModifiers::NONE);
        assert_eq!(a.where_chip(), "current branch · feature/x");
        // a worktree from the threads tab wins until ←→
        a.target = Some(Target::Worktree { path: "/w".into(), branch: "b".into() });
        assert_eq!(a.where_chip(), "worktree b");
        key(&mut a, KeyCode::Left, KeyModifiers::NONE);
        assert_eq!(a.where_chip(), "current branch · feature/x");
        // field order: title → where → opts (also with disabled boxes) → agents
        a.field = Field::Title;
        key(&mut a, KeyCode::Down, KeyModifiers::NONE);
        assert!(a.field == Field::Where);
        key(&mut a, KeyCode::Down, KeyModifiers::NONE);
        assert!(a.field == Field::Opts);
        key(&mut a, KeyCode::Down, KeyModifiers::NONE);
        assert!(a.field == Field::Agents);
        key(&mut a, KeyCode::Up, KeyModifiers::NONE);
        assert!(a.field == Field::Opts);
        a.screen = Screen::Compose;
        a.agents = vec![
            Agent { name: "amon".into(), harness: "claude".into(), model: Some("opus".into()), color: Some("red".into()), ..Default::default() },
            Agent { name: "nova".into(), harness: "codex".into(), model: Some("gpt-5.5".into()), ..Default::default() },
        ];
        a.picks = vec!["amon".into()];
        let out = dump(&mut a, 110, 32);
        assert!(out.contains("where   current branch · feature/x"), "{out}");
        assert!(out.contains("◼ checkout selected branch"), "{out}");
        assert!(out.contains("claude · Opus 5.5") && out.contains("codex · gpt-5.5"), "{out}");
        println!("--- where row\n{out}");
        let _ = std::fs::remove_dir_all(&dir);
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
        assert!(a.screen == Screen::Compose && a.field == Field::Title);
        println!("--- start\n{}", dump(&mut a, 100, 18));
    }

    #[test]
    fn enter_keeps_zero_notes() {
        let mut a = app();
        // title → agents → notes field (↓), enter opens the picker
        key(&mut a, KeyCode::Down, KeyModifiers::NONE);
        key(&mut a, KeyCode::Down, KeyModifiers::NONE);
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
            key(&mut a, KeyCode::Char('s'), KeyModifiers::CONTROL);
            assert!(a.outcome.is_none(), "ctrl+s must not start");
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
        key(&mut a, KeyCode::Char('f'), KeyModifiers::NONE);
        a.paste("thread filter");
        key(&mut a, KeyCode::Esc, KeyModifiers::NONE);
        assert!(!a.tfilter_editing && a.tab == Tab::Threads);
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
    fn drafts_backup_list_load_delete() {
        let mut a = app();
        let dir = a.cfg.drafts.clone();
        // 0 text: 0 draft file, 0 drafts field
        assert!(drafts::load(&dir, "").is_empty());
        assert!(!dump(&mut a, 120, 40).contains("view drafts"));
        // every key: the draft file has the text
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
        // another codehead (the 1st one died): the draft shows, ↑ from the title reaches the field
        let mut b = app_with(Some(dir.clone()));
        assert!(dump(&mut b, 120, 40).contains("view drafts (1)"));
        assert!(b.field == Field::Title);
        key(&mut b, KeyCode::Up, KeyModifiers::NONE);
        assert!(b.field == Field::Drafts);
        // enter: the dropdown; esc: back to the task (0 quit)
        key(&mut b, KeyCode::Enter, KeyModifiers::NONE);
        let s = dump(&mut b, 120, 40);
        assert!(s.contains(" drafts ") && s.contains("fix the build") && s.contains("d: delete"), "{s}");
        key(&mut b, KeyCode::Esc, KeyModifiers::NONE);
        assert!(b.drafts_menu.is_none() && b.outcome.is_none());
        // enter enter: loaded, the draft is this task's own now
        key(&mut b, KeyCode::Enter, KeyModifiers::NONE);
        key(&mut b, KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!((b.title.text().as_str(), b.prompt.text().as_str()), ("fix the build", "- a\n- b"));
        assert!(b.drafts.is_empty() && b.field == Field::Prompt);
        assert_eq!(drafts::load(&dir, "").len(), 1);
        // a 3rd codehead: d deletes the draft, the field goes
        let mut c = app_with(Some(dir.clone()));
        key(&mut c, KeyCode::Up, KeyModifiers::NONE);
        key(&mut c, KeyCode::Enter, KeyModifiers::NONE);
        key(&mut c, KeyCode::Char('d'), KeyModifiers::NONE);
        assert!(c.drafts.is_empty() && c.drafts_menu.is_none() && c.field == Field::Title);
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
        assert!(c.selected.is_empty() && c.files.is_empty() && c.carry.is_empty() && c.field == Field::Title && c.draft_id != old);
        assert!(drafts::load(&dir, "").is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn prompt_command_dropdown_inserts_a_file() {
        let mut a = app();
        a.field = Field::Prompt;
        type_text(&mut a, "read >");
        let s = dump(&mut a, 120, 40);
        assert!(s.contains("╭ commands ") && s.contains("mention"), "{s}");
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
        assert!(!s.contains("╭ commands "), "{s}");
    }

    #[test]
    fn up_from_the_task_top_reaches_the_tabs() {
        let mut a = app();
        assert!(a.screen == Screen::Compose && a.field == Field::Title && a.drafts.is_empty());
        key(&mut a, KeyCode::Up, KeyModifiers::NONE);
        assert!(a.tabs_focus);
        key(&mut a, KeyCode::Down, KeyModifiers::NONE);
        assert!(!a.tabs_focus && a.field == Field::Title);
        // with drafts: title → drafts row → tabs
        a.drafts = vec![Default::default()];
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
        assert!(a.field == Field::Title);
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
        key(&mut a, KeyCode::Char('f'), KeyModifiers::NONE);
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
        assert_eq!(bg(&t, tab.x + 1, tab.y), theme::HOVER);
        // the where row of the task view
        let w = a.regions.where_row;
        a.mouse(moved(w.x + 2, w.y));
        t.draw(|f| a.draw(f)).unwrap();
        assert_eq!(bg(&t, w.x + 2, w.y), theme::HOVER);
        assert_ne!(bg(&t, tab.x + 1, tab.y), theme::HOVER, "1 item at a time");
        // agents tab: a card takes the hover, the free room right of it and its heading do not
        a.switch_tab(Tab::Agents);
        a.agents.truncate(1);
        t.draw(|f| a.draw(f)).unwrap();
        let card = a.acards[0].0;
        a.mouse(moved(card.right() + 1, card.y + 1));
        t.draw(|f| a.draw(f)).unwrap();
        assert_ne!(bg(&t, card.right() + 1, card.y + 1), theme::HOVER);
        a.mouse(moved(card.x + 1, card.y - 1));
        t.draw(|f| a.draw(f)).unwrap();
        assert_ne!(bg(&t, card.x + 1, card.y - 1), theme::HOVER);
        a.mouse(moved(card.x + 2, card.y + 1));
        t.draw(|f| a.draw(f)).unwrap();
        assert_eq!(bg(&t, card.x + 2, card.y + 1), theme::HOVER);
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
        key(&mut a, KeyCode::Char('f'), KeyModifiers::NONE);
        undo_redo(&mut a, |a| a.tfilter.clone().unwrap_or_default());
        a.switch_tab(Tab::Notes);
        key(&mut a, KeyCode::Char('/'), KeyModifiers::NONE);
        undo_redo(&mut a, |a| a.filter.clone().unwrap_or_default());
        assert_eq!(a.rows.len(), 0, "filter \"hi\" matches 0 notes");
        key(&mut a, KeyCode::Char('z'), KeyModifiers::CONTROL);
        assert!(!a.rows.is_empty(), "undo rebuilds the list");
    }

    #[test]
    fn agent_form_sprite_set_then_sprite() {
        let mut a = app();
        a.run(Act::NewAgent);
        let form = |a: &App| match &a.modal {
            Some(Modal::Agent(f)) => (f.field, f.set_name().to_string(), f.sprite.id.clone()),
            _ => panic!("agent form closed"),
        };
        if let Some(Modal::Agent(f)) = &mut a.modal {
            f.field = F_SET;
            f.set = 0;
        }
        key(&mut a, KeyCode::Right, KeyModifiers::NONE);
        key(&mut a, KeyCode::Right, KeyModifiers::NONE);
        let (_, set, id) = form(&a);
        assert_eq!((set.as_str(), id.as_str()), ("regi", "regi-000"), "set ←→: 1st sprite of the set");
        key(&mut a, KeyCode::Down, KeyModifiers::NONE);
        key(&mut a, KeyCode::Right, KeyModifiers::NONE);
        assert_eq!(form(&a), (F_SPRITE, "regi".into(), "regi-001".into()));
        key(&mut a, KeyCode::Left, KeyModifiers::NONE);
        key(&mut a, KeyCode::Left, KeyModifiers::NONE);
        assert_eq!(form(&a).2, "regi-099", "sprite ←→ wraps inside the set");
        for _ in 0..30 {
            key(&mut a, KeyCode::Char('r'), KeyModifiers::NONE);
            assert!(form(&a).2.starts_with("regi-"), "r: random in the set");
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
        key(&mut a, KeyCode::Char('f'), KeyModifiers::NONE);
        assert!(a.tfilter_editing);
        for c in "amp".chars() {
            key(&mut a, KeyCode::Char(c), KeyModifiers::NONE);
        }
        assert_eq!(a.n_threads(), 1);
        assert_eq!(a.current_session().unwrap().title, "Ampcode ideas");
        key(&mut a, KeyCode::Enter, KeyModifiers::NONE);
        assert!(!a.tfilter_editing && a.tfilter.as_deref() == Some("amp"));
        key(&mut a, KeyCode::Char('f'), KeyModifiers::NONE);
        key(&mut a, KeyCode::Esc, KeyModifiers::NONE);
        assert!(!a.tfilter_editing);
        println!("--- thread filter\n{}", dump(&mut a, 110, 10));
    }

    #[test]
    fn settings_tab_entry_keeps_paths_unfocused() {
        let mut a = app();
        let config = format!("notes = {:?}\n", format!("{}/infra/", a.cfg.cwd.display()));
        std::fs::write(&a.settings_path, &config).unwrap();

        a.switch_tab(Tab::Agents);
        key(&mut a, KeyCode::Tab, KeyModifiers::NONE);
        assert!(a.tab == Tab::Settings && a.tabs_focus);
        let screen = dump(&mut a, 100, 36);
        assert!(!screen.contains("deploy.md"), "{screen}");
        key(&mut a, KeyCode::Tab, KeyModifiers::NONE);
        assert!(a.screen == Screen::Compose);
        assert_eq!(std::fs::read_to_string(&a.settings_path).unwrap(), config);

        key(&mut a, KeyCode::BackTab, KeyModifiers::SHIFT);
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
        key(&mut a, KeyCode::Tab, KeyModifiers::NONE);
        assert!(a.tab == Tab::Settings && a.tabs_focus);
        let screen = dump(&mut a, 100, 36);
        assert!(!screen.contains("deploy.md"), "{screen}");
        key(&mut a, KeyCode::BackTab, KeyModifiers::SHIFT);
        assert!(a.tab == Tab::Agents);
        assert_eq!(std::fs::read_to_string(&a.settings_path).unwrap(), config);
    }

    #[test]
    fn settings_path_escape_closes_dropdown_before_tab_navigation() {
        let mut a = app();
        a.switch_tab(Tab::Settings);
        key(&mut a, KeyCode::Down, KeyModifiers::NONE);
        key(&mut a, KeyCode::Char('n'), KeyModifiers::CONTROL);
        a.paste("local");
        key(&mut a, KeyCode::Down, KeyModifiers::NONE);
        a.paste(&format!("{}/infra/", a.cfg.cwd.display()));
        let screen = dump(&mut a, 100, 36);
        assert!(screen.contains("deploy.md"), "{screen}");
        key(&mut a, KeyCode::Esc, KeyModifiers::NONE);
        assert!(a.tab == Tab::Settings && a.outcome.is_none());
        assert!(a.last_esc.is_none(), "dropdown Escape must not arm double Escape");
        let screen = dump(&mut a, 100, 36);
        assert!(!screen.contains("deploy.md"), "{screen}");
        key(&mut a, KeyCode::Tab, KeyModifiers::NONE);
        assert!(a.screen == Screen::Compose);
    }

    #[test]
    fn settings_tab_keeps_form_edits_and_task_text() {
        let mut a = app();
        a.title.set_text("task title");
        a.prompt.set_text("task prompt");
        key(&mut a, KeyCode::BackTab, KeyModifiers::SHIFT);
        assert!(a.tab == Tab::Settings);
        key(&mut a, KeyCode::Down, KeyModifiers::NONE);
        key(&mut a, KeyCode::Char('n'), KeyModifiers::CONTROL);
        a.paste("project:platform");
        key(&mut a, KeyCode::Down, KeyModifiers::NONE);
        a.paste("~/notes/platform/");
        let screen = dump(&mut a, 60, 18);
        assert!(screen.lines().next().unwrap().contains("settings"), "{screen}");
        assert!(screen.contains("not applied"), "{screen}");
        assert!(screen.contains("~/notes/platform/"), "{screen}");
        key(&mut a, KeyCode::Esc, KeyModifiers::NONE);
        assert!(a.tab == Tab::Settings);
        key(&mut a, KeyCode::Tab, KeyModifiers::NONE);
        assert!(a.screen == Screen::Compose);
        assert_eq!(a.title.text(), "task title");
        assert_eq!(a.prompt.text(), "task prompt");
        key(&mut a, KeyCode::BackTab, KeyModifiers::SHIFT);
        let screen = dump(&mut a, 100, 24);
        assert!(screen.contains("~/notes/platform/"), "{screen}");
        key(&mut a, KeyCode::BackTab, KeyModifiers::SHIFT);
        assert!(a.tab == Tab::Agents);
    }

    #[test]
    fn tabs_cycle_from_task() {
        let mut a = app();
        key(&mut a, KeyCode::Tab, KeyModifiers::NONE);
        assert!(a.screen == Screen::Pick && a.tab == Tab::Threads);
        key(&mut a, KeyCode::Esc, KeyModifiers::NONE);
        assert!(a.screen == Screen::Pick && a.tab == Tab::Threads);
        key(&mut a, KeyCode::BackTab, KeyModifiers::SHIFT);
        assert!(a.screen == Screen::Compose);
        // tab always switches tabs, also from the task fields
        key(&mut a, KeyCode::Tab, KeyModifiers::NONE);
        assert!(a.screen == Screen::Pick && a.tab == Tab::Threads);
        key(&mut a, KeyCode::Tab, KeyModifiers::NONE);
        assert!(a.tab == Tab::Agents);
        key(&mut a, KeyCode::Tab, KeyModifiers::NONE);
        assert!(a.tab == Tab::Settings);
        key(&mut a, KeyCode::Esc, KeyModifiers::NONE);
        assert!(a.tab == Tab::Settings);
        key(&mut a, KeyCode::Tab, KeyModifiers::NONE);
        assert!(a.screen == Screen::Compose);
        // the agents tab loaded the machine's agents: 1 card again (↓ leaves the cards)
        a.agents.truncate(1);
        // ↑↓ walk the fields, ↑ on the prompt's 1st row leaves it
        for _ in 0..3 {
            key(&mut a, KeyCode::Down, KeyModifiers::NONE);
        }
        assert!(a.field == Field::Prompt);
        key(&mut a, KeyCode::Up, KeyModifiers::NONE);
        assert!(a.field == Field::Notes);
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

    #[test]
    fn thread_sections_have_gaps_and_preview_uses_title() {
        let mut a = app();
        let w = Worktree {
            path: "/project".into(), branch: "in place".into(), linked: false,
            unpushed: 0, when: "1m".into(),
            thread: Some(threads::Thread {
                id: "a1".into(), title: "fix latency".into(), state: "open".into(),
                live: 1, updated: 0, worktree: None, branch: None, dir: Some("/project".into()),
            }),
        };
        a.tpreview.insert(w.key(), threads::preview_header(&w));
        a.worktrees = Some(vec![w]);
        a.sessions = vec![Session { id: "s1".into(), title: "previous work".into(), cwd: "/project".into(), ..Default::default() }];
        a.switch_tab(Tab::Threads);
        let rows = a.trows(&a.tvisible());
        assert!(matches!(rows[2], TRow::Gap));
        let mut t = Terminal::new(TestBackend::new(100, 36)).unwrap();
        t.draw(|f| a.draw(f)).unwrap();
        let buffer = t.backend().buffer();
        let line = |y| (0..100).map(|x| buffer[(x, y)].symbol()).collect::<String>();
        let label_y = (0..36).find(|&y| line(y).contains("claude code sessions")).unwrap();
        assert!(line(label_y - 1).trim().is_empty());
        assert!(line(a.regions.preview.y - 2).trim().is_empty());
        assert!(line(a.regions.preview.y).contains("fix latency"));
        assert!(!line(a.regions.preview.y).contains("in place"));
    }

    #[test]
    fn screens() {
        let mut a = app();
        for c in "Fix deploy".chars() {
            key(&mut a, KeyCode::Char(c), KeyModifiers::NONE);
        }
        key(&mut a, KeyCode::Down, KeyModifiers::NONE);
        key(&mut a, KeyCode::Down, KeyModifiers::NONE);
        key(&mut a, KeyCode::Enter, KeyModifiers::NONE);
        println!("--- notes picker\n{}", dump(&mut a, 130, 16));
        key(&mut a, KeyCode::Down, KeyModifiers::NONE);
        key(&mut a, KeyCode::Char(' '), KeyModifiers::NONE);
        key(&mut a, KeyCode::F(2), KeyModifiers::NONE);
        println!("--- menu\n{}", dump(&mut a, 100, 22));
        key(&mut a, KeyCode::Esc, KeyModifiers::NONE);
        key(&mut a, KeyCode::Enter, KeyModifiers::NONE);
        key(&mut a, KeyCode::Down, KeyModifiers::NONE);
        a.paste("line one\nline two");
        println!("--- compose\n{}", dump(&mut a, 100, 22));
        println!("--- message\n{}", a.message());
        key(&mut a, KeyCode::Tab, KeyModifiers::NONE);
        println!("--- threads\n{}", dump(&mut a, 120, 14));
        key(&mut a, KeyCode::Char('3'), KeyModifiers::NONE);
        assert!(a.tab == Tab::Threads, "0 number keys for tabs");
        key(&mut a, KeyCode::Tab, KeyModifiers::NONE);
        println!("--- agents\n{}", dump(&mut a, 120, 14));
        key(&mut a, KeyCode::Char('n'), KeyModifiers::NONE);
        println!("--- new agent\n{}", dump(&mut a, 120, 42));
        for _ in 0..F_SPRITE {
            key(&mut a, KeyCode::Tab, KeyModifiers::NONE);
        }
        key(&mut a, KeyCode::Right, KeyModifiers::NONE);
        println!("--- sprite field\n{}", dump(&mut a, 120, 42));
        key(&mut a, KeyCode::Esc, KeyModifiers::NONE);
        assert!(a.message().starts_with("Fix deploy\n\nRead these notes first:\n- "));
        assert!(a.message().ends_with("\nline one\nline two"));
        let _ = std::fs::remove_dir_all(&a.cfg.notes);
    }
}
