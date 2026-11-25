//! The source control panel left of the review (ctrl+b): the commit message on top, then
//! sections as VS Code has them: staged, changes (by level), comments (by severity, then file), stash,
//! commits (unpushed, pushed; enter: the commit in the review's place, read only). Section
//! and level labels stick to the top / bottom edge while their rows are out of view; a click
//! on a stuck label scrolls to it.

use std::collections::{BTreeSet, HashSet};
use std::path::PathBuf;
use std::sync::mpsc::Receiver;

use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};
use unicode_width::UnicodeWidthStr;

use super::{Act, Modal, Op, Status, Then, View, patch};
use crate::domain::changes::rank::Level;
use crate::ui::textarea::{Outcome, TextArea};
use crate::ui::theme;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum Sec {
    Staged,
    Changes,
    Comments,
    Stash,
    Commits,
}

impl Sec {
    fn label(self) -> &'static str {
        match self {
            Sec::Staged => "STAGED",
            Sec::Changes => "CHANGES",
            Sec::Comments => "COMMENTS",
            Sec::Stash => "STASH",
            Sec::Commits => "COMMITS",
        }
    }
}

/// A foldable label: a section, a level of the changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Fold {
    Sec(Sec),
    Level(Level),
    /// A severity of the comments.
    CLevel(Level),
    /// The commits' group: pushed (true) / unpushed.
    Push(bool),
    /// The comments' group: resolved (true) / open.
    CState(bool),
}

#[derive(Clone, Debug, PartialEq)]
pub(super) enum PRow {
    Section(Sec, usize),
    Level(Level, usize),
    /// A severity of the comments: its count.
    CLevel(Level, usize),
    /// File (index), in the staged section.
    File(usize, bool),
    /// A file of the comments: path, count.
    CommentFile(String, usize),
    Comment(usize),
    /// Open comments in another checkout: its root, their count (enter: the view goes there).
    Elsewhere(PathBuf, usize),
    Stash(usize),
    Empty(&'static str),
    /// PR mode: the commit picker.
    Pick,
    /// The commits' group (pushed: true): its count.
    Push(bool, usize),
    /// The commits of a running push (above UNPUSHED): their count; 0 keys, 0 clicks.
    Pushing(usize),
    /// A commit of the running push (index in the read's commits); 0 keys, 0 clicks.
    PushingCommit(usize),
    /// A commit (index in the read's commits).
    Commit(usize),
    /// A file of the shown commit: the commit's index, the file's index in the commit's view.
    CommitFile(usize, usize),
    /// The comments' group (resolved: true): its count.
    CState(bool, usize),
    /// The branch's changes against its base, as 1 view (commits + uncommitted).
    All,
}

impl PRow {
    fn label(&self) -> Option<Fold> {
        match self {
            PRow::Section(text, _) => Some(Fold::Sec(*text)),
            PRow::Level(line, _) => Some(Fold::Level(*line)),
            PRow::CLevel(line_value, _) => Some(Fold::CLevel(*line_value)),
            PRow::Push(path, _) => Some(Fold::Push(*path)),
            PRow::CState(result, _) => Some(Fold::CState(*result)),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Btn {
    Restore(String),
    Drop(String),
    Clear,
    /// The UNPUSHED row's push: every unpushed commit, after the question.
    Push,
}

pub(super) struct Panel {
    pub shown: bool,
    /// The keys go to the panel (else the review).
    pub focus: bool,
    pub msg: TextArea,
    pub msg_focus: bool,
    drafting: Option<Receiver<Result<String, String>>>,
    rows: Vec<PRow>,
    pub(crate) at: usize,
    top: usize,
    folded: HashSet<Fold>,
    /// Picked files (space, shift+click): s / p take them.
    pub picked: BTreeSet<String>,
    /// Picked unpushed commits (space): ctrl+. → uncommit takes them, the ship section pushes up to them.
    pub commit_picks: BTreeSet<String>,
    /// Picked comments (ids; space): the next prompt takes them along, then they clear.
    pub comment_picks: BTreeSet<String>,
    anchor: Option<usize>,
    pub rect: Rect,
    msg_rect: Rect,
    /// The list's screen rows: y, row index, a stuck label.
    pub(crate) lines: Vec<(u16, usize, bool)>,
    buttons: Vec<(Rect, Btn)>,
    /// PR mode: the commit dropdown open, its cursor (0: every commit, k: the k-th newest);
    /// its rows (clicks).
    pub(super) pick: Option<usize>,
    pick_rows: Vec<(Rect, usize)>,
}

impl Panel {
    /// The file (index) of the panel's cursor row, with the keys on the panel.
    pub(super) fn file_at(&self) -> Option<usize> {
        match self.rows.get(self.at) {
            Some(PRow::File(file, _)) if self.focus => Some(*file),
            _ => None,
        }
    }

    /// PR mode: the commit dropdown is open.
    pub(super) fn picking(&self) -> bool {
        self.pick.is_some()
    }

    /// The cursor row is a commit.
    pub(super) fn on_commit(&self) -> bool {
        matches!(self.rows.get(self.at), Some(PRow::Commit(_)))
    }

    /// The cursor row is a file of the shown commit.
    pub(super) fn on_commit_file(&self) -> bool {
        matches!(self.rows.get(self.at), Some(PRow::CommitFile(..)))
    }

    pub(super) fn new() -> Panel {
        let mut msg = TextArea::new(false, "message");

        msg.style = Some(theme::panel());
        Panel {
            shown: true,
            focus: false,
            msg,
            msg_focus: false,
            drafting: None,
            rows: Vec::new(),
            at: 0,
            top: 0,
            folded: HashSet::new(),
            picked: BTreeSet::new(),
            commit_picks: BTreeSet::new(),
            comment_picks: BTreeSet::new(),
            anchor: None,
            rect: Rect::default(),
            msg_rect: Rect::default(),
            lines: Vec::new(),
            buttons: Vec::new(),
            pick: None,
            pick_rows: Vec::new(),
        }
    }
}

/// The color of level L.
pub(super) fn level_color(line: Level) -> Color {
    match line {
        Level::Critical => theme::pal().error(),
        // orange, yellow
        Level::High => theme::pal().caution(),
        Level::Medium => theme::pal().attention(),
        Level::Low => theme::pal().subtext0,
    }
}

/// The 1-letter status of a file with status S: A added, M modified, D deleted, R renamed.
fn status_letter(text: Status) -> (char, Color) {
    match text {
        Status::New => ('A', theme::pal().success()),
        Status::Deleted => ('D', theme::pal().error()),
        Status::Renamed => ('R', theme::pal().renamed()),
        Status::Modified => ('M', theme::pal().info()),
    }
}

/// `name` and `dir/` of PATH.
fn split_path(path: &str) -> (&str, &str) {
    match path.rsplit_once('/') {
        Some((directory, count)) => (count, directory),
        None => (path, ""),
    }
}

#[path = "panel_commits.rs"]
mod panel_commits;
#[path = "panel_draw.rs"]
mod panel_draw;
#[path = "panel_groups.rs"]
mod panel_groups;
#[path = "panel_keys.rs"]
mod panel_keys;
#[path = "panel_line.rs"]
mod panel_line;
#[path = "panel_menu.rs"]
mod panel_menu;
#[path = "panel_mouse.rs"]
mod panel_mouse;
#[path = "panel_operations.rs"]
mod panel_operations;
#[path = "panel_picker.rs"]
mod panel_picker;
#[path = "panel_rows.rs"]
mod panel_rows;

#[cfg(test)]
#[path = "changes_panel_tests.rs"]
mod tests;
