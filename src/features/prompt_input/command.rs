//! `>` in a prompt input: a command dropdown above the prompt box (never over it), filtered as the
//! text after the `>` is typed. Levels, each a dropdown beside its parent (0 room beside: above them):
//! - `mention` → kinds (files, notes, threads, artifacts) → the items of the kind: oldest on top,
//!   the latest at the bottom and current; typed text: fuzzy matches, the best at the bottom.
//!   enter / tab inserts the item (relative file path, note path, thread name, artifact url).
//! - `branch` (thread view): the thread copied into a new one, the view switches to it.
//! - `skills` → the skills of the thread's harnesses, with their description; enter inserts `/name`.
//!
//! ↑↓ move (items: the item goes into the text at once, a live preview), enter / tab open the next level. esc (or ← / ⌫ on an empty filter): 1 level back; on
//! the 1st level: closed until the next `>`.

use std::collections::{HashMap, HashSet};
use std::ops::ControlFlow;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::ui::dropdown;
use crate::ui::textarea::{Pos, TextArea};
use crate::ui::theme;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(crate) enum Kind {
    Files,
    Notes,
    Threads,
    Artifacts,
    Skills,
}

/// A command the host runs (it has the thread).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Action {
    Branch,
    Handoff,
    Reload,
    /// The flow box of the agent a message goes to now, on the flows (change / set its flow).
    Flow,
}

/// A dropdown of the open path.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Level {
    Commands,
    /// Kinds of `mention`.
    Kinds,
    Items(Kind),
}

const MENTION: usize = 0;
const BRANCH: usize = 1;
const SKILLS: usize = 2;
const HANDOFF: usize = 3;
const RELOAD: usize = 4;
const FLOW: usize = 5;
const SUB: usize = 6;
/// 1st level: name, what it does.
const COMMANDS: [(&str, &str); 7] = [
    ("mention", "path · name · url"),
    ("branch", "copy of this thread"),
    ("skills", "of the harnesses"),
    ("handoff", "new thread from a summary of this one"),
    ("reload", "new harness sessions: MCP servers, settings"),
    ("flow", "set / change the flow of the agent a message goes to"),
    ("sub", "sub-thread: 0 history, same checkout, reports here"),
];
/// 2nd level of `mention`: kind, name, what goes into the text.
const KINDS: [(Kind, &str, &str); 4] = [
    (Kind::Files, "files", "relative path"),
    (Kind::Notes, "notes", "path"),
    (Kind::Threads, "threads", "name"),
    (Kind::Artifacts, "artifacts", "url"),
];

/// Widest item dropdown (columns); skills (name + description): wider.
const MAX_W: usize = 72;
const MAX_W_SKILLS: usize = 110;
/// Most rows of the description box above the skills.
const INFO_ROWS: usize = 6;
/// Most files listed (the newest win).
const MAX_FILES: usize = 50_000;

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct Item {
    /// Shown, and what the typed text matches.
    pub label: String,
    /// Shown right, gray (age, place, description), cut to the room.
    pub detail: String,
    /// Goes into the text.
    pub insert: String,
    /// Seconds since the epoch (skills: the row number): the order.
    pub time: u64,
    /// Long text of the current row, in a box above the dropdown (skills: the description).
    pub info: String,
}

#[derive(Default)]
pub(crate) struct Command {
    /// The `>` this state belongs to. Another `>`: 1st level again.
    at: Option<Pos>,
    /// esc on the 1st level of the `>` at this position: closed until another `>`.
    closed: Option<Pos>,
    /// Open levels below the commands (`Kinds`, `Items`).
    path: Vec<Level>,
    /// Per level: the row (menus: its index in COMMANDS / KINDS once a submenu is open), the 1st row in view.
    cursor: [usize; 3],
    offset: [usize; 3],
    /// Typed text the cursor of the open level belongs to (new text: the cursor goes to its default).
    seen: Option<(usize, String)>,
    items: Vec<Item>,
    /// Items in view order for the filter text (index into `items`).
    matched: (String, Vec<usize>),
    /// Widest label + detail of the items.
    items_w: usize,
    /// The artifact scan runs: the items come later.
    loading: bool,
    reads: crate::infra::background::Reads<String, Vec<Item>>,
    cache: HashMap<String, Vec<Item>>,
    source: Option<String>,
    title_reads: crate::infra::background::Reads<String, Option<String>>,
    title_checked: HashSet<String>,
    /// ↑↓ on items: the item in the text in place of `>query` (the typed query: the filter).
    preview: dropdown::Preview,
    /// Rows of the last draw (mouse): area, level, row.
    rows: Vec<(Rect, usize, usize)>,
    /// Dropdown areas of the last draw.
    areas: Vec<Rect>,
    /// Files: the repo / worktree (else the dir) they come from.
    pub cwd: PathBuf,
    /// A thread is open: `branch` shows.
    pub can_branch: bool,
    /// Harnesses of the thread's agents (task screen: of the picked agents), for `skills`.
    pub harnesses: Vec<String>,
    /// `branch` picked: the host takes it.
    pub action: Option<Action>,
}

fn secs(thread: SystemTime) -> u64 {
    thread
        .duration_since(UNIX_EPOCH)
        .map_or(0, |directory| directory.as_secs())
}

fn age(time: u64) -> String {
    if time == 0 {
        String::new()
    } else {
        crate::domain::notes::ago(UNIX_EPOCH + Duration::from_secs(time))
    }
}

/// Fuzzy score of Q in a path-like LABEL, like the VS Code file picker: Q's chars in order, a run
/// and a word start score more, a match in the last path part counts double. None: 0 match.
pub(crate) fn score(query: &str, label: &str) -> Option<i64> {
    let query_value = query.to_lowercase();
    let whole = crate::domain::notes::fuzzy(&query_value, label)?;
    let base = label.rsplit('/').next().unwrap_or(label);
    let in_base = crate::domain::notes::fuzzy(&query_value, base).map_or(0, |text| text * 2);

    // a shorter label with the same match: closer to what was typed
    Some(whole.max(in_base) * 4 - (label.chars().count() as i64).min(200) / 8)
}

/// Items in view order for Q: all by time (oldest first, the latest last); with text: the matches,
/// the best last (ties: the newer last). A match in the long text (`info`) only: before the others.
pub(crate) fn matches(items: &[Item], query: &str) -> Vec<usize> {
    if query.is_empty() {
        return (0..items.len()).collect();
    }

    let in_info = |it: &Item| {
        if it.info.is_empty() {
            None
        } else {
            crate::domain::notes::fuzzy(&query.to_lowercase(), &it.info)
                .filter(|text| *text >= 100)
                .map(|text_value| text_value - 10_000)
        }
    };

    let mut value: Vec<(i64, usize)> = items
        .iter()
        .enumerate()
        .filter_map(|(i, it)| {
            score(query, &it.label)
                .or_else(|| in_info(it))
                .map(|text_2| (text_2, i))
        })
        .collect();

    value.sort_by_key(|(text_3, i)| (*text_3, items[*i].time, *i));
    value.into_iter().map(|(_, i)| i).collect()
}

/// The last W columns of S (`…` in front when cut): a path keeps its file name.
fn tail(text: &str, width_value: usize) -> String {
    if text.width() <= width_value {
        return text.to_string();
    }

    let mut out: Vec<char> = Vec::new();
    let mut used = 1;

    for character in text.chars().rev() {
        let cw = character.width().unwrap_or(0);

        if used + cw > width_value {
            break;
        }

        used += cw;
        out.push(character);
    }

    out.push('…');

    out.iter().rev().collect()
}

/// The first W columns of S (`…` at the end when cut).
fn head(text: &str, width_value: usize) -> String {
    if text.width() <= width_value {
        return text.to_string();
    }

    let mut out = String::new();
    let mut used = 1;

    for character in text.chars() {
        let cw = character.width().unwrap_or(0);

        if used + cw > width_value {
            break;
        }

        used += cw;
        out.push(character);
    }

    out.push('…');

    out
}

impl Command {
    pub(crate) fn new(cwd: &Path) -> Command {
        Command {
            cwd: cwd.to_path_buf(),
            ..Default::default()
        }
    }

    /// The `>` under the cursor + the text after it. None: closed (state back to the 1st level).
    fn sync(&mut self, ta: &TextArea) -> Option<(Pos, String)> {
        sync_command(self, ta)
    }

    /// The artifact scan runs: draw again soon.
    pub(crate) fn busy(&self) -> bool {
        self.loading || self.reads.pending() > 0 || self.title_reads.pending() > 0
    }

    fn load(&mut self, kind: Kind) {
        load_command(self, kind);
    }

    /// Skills read top-down (a → z, the 1st row current); the other lists: the latest at the bottom, current.
    fn from_top(level: Level) -> bool {
        !matches!(level, Level::Items(kind) if kind != Kind::Skills)
    }

    /// Rows of the open level for Q, the cursor on its default after new text.
    fn settle(&mut self, query: &str) -> usize {
        settle_command(self, query)
    }

    /// Dropdown areas of the last draw.
    #[cfg(test)]
    pub(crate) fn areas(&self) -> &[Rect] {
        &self.areas
    }

    /// The dropdown shows (as `key` sees it: a deeper level, or 1+ command matching the text).
    pub(crate) fn is_open(&mut self, ta: &TextArea) -> bool {
        let Some((_, query)) = self.sync(ta) else { return false };

        self.settle(&query) > 0 || self::depth(self) > 0
    }

    /// Row of the last draw at P (hover).
    pub(crate) fn row_at(&self, path: Position) -> Option<Rect> {
        self.rows
            .iter()
            .map(|(result, _, _)| *result)
            .find(|result_value| result_value.contains(path))
    }
}

#[cfg(test)]
#[path = "prompt_input_command_tests.rs"]
mod tests;

#[path = "command_key.rs"]
mod key;
#[path = "command_mouse.rs"]
mod mouse;
#[path = "command_render.rs"]
mod render;
#[path = "command_updates.rs"]
mod updates;

#[cfg(test)]
fn set_test_items(kind: Kind, items: Vec<Item>, command: &mut Command) {
    command.path = if kind == Kind::Skills {
        vec![Level::Items(kind)]
    } else {
        vec![Level::Kinds, Level::Items(kind)]
    };

    command.items = items;
    self::set_items(command);
}

#[cfg(test)]
fn open_level(ta: &TextArea, command: &mut Command) -> Option<usize> {
    command.sync(ta).map(|_| self::depth(command))
}

#[path = "command_sources.rs"]
mod sources;
use sources::threads;
pub(crate) use sources::{files, notes, skills};

#[path = "command_artifacts.rs"]
mod artifacts;
use artifacts::remote_artifact_title;
pub(crate) use artifacts::scan_artifacts;
#[cfg(test)]
use artifacts::{page_title, transcript_titles};

fn apply_current_source(source: &str, items: Vec<Item>, command: &mut Command) {
    if command.source.as_deref() == Some(source) {
        command.loading = false;
        command.update_items(items);
    }
}

fn sync_command(command_value: &mut Command, ta: &TextArea) -> Option<(Pos, String)> {
    // a live preview in the text: the `>query` typed before it stays open
    if let Some((at, typed)) = command_value.preview.typed(ta)
        && command_value.at == Some(at)
    {
        return Some((at, typed.chars().skip(1).collect()));
    }

    let found = ta.command();

    // the cursor left the closed `>`: a `>` typed there later opens again
    if found.is_none() {
        command_value.closed = None;
    }

    let Some((at, query)) = found.filter(|(at, _)| command_value.closed != Some(*at)) else {
        self::reset(command_value);
        return None;
    };

    if command_value.at != Some(at) {
        self::reset(command_value);
        command_value.at = Some(at);
    }

    Some((at, query))
}

fn load_command(command: &mut Command, kind: Kind) {
    let cwd = command.cwd.clone();
    let harnesses = command.harnesses.clone();
    let root = crate::infra::config::notes();
    let scope = CacheScope {
        kind,
        cwd: &cwd,
        root: &root,
        harnesses: &harnesses,
    }
    .to_string();
    let name = format!("mention-{}", crate::infra::background::scope(Path::new(&scope)));
    let cached = command
        .cache
        .entry(scope.clone())
        .or_insert_with(|| crate::infra::background::snapshot(&name).unwrap_or_default());

    command.items = cached.clone();
    command.loading = command.items.is_empty();
    command.source = Some(scope.clone());

    command.reads.request(scope, move || {
        let items = match kind {
            Kind::Files => files(&cwd),
            Kind::Notes => notes(&root),
            Kind::Threads => threads(),
            Kind::Skills => skills(&harnesses, &cwd),
            Kind::Artifacts => scan_artifacts(&crate::infra::config::home().join(".claude/projects")),
        };

        crate::infra::background::save(&name, &items);
        items
    });

    self::set_items(command);
}

fn settle_command(command: &mut Command, query: &str) -> usize {
    for (source, items) in command.reads.poll() {
        command.cache.insert(source.clone(), items.clone());

        apply_current_source(&source, items, command);
    }

    for (url, title) in command.title_reads.poll() {
        if let Some(title) = title {
            update_cached_titles(command, &url, &title);

            let mut items = command.items.clone();

            update_artifact_labels(&url, &mut items, &title);

            command.update_items(items);
        }
    }

    if self::level(self::depth(command), command) == Level::Items(Kind::Artifacts) {
        request_artifact_titles(command);
    }

    let directory = self::depth(command);
    let level = self::level(directory, command);

    let count = if let Level::Items(_) = level {
        update_matches(command, query, level);

        command.matched.1.len()
    } else {
        self::menu(level, query, command).len()
    };

    if command
        .seen
        .as_ref()
        .is_none_or(|(sd, sq)| *sd != directory || sq != query)
    {
        let top = Command::from_top(level);

        command.cursor[directory] = if top { 0 } else { count.saturating_sub(1) };

        command.offset[directory] = if top {
            0
        } else {
            count.saturating_sub(dropdown::MAX_ROWS)
        };

        command.seen = Some((directory, query.to_string()));
    }

    command.cursor[directory] = command.cursor[directory].min(count.saturating_sub(1));

    count
}

fn update_matches(command: &mut Command, query: &str, level: Level) {
    if command.matched.0 != query {
        let mut message = matches(&command.items, query);

        // top-down list: the best match on top
        if Command::from_top(level) && !query.is_empty() {
            message.reverse();
        }

        command.matched = (query.to_string(), message);
    }
}

fn update_cached_titles(command: &mut Command, url: &String, title: &String) {
    for items in command.cache.values_mut() {
        for item in items.iter_mut().filter(|i| i.insert == *url) {
            title.clone_into(&mut item.label);
        }
    }
}

fn request_artifact_titles(command: &mut Command) {
    for item in &command.items {
        if command.title_reads.pending() >= 4 {
            break;
        }

        if item.label.starts_with("untitled artifact · ") && command.title_checked.insert(item.insert.clone()) {
            let url = item.insert.clone();

            command
                .title_reads
                .request(url.clone(), move || remote_artifact_title(&url));
        }
    }
}

fn update_artifact_labels(url: &str, items: &mut [Item], title: &str) {
    for item in items.iter_mut().filter(|i| i.insert == url) {
        title.clone_into(&mut item.label);
    }
}

#[path = "command_navigation.rs"]
mod navigation;
use navigation::{activate, back, depth, level, menu, reset, set_items};

struct CacheScope<'scope> {
    kind: Kind,
    cwd: &'scope Path,
    root: &'scope Path,
    harnesses: &'scope [String],
}

impl std::fmt::Display for CacheScope<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{:?}:", self.kind)?;
        std::fmt::Debug::fmt(self.cwd, formatter)?;
        formatter.write_str(":")?;
        std::fmt::Debug::fmt(self.root, formatter)?;
        write!(formatter, ":{:?}", self.harnesses)
    }
}
