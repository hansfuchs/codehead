//! The thread list of both threads tabs: the menu's (ctrl+p: a quick switch, type to filter) and
//! the start screen's (manage: preview, multi-select, letter keys). 1 look: thread cards, the repo
//! tabs, the project tabs, the view tabs (active · archived; the start screen also: worktrees
//! without thread, claude sessions: their rows are the host's), the status tabs (open · stale ·
//! shipped · all: a fixed row above the list, out of its scroll), a name filter.
//!
//! Actions on the card under the cursor (the start screen: or the selection): open, archive /
//! unarchive, edit, delete. ctrl+.: their dropdown (both tabs); the start screen also: a u e d.
//! Archive and delete ask first (1 row). Edit: a form (title, repo, linked repos, agents). The
//! jobs run off the UI thread; their result shows on the list's last row.

use crate::ui::dissolve::Dissolve;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph};

use crate::ui::tab_group::TabGroup;
use crate::ui::textarea::TextArea;
use crate::ui::theme;
use crate::ui::thread_card::{self, Card};

/// Menu (quick switch) or start screen (manage).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Mode {
    Switch,
    Manage,
}

/// The view tabs.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum View {
    Active,
    Archived,
    /// Start screen: worktrees without a thread (the host's rows).
    Worktrees,
    /// Start screen: Claude Code sessions (the host's rows).
    Claude,
}

impl View {
    fn label(self) -> &'static str {
        match self {
            View::Active => "active",
            View::Archived => "archived",
            View::Worktrees => "worktrees",
            View::Claude => "claude",
        }
    }

    /// Thread cards (else the host's rows).
    pub(crate) fn cards(self) -> bool {
        matches!(self, View::Active | View::Archived)
    }
}

/// Where the keys go: the repo tabs, the project tabs, the view tabs, the status tabs, the list.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Focus {
    Repos,
    Projects,
    Views,
    Status,
    List,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Act {
    Open,
    Archive,
    Unarchive,
    Edit,
    Delete,
}

impl Act {
    fn label(self) -> &'static str {
        match self {
            Act::Open => "open",
            Act::Archive => "archive: worktree goes, branch stays",
            Act::Unarchive => "unarchive",
            Act::Edit => "edit: title, repo, projects, linked repos, agents",
            Act::Delete => "delete: thread + worktree",
        }
    }
}

/// What the host does after a key / click.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Out {
    /// Done here.
    None,
    /// Not a key of the list: the host's.
    Pass,
    /// ↑ above the repo tabs: the host's tab bar.
    Up,
    Open(Box<Card>),
    /// The thread in view got archived / deleted: the host goes to the start screen.
    Home,
}

/// A repo tab.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum RepoTab {
    Repo(PathBuf),
    Outside,
    All,
}

impl RepoTab {
    pub(crate) fn keeps(&self, repo: Option<&Path>) -> bool {
        match self {
            RepoTab::Repo(path) => repo == Some(path.as_path()),
            RepoTab::Outside => repo.is_none(),
            RepoTab::All => true,
        }
    }

    fn label(&self) -> String {
        match self {
            RepoTab::Repo(path) => repo_name(path),
            RepoTab::Outside => "outside git".into(),
            RepoTab::All => "all".into(),
        }
    }
}

/// A project tab: every thread, the threads of a project (slug), the threads of 0 projects.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum ProjTab {
    All,
    Project(String),
    None,
}

impl ProjTab {
    pub(crate) fn keeps(&self, projects: &[String]) -> bool {
        match self {
            ProjTab::All => true,
            ProjTab::Project(slug) => projects.contains(slug),
            ProjTab::None => projects.is_empty(),
        }
    }

    fn label(&self) -> String {
        match self {
            ProjTab::All => "all".into(),
            ProjTab::Project(slug) => slug.clone(),
            ProjTab::None => "0 projects".into(),
        }
    }
}

/// A status tab of the active view: the cards of 1 section, or all.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StatusTab {
    Open,
    Stale,
    Shipped,
    All,
}

impl StatusTab {
    const ALL: [StatusTab; 4] = [StatusTab::Open, StatusTab::Stale, StatusTab::Shipped, StatusTab::All];

    /// The tab of section S (`ThreadList::section`).
    fn of(section: u8) -> StatusTab {
        match section {
            0 => StatusTab::Open,
            1 => StatusTab::Stale,
            _ => StatusTab::Shipped,
        }
    }

    pub(crate) fn keeps(self, section: u8) -> bool {
        self == StatusTab::All || self == StatusTab::of(section)
    }

    fn label(self) -> &'static str {
        match self {
            StatusTab::Open => "open",
            StatusTab::Stale => "stale",
            StatusTab::Shipped => "shipped",
            StatusTab::All => "all",
        }
    }

    /// Muted text right of the tabs, while the tab is picked.
    fn hint(self) -> &'static str {
        match self {
            StatusTab::Stale => "0 code or on main, idle 3+ days: go on or archive",
            StatusTab::Shipped => "archive?",
            _ => "",
        }
    }
}

fn repo_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

/// Archive / delete wait for a yes.
struct Pending {
    act: Act,
    ids: Vec<String>,
}

/// A job's result shows this long.
const STATUS_FOR: Duration = Duration::from_secs(8);

/// Thread cards, read in the background (also saved: the next list shows them at once).
fn read_cards() -> Receiver<Vec<Card>> {
    let (tx, rx) = crate::infra::bus::channel();

    std::thread::spawn(move || {
        let cards = thread_card::load();

        crate::infra::background::save("cards", &cards);
        let _ = tx.send(cards);

        crate::infra::bus::wake();
    });

    rx
}

pub(crate) struct ThreadList {
    pub mode: Mode,
    pub cards: Vec<Card>,
    /// The thread in view (the menu over a thread): its repo tab first, `here` on its card.
    here: Option<String>,
    /// The repo of the start screen (its tab first; None: `all` first).
    home_repo: Option<PathBuf>,
    pub filter: TextArea,
    /// Manage: the filter takes the keys (ctrl+f; enter / esc: back to the list).
    pub filtering: bool,
    pub cursor: usize,
    /// 1st virtual row in view.
    offset: u16,
    /// Manage: the picked threads (space).
    pub sel: HashSet<String>,
    repos: TabGroup,
    repo_pick: Option<RepoTab>,
    projs: TabGroup,
    proj_pick: ProjTab,
    views: TabGroup,
    pub view: View,
    stats: TabGroup,
    /// The picked status tab (None: the section of the thread in view, else open).
    pub stat_pick: Option<StatusTab>,
    pub focus: Focus,
    /// Manage: rows of the host's views (worktrees, claude): the counts on the view tabs, their
    /// repos in the repo tabs.
    pub extra: [(usize, Vec<Option<PathBuf>>); 2],
    /// Jobs run on their own thread (tests: at once, the perf case: on their thread).
    pub inline: bool,
    form: Option<Form>,
    /// Agent names (the edit form's agents).
    pub roster: Vec<String>,
    pub list: Rect,
    card_reads: CardReads,
    job_state: JobState,
    hit_regions: HitRegions,
}

impl ThreadList {
    /// CARDS: the host's (empty: the last read at once, a new read in the background).
    pub(crate) fn new(mode: Mode, here: Option<String>, home_repo: Option<PathBuf>, cards: Vec<Card>) -> ThreadList {
        let loading = (cards.is_empty() && !cfg!(test)).then(read_cards);

        let cards = if cards.is_empty() && !cfg!(test) {
            crate::infra::background::snapshot("cards").unwrap_or_default()
        } else {
            cards
        };

        let mut filter = TextArea::new(
            true,
            if mode == Mode::Switch {
                "filter threads"
            } else {
                "ctrl+f: fuzzy search thread names"
            },
        );

        filter.style = Some(theme::text());
        Self {
            mode,
            cards,
            here,
            home_repo,
            filter,
            filtering: false,
            cursor: 0,
            offset: 0,
            sel: HashSet::new(),
            repos: TabGroup::default(),
            repo_pick: None,
            projs: TabGroup::default(),
            proj_pick: ProjTab::All,
            views: TabGroup::default(),
            view: View::Active,
            stats: TabGroup::default(),
            stat_pick: None,
            focus: Focus::List,
            extra: Default::default(),
            inline: cfg!(test),
            form: None,
            roster: Vec::new(),
            list: Rect::default(),
            card_reads: new_card_reads(loading),
            job_state: JobState {
                actions: None,
                pending: None,
                job: None,
                moved: HashMap::new(),
                settle: false,
                status: None,
                changed: false,
            },
            hit_regions: HitRegions {
                rows: Vec::new(),
                act_rows: Vec::new(),
            },
        }
    }

    /// A new read now (a job ended, the host's `r`).
    pub(crate) fn reload(&mut self) {
        if !cfg!(test) && self.card_reads.loading.is_none() {
            self.card_reads.loading = Some(read_cards());
            self.card_reads.cards_at = Some(Instant::now());
        }
    }

    /// A background read on its way, a job, a status that times out: the host draws often.
    pub(crate) fn waiting(&self) -> bool {
        self.card_reads.loading.is_some() || self.job_state.job.is_some() || self.job_state.status.is_some()
    }

    /// A job ended since the last call: the host reads its rows again.
    pub(crate) fn take_changed(&mut self) -> bool {
        std::mem::take(&mut self.job_state.changed)
    }

    /// The list takes esc / enter itself (a form, a question, the dropdown, the filter in edit).
    pub(crate) fn modal(&self) -> bool {
        self.form.is_some() || self.job_state.pending.is_some() || self.job_state.actions.is_some() || self.filtering
    }

    fn done(&mut self, text: String, err: bool) {
        self.job_state.status = Some((text, err, Instant::now()));
        self.job_state.changed = true;

        // a read from before the end: dropped, a new one ends the moved cards
        self.card_reads.loading = None;
        self.job_state.settle = true;
        self.reload();
    }

    /// New cards (a read): the cursor stays on its thread. Moved cards (a job on its way) as
    /// the job leaves them.
    pub(crate) fn set_cards(&mut self, cards: Vec<Card>) {
        set_cards_thread_list(self, cards);
    }

    pub(crate) fn query(&self) -> String {
        self.filter.text()
    }
}

// ---------- edit form ----------

fn push_repo(auto: Option<&PathBuf>, repos: &mut Vec<Option<String>>, repo: String) {
    if !repos.contains(&Some(repo.clone())) && Some(repo.as_str()) != auto.as_ref().and_then(|auto| auto.to_str()) {
        repos.push(Some(repo));
    }
}

fn build_form(
    thread: &crate::engine::store::Thread,
    auto: Option<PathBuf>,
    repos: Vec<Option<String>>,
    repo_i: usize,
    title: TextArea,
    agents: Vec<(String, bool)>,
) -> Form {
    let all = crate::domain::threads::repo_of_thread_store(thread)
        .map(|repo| crate::domain::projects::list(&repo))
        .unwrap_or_default();
    let mut projs: Vec<(String, bool)> = vec![("auto".into(), !thread.owner_choices.projects_set)];

    for project in all
        .iter()
        .filter(|project| !project.done || thread.projects.contains(&project.slug))
    {
        projs.push((project.slug.clone(), thread.projects.contains(&project.slug)));
    }

    // gone briefs: kept until dropped here
    for slug in thread
        .projects
        .iter()
        .filter(|slug| !all.iter().any(|project| &project.slug == *slug))
    {
        projs.push((slug.clone(), true));
    }

    Form {
        id: thread.id.clone(),
        title,
        projs,
        repos,
        auto: auto.map_or_else(|| "outside git".into(), |repo| repo_name(&repo)),
        repo_i,
        links: thread
            .checkouts
            .iter()
            .map(|checkout| (checkout.dir.clone(), true))
            .collect(),
        agents,
        row: 0,
        error: None,
    }
}

#[cfg(test)]
#[path = "thread_list_tests.rs"]
mod tests;

#[path = "thread_list_draw.rs"]
mod draw;
#[path = "thread_list_draw_helpers.rs"]
mod draw_helpers;
#[path = "thread_list_key.rs"]
mod key;
#[path = "thread_list_key_helpers.rs"]
mod key_helpers;
#[path = "thread_list_mouse.rs"]
mod mouse;
#[path = "thread_list_poll.rs"]
mod poll;

mod tabs;

mod selection;

mod actions;

mod hints;

mod form;

/// The edit form of a thread: title, repo (←→), linked repos and agents (space: on / off).
pub(super) struct Form {
    id: String,
    title: TextArea,
    /// Repo picks: None = the repo of its dirs (AUTO).
    repos: Vec<Option<String>>,
    auto: String,
    repo_i: usize,
    /// Row 0: auto (the agents assign), then the projects of the repo: (slug, on).
    projs: Vec<(String, bool)>,
    links: Vec<(String, bool)>,
    agents: Vec<(String, bool)>,
    row: usize,
    error: Option<String>,
}

pub(super) enum FormOut {
    Stay,
    Close,
    Save(String, crate::domain::threads::Edit),
}

fn apply_moved(cards: &mut Vec<Card>, thread_list: &mut ThreadList) {
    cards.retain(|card| !matches!(thread_list.job_state.moved.get(&card.id), Some(None)));

    for card in cards {
        if let Some(Some(state)) = thread_list.job_state.moved.get(&card.id) {
            card.state = state.clone();
        }
    }
}

struct CardReads {
    loading: Option<Receiver<Vec<Card>>>,
    cards_at: Option<Instant>,
    /// `landed::version` of the cards' lands.
    land_v: u64,
    /// Bus topics: a thread changed → the cards again.
    topics: std::sync::mpsc::Receiver<crate::infra::bus::Topic>,
    cards_due: bool,
    /// Data version of the cards (a new read dissolves in).
    version: u64,
    fx: crate::ui::dissolve::Dissolve,
}
struct JobState {
    /// The actions dropdown: its cursor.
    actions: Option<usize>,
    pending: Option<Pending>,
    job: Option<Receiver<(String, bool)>>,
    /// Cards a job acts on, as the job leaves them (Some: their new state; None: gone): shown at
    /// once, over every read until the 1st read after the job's end.
    moved: HashMap<String, Option<String>>,
    /// A job ended: the next read clears `moved`.
    settle: bool,
    status: Option<(String, bool, Instant)>,
    /// A job ended: the host reads its own rows again (`take_changed`).
    changed: bool,
}
struct HitRegions {
    /// Last draw: card rows, dropdown rows, the list.
    rows: Vec<(Rect, usize)>,
    act_rows: Vec<(Rect, Act)>,
}

fn set_cards_thread_list(state: &mut ThreadList, mut cards: Vec<Card>) {
    if !state.job_state.moved.is_empty() {
        apply_moved(&mut cards, state);
    }

    if cards == state.cards {
        return;
    }

    let picked = state.current().map(|card| card.id.clone());

    state.cards = cards;
    state.card_reads.version += 1;
    let ids: HashSet<&str> = state.cards.iter().map(|card| card.id.as_str()).collect();

    state.sel.retain(|id| ids.contains(id.as_str()));

    if let Some(id) = picked
        && let Some(row) = state.visible().iter().position(|&i| state.cards[i].id == id)
    {
        state.cursor = row;
    }
}

fn new_card_reads(loading: Option<Receiver<Vec<Card>>>) -> CardReads {
    CardReads {
        cards_at: loading.is_some().then(Instant::now),
        land_v: crate::domain::landed::version(),
        topics: crate::infra::bus::subscribe(),
        cards_due: false,
        loading,
        version: 0,
        fx: Dissolve::default(),
    }
}
