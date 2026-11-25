//! The changes view of a thread: the uncommitted changes (staged + unstaged + untracked; the
//! panel's "all changes": the branch against its merge-base), 1 review box per file, ordered by what to
//! review first (schema / API → logic → config → UI → docs → tests → generated). A box marked
//! reviewed folds; a reviewed file whose diff changed shows `updated`. Unified or side by side by
//! the room. Marks: `<thread dir>/review.json` (path → hash of the file's diff).

pub(crate) mod review;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
#[cfg(test)]
use std::process::Command;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use unicode_width::UnicodeWidthStr;

use crate::ui::theme;

mod scm;

/// Side by side from this many columns.
pub(crate) const SIDE_MIN: u16 = 170;

/// Review marks of thread ID: path → diff hash at the mark.
fn marks_path(id: &str) -> PathBuf {
    crate::engine::store::thread_dir(id).join("review.json")
}

fn read_marks(id: &str) -> HashMap<String, u64> {
    std::fs::read(marks_path(id))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

fn write_marks(id: &str, marks: &HashMap<String, u64>) {
    if let Ok(bytes) = serde_json::to_vec_pretty(marks) {
        let _ = crate::engine::write_atomic(&marks_path(id), &bytes);
    }
}

/// What a key or click asks the thread view to do.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Act {
    None,
    /// Open FILE (relative to the worktree) at LINE in the IDE.
    Open(String, u32),
    Toast(String),
    /// Send this text to the thread (the review comments batch).
    Send(String),
    /// A sub-thread of the thread does this task (`>sub TASK`; the ship section's agent steps).
    Sub(String),
    /// The chat above the prompt: open / folded.
    ToggleChat,
    /// The view on the checkout at this root (its comments).
    Checkout(PathBuf),
    /// An `@agent` in a comment / reply of the owner: TEXT to agents TO at once, CONTEXT (the
    /// comment's thread) for the agents only.
    Mention {
        to: Vec<String>,
        text: String,
        context: String,
    },
}

/// Lines of 1 file a comment is on: LINE (new side) shows it; LINES (`11-13, 15 · removed 14`)
/// names a selection ("": LINE alone).
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Spot {
    pub path: String,
    pub line: u32,
    pub lines: String,
}

impl Spot {
    /// `src/a.rs:11-13, 15`.
    pub(crate) fn label(&self) -> String {
        format!(
            "{}:{}",
            self.path,
            if self.lines.is_empty() {
                self.line.to_string()
            } else {
                self.lines.clone()
            }
        )
    }
}

/// What a row shows; its line is made when it comes into view.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Body {
    Conflict,
    Blank,
    /// A group head: the model's group, else the category.
    Group(Option<usize>, Cat),
    /// The filter's result count.
    Hits(usize),
    Head,
    Foot,
    Why,
    Note(&'static str),
    Hunk(usize),
    /// Unified: hunk, line.
    Diff(usize, usize),
    /// Side by side: hunk, the removed (or context) line, the added (or context) line.
    Side(usize, Option<usize>, Option<usize>),
    /// Finding I, its row K (0: the title).
    Finding(usize, usize),
    /// Comment I, its text line K.
    Comment(usize, usize),
    /// GitHub thread I of the pull request (PR mode), its line K.
    Gh(usize, usize),
}

/// A row: what it shows, the file it belongs to, the new-side line it points at.
#[derive(Clone, Debug)]
struct Row {
    body: Body,
    file: usize,
    at: Option<u32>,
    /// A box's header row.
    head: bool,
    /// A hunk's 1st row.
    hunk: bool,
    /// A review comment's row: its index.
    comment: Option<usize>,
    /// A reviewer finding's row: its index.
    finding: Option<usize>,
}

impl Row {
    fn new(body: Body, file: usize, at: Option<u32>) -> Row {
        let (comment, finding) = match body {
            Body::Comment(index, _) => (Some(index), None),
            Body::Finding(index, _) => (None, Some(index)),
            _ => (None, None),
        };

        Row {
            body,
            file,
            at,
            head: body == Body::Head,
            hunk: matches!(body, Body::Hunk(_)),
            comment,
            finding,
        }
    }
}

/// A diff line of a file, for the selection: (old side, its number); added and context lines:
/// the new side.
type LKey = (bool, u32);

fn lkey(line: &DLine) -> Option<LKey> {
    match line.kind {
        Kind::Del => line.old.map(|number| (true, number)),
        _ => line.new.map(|number| (false, number)),
    }
}

/// `11-13, 15 · removed 14` of KEYS (sorted).
fn lines_label(keys: &std::collections::BTreeSet<LKey>) -> String {
    let new: Vec<u32> = keys.iter().filter(|key| !key.0).map(|key| key.1).collect();
    let old: Vec<u32> = keys.iter().filter(|key| key.0).map(|key| key.1).collect();

    match (new.is_empty(), old.is_empty()) {
        (_, true) => line_runs(&new),
        (true, false) => format!("removed {}", line_runs(&old)),
        (false, false) => format!("{} · removed {}", line_runs(&new), line_runs(&old)),
    }
}

pub(crate) struct View {
    operation_state: operation_state::OperationState,
    pull_request: pull_request::PullRequest,
    reviewer: reviewer::Reviewer,
    comment_state: comment_state::CommentState,
    stash_state: stash_state::StashState,
    modal_state: modal_state::ModalState,
    selection_state: selection_state::SelectionState,
    filter_state: filter_state::FilterState,
    classifier: classifier::Classification,
    layout: layout::Layout,
    id: String,
    dir: PathBuf,
    pub changes: Changes,
    marks: HashMap<String, u64>,
    /// Boxes opened by hand (reviewed / generated: folded by default) or closed by hand.
    toggled: HashSet<String>,
    /// Side by side: None: by the room.
    side: Option<bool>,
    /// Cursor row, 1st row in view.
    at: usize,
    top: usize,
    loading: Option<Receiver<Result<Changes, String>>>,
    loaded: Option<Instant>,
    /// Bus topics: a change in this checkout reads git again (0 timer).
    topics: Receiver<crate::infra::bus::Topic>,
    /// This checkout's work tree stays watched while the view lives.
    _shown: crate::infra::watch::Shown,
    /// (top dir, common git dir, own git dir), canonical: the watcher's paths.
    git: Option<(PathBuf, PathBuf, PathBuf)>,
    /// A change came while a read ran: 1 more read after it.
    stale: bool,
    pub error: Option<String>,
    /// After a re-read: the file the cursor was on, its row in the file's rows.
    pending_cursor: Option<(String, usize)>,
    /// The source control panel (tab: the keys to it; context menu: show / hide).
    scm: scm::Panel,
    /// `d d`: the 1st `d` armed the delete of this row (panel: true, row index); the key before
    /// this one's arm (any other key between: 0 delete).
    del: Option<(bool, usize)>,
    del_prev: Option<(bool, usize)>,
    /// The commits a running push takes (oids, newest first): the PUSHING group, since when.
    pub(crate) pushing: Vec<String>,
    pub(crate) pushing_at: Option<Instant>,
    /// The push of another process (an agent, shell mode) its lock shows: who. HOLD: it ended,
    /// its commits stay in PUSHING until the read after it.
    pub(crate) push_by: Option<String>,
    push_hold: bool,
    /// Tests: the branch the agent steps name (0 git repo).
    #[cfg(test)]
    pub(crate) branch_override: Option<String>,
    /// A pull request's diff (the merge tab): read only, 0 git steps.
    pub pr_src: Option<PrSrc>,
    /// A commit view: this commit's diff, read only (0 comments, marks, git steps).
    commit_src: Option<commits::Commit>,
    /// The commit the panel's cursor showed (enter on a commit): it takes the review's place.
    peek: Option<Box<View>>,
    /// The agents (the `@` dropdown of the comment modal), the thread's members.
    pub agents: Vec<crate::domain::agents::Agent>,
    pub members: Vec<String>,
    /// The branch's changes against its merge-base (the panel's "all changes" row): comments and
    /// reviewed marks (own file), 0 git steps. Else the uncommitted changes.
    all: bool,
    /// Where the reviewed marks live (`<id>~all` for the branch view: its diffs differ).
    marks_id: String,
}

mod classification;
mod comment_editor;
mod comment_navigation;
mod context_menu;
mod draw;
mod keys;
mod lifecycle;
mod modal;
mod mouse;
mod navigation;
mod operations;
mod refresh;
mod review_state;
mod row_draw;
mod rows;
mod selection;
mod ship;
mod view;

/// A key of a commit view (read only): it moves, folds, filters, reads again.
fn read_only_key(key: KeyEvent) -> bool {
    let plain = !key
        .modifiers
        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER);

    match key.code {
        KeyCode::Up
        | KeyCode::Down
        | KeyCode::Left
        | KeyCode::Right
        | KeyCode::PageUp
        | KeyCode::PageDown
        | KeyCode::Home
        | KeyCode::End
        | KeyCode::Esc => true,
        KeyCode::Enter => plain,
        // 0 o / e: the worktree's file may differ from the commit's (other lines, deleted)
        KeyCode::Char(character) => matches!(
            character,
            'j' | 'k' | 'n' | 'N' | 'g' | 'G' | 'w' | 'f' | '/' | 't' | 'r'
        ),
        _ => false,
    }
}

/// The thread of comment C for the modal: the comment, then its replies (UPTO: the ones before
/// that reply, the edited one).
fn thread_lines(comment: &Comment, upto: Option<usize>) -> Vec<QLine> {
    let mut out = vec![QLine::Gap];

    let mut add = |head: String, text: &str| {
        out.push(QLine::Note(true, head));
        out.extend(text.lines().map(|line| QLine::Note(false, format!("  {line}"))));
    };

    add(format!("{} · {}", comment.author(), comment.id), &comment.text);

    for reply in comment.replies.iter().take(upto.unwrap_or(usize::MAX)) {
        add(format!("↳ {} · {}", reply.author(), stamp(reply.at)), &reply.text);
    }

    out
}

/// Unix ms AT as `14:03`; 0: "".
fn stamp(at: u64) -> String {
    if at == 0 {
        String::new()
    } else {
        crate::domain::chat::clock(at)
    }
}

/// A row of a comment in the file view: its head, a text line, a reply's head, a reply's text
/// line.
#[derive(Clone, Copy, Debug, PartialEq)]
enum CLine {
    Head,
    Text(usize),
    Reply(usize),
    RText(usize, usize),
}

/// The comment modal's text input with TEXT.
fn comment_area(text: &str) -> Box<crate::ui::textarea::TextArea> {
    let mut area = crate::ui::textarea::TextArea::new(false, "the comment · ctrl+⏎ save");

    area.style = Some(theme::panel());
    area.set_text(text);
    area.move_end();
    Box::new(area)
}

/// The text of a new reply.
fn reply_area() -> Box<crate::ui::textarea::TextArea> {
    let mut area = comment_area("");

    area.placeholder = "the reply · @agent: it gets the thread at once · ctrl+⏎ save".into();
    area
}

/// The code's background inside the boxes: the panel color, darker.
fn code_bg() -> ratatui::style::Color {
    theme::tint(ratatui::style::Color::Rgb(0, 0, 0), theme::pal().surface0, 0.55)
}

/// The panel shows from this many columns.
const PANEL_MIN: u16 = 90;
/// 2 right clicks within this time: the selection clears.
const DOUBLE_CLICK: Duration = Duration::from_millis(400);

/// The worktree line of each line of hunk H: added and context lines their own, a removed line
/// the line it sat before.
fn worktree_lines(hunk: &Hunk) -> Vec<u32> {
    let mut next = hunk.new.max(1);

    hunk.lines
        .iter()
        .map(|line| {
            if line.kind == Kind::Del {
                next
            } else {
                let at = line.new.unwrap_or(next);

                next = at + 1;
                at
            }
        })
        .collect()
}

/// The patch pick of KEYS (selected lines) in FILE: added lines by number, removed lines by (the
/// new-side line they sit before, their text).
fn pick_of(file: &FileDiff, keys: &std::collections::BTreeSet<LKey>) -> patch::Pick {
    let mut pick = patch::Pick::default();

    for hunk in &file.hunks {
        pick_hunk(keys, &mut pick, hunk, hunk.new);
    }

    pick
}

fn pick_hunk(keys: &std::collections::BTreeSet<(bool, u32)>, pick: &mut patch::Pick, hunk: &Hunk, mut new_line: u32) {
    for line in &hunk.lines {
        pick_line(keys, pick, &mut new_line, line);
    }
}

fn pick_line(keys: &std::collections::BTreeSet<(bool, u32)>, pick: &mut patch::Pick, new_line: &mut u32, line: &DLine) {
    match line.kind {
        Kind::Add => {
            if keys.contains(&(false, line.new.unwrap_or(0))) {
                pick.adds.insert(*new_line);
            }

            *new_line += 1;
        }
        Kind::Ctx => *new_line += 1,
        Kind::Del => {
            if line.old.is_some_and(|old| keys.contains(&(true, old))) {
                pick.dels.insert((*new_line, line.text.clone()));
            }
        }
    }
}

#[cfg(test)]
#[path = "changes_support_tests.rs"]
mod tests_support;

#[cfg(test)]
#[path = "changes_tests.rs"]
mod tests;

mod classifier;
mod comment_state;
mod filter_state;
mod layout;
mod modal_state;
mod operation_state;
mod pull_request;
mod reviewer;
mod selection_state;
mod stash_state;

pub(crate) use crate::domain::changes::*;

mod modal_draw;

mod paint;
#[cfg(test)]
pub(crate) use paint::word_marks;
use paint::{Painted, body_text, paint, painted_spans, sign_span};
#[cfg(test)]
use paint::{SATURATION, tints};
mod source;
pub(crate) use source::PrSrc;
use source::{gh_lines, load_commit, load_pr};
mod modal_types;
use modal_types::{Modal, QLine, Ship, ShipGroup, ShipRow, Target, Then};

fn line_runs(nums: &[u32]) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;

    while i < nums.len() {
        let mut j = i;

        while j + 1 < nums.len() && nums[j + 1] == nums[j] + 1 {
            j += 1;
        }

        out.push(if j > i {
            format!("{}-{}", nums[i], nums[j])
        } else {
            nums[i].to_string()
        });
        i = j + 1;
    }

    out.join(", ")
}
