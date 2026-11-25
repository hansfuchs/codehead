//! The merge tab: the pull requests of a thread (the branches of its checkouts, the PR links in
//! its log), or 1 PR without a thread (the start screen's merge tab). 1 tab per PR
//! (`platform#1234`), 2 views each: overview (hero, actions, checks, comments, description) and
//! changes (the changes view on the PR's diff, a commit picker in its panel).
//!
//! Keys: ↑↓ (j k) move between the things to act on (not lines), ←→ along a row, enter acts,
//! ctrl+enter / ctrl+click opens on GitHub, [ ] the previous / next PR, < > overview / changes,
//! r reads again. Edits happen in place (inline inputs): ctrl+enter (alt+enter) saves, esc drops.
//!
//! gh use: 1 GraphQL call per read of a PR. The PR in view: every 30 s (every 12 s for 2 min
//! after an edit, 20 s while checks run); 0 reads after 5 min without a key (sleep, until the
//! next key) or with the tab out of view; max. 20 calls per minute; `r`: at once (3 s apart).

pub(crate) mod pr_list;

pub(crate) mod merge_tab;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use unicode_width::UnicodeWidthStr;

use crate::infra::gh::{self, Kind, Op, Pr, PrRef, Ready, State, Verdict};
use crate::ui::textarea::{Outcome, TextArea};
use crate::ui::theme;

const POLL: Duration = Duration::from_secs(30);
const POLL_FAST: Duration = Duration::from_secs(12);
const POLL_CHECKS: Duration = Duration::from_secs(20);
/// Fast polls this long after an edit.
const FAST_FOR: Duration = Duration::from_secs(120);
/// 0 polls after this long without a key.
const SLEEP: Duration = Duration::from_secs(300);
/// The PR list of a thread: read again after this long.
const ASSOC: Duration = Duration::from_secs(300);
const MIN_MANUAL: Duration = Duration::from_secs(3);
const MAX_PER_MIN: usize = 20;
/// A 2nd enter on merge within this long merges.
const ARM: Duration = Duration::from_secs(4);

/// What the host (thread view, start screen) does after a key / click.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Act {
    None,
    Toast(String),
    /// A message for the agents (thread) / the new-thread view (0 thread), the owner sends it.
    Prompt(String),
    /// Open the changes view's file in the IDE.
    Open(String, u32),
}

/// A thing to act on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stop {
    Tabs,
    Subs,
    Title,
    Status,
    Label(usize),
    AddLabel,
    Update,
    Merge,
    Checks,
    Check(usize),
    NewComment,
    /// Thread I of the conversation, its comment K (0: the 1st, k: reply k).
    Comment(usize, usize),
    Body,
}

impl Stop {
    /// The row of stops it shares (←→ move along it).
    fn row(self) -> u8 {
        match self {
            Stop::Label(_) | Stop::AddLabel => 1,
            Stop::Update | Stop::Merge => 2,
            _ => 0,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Sub {
    #[default]
    Overview,
    Changes,
}

/// An inline input: what it edits.
enum Edit {
    Title,
    Body,
    /// A new comment (a quote of another one, or empty).
    Comment,
    /// A reply on line thread I (its 1st comment's id).
    Reply(usize, u64),
    /// Comment K of thread I (its kind, id).
    Change(usize, usize, Kind, u64),
}

enum Menu {
    /// The status dropdown: its options.
    Status(usize, Vec<(&'static str, Op)>),
    /// The own comment's dropdown: edit, delete (armed: a 2nd enter deletes).
    Mine {
        cur: usize,
        thread: usize,
        comment: usize,
        armed: bool,
    },
    /// The labels dropdown: the filter, the repo's labels (None: on their way).
    Labels { cur: usize, filter: Box<TextArea> },
}

/// Enter on stop S of tab T: true on the 2nd enter within ARM (a 1st arms it).
fn twice(tab: &mut Tab, stop: Stop) -> bool {
    if tab
        .input
        .armed
        .is_some_and(|(armed, at)| armed == stop && at.elapsed() < ARM)
    {
        tab.input.armed = None;
        return true;
    }

    tab.input.armed = Some((stop, Instant::now()));
    false
}

/// 1 PR's tab.
#[derive(Default)]
struct Tab {
    pr: Option<Pr>,
    error: Option<String>,
    sub: Sub,
    focus: Option<Stop>,
    scroll: usize,
    /// A mouse scroll moved the view: the focused stop need not be in view (a key: it is again).
    free: bool,
    checks_open: bool,

    changes: Option<crate::features::changes::View>,

    input: EditState,
    reads: ReadState,
}

/// The disk cache of PR's last read: `prs/pr/<owner>/<name>/<number>.json` in the data dir.
fn cache_path(pr: &PrRef) -> PathBuf {
    crate::engine::data_dir()
        .join("prs")
        .join("pr")
        .join(format!("{}.json", pr.key()))
}

fn cache_read(pr: &PrRef) -> Option<Pr> {
    serde_json::from_slice(&std::fs::read(cache_path(pr)).ok()?).ok()
}

fn cache_write(pr: &Pr) {
    let path = cache_path(&pr.pr);

    if let (Some(dir), Ok(bytes)) = (path.parent(), serde_json::to_vec(pr)) {
        let _ = std::fs::create_dir_all(dir);
        let _ = crate::engine::write_atomic(&path, &bytes);
    }
}

enum Res {
    Pr(PrRef, Box<Result<Pr, String>>),
    Prs(Result<Vec<PrRef>, String>),
    Op(PrRef, Op, Result<(), String>),
    Labels(String, Result<Vec<gh::Label>, String>),
}

/// What a thread's PRs come from.
#[derive(Clone, Debug, Default)]
pub(crate) struct Sources {
    /// (checkout dir, branch).
    pub checkouts: Vec<(PathBuf, Option<String>)>,
    /// The PR links of the log.
    pub links: Vec<PrRef>,
}

pub(crate) struct Merge {
    /// The thread (None: 1 PR, the start screen).
    thread: bool,
    thread_id: String,
    pub prs: Vec<PrRef>,
    cur: usize,
    tabs: HashMap<PrRef, Tab>,
    tx: crate::infra::bus::Tx<Res>,
    rx: Receiver<Res>,
    sources: Option<Sources>,
    /// Data version: up with each read that brings new data (its area dissolves in).
    version: u64,
    fx: crate::ui::dissolve::Dissolve,
    assoc_at: Option<Instant>,
    assoc_busy: bool,
    assoc_error: Option<String>,
    /// The owner's last key / click (sleep after SLEEP).
    active: Instant,
    manual_at: Option<Instant>,
    /// The times of the calls in the last minute.
    calls: Vec<Instant>,
    /// The repo labels, by repo.
    labels: HashMap<String, Vec<gh::Label>>,
    /// The checkout of each repo (the changes view's dir, the agents' hint).
    dirs: HashMap<String, PathBuf>,

    pub note: Option<String>,

    layout: MergeLayout,
}

/// A row of the overview: its line, the stops on it (x from, width, stop).
struct Row {
    line: Line<'static>,
    stops: Vec<(u16, u16, Stop)>,
    /// The inline input goes here (its rows).
    input: u16,
}

impl Row {
    fn stop(line: Line<'static>, stop: Stop) -> Row {
        let width = line.width() as u16;

        Row {
            line,
            stops: vec![(0, width, stop)],
            input: 0,
        }
    }
}

fn hex(code: &str) -> Color {
    let rgb = u32::from_str_radix(code.trim_start_matches('#'), 16).unwrap_or(0x0080_8080);

    Color::Rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
}

/// Text on a label of color BG: dark on light, light on dark.
fn on(bg: Color) -> Color {
    let (red, green, blue) = theme::rgb(bg);

    if (u32::from(red) * 299 + u32::from(green) * 587 + u32::from(blue) * 114) / 1000 > 140 {
        Color::Rgb(20, 20, 24)
    } else {
        Color::Rgb(240, 240, 244)
    }
}

/// A label / button / pill of COLOR: a dim tint of it without the focus, the full color with it.
fn chip(color: Color, focus: bool) -> Style {
    if focus {
        return Style::default().bg(color).fg(on(color)).add_modifier(Modifier::BOLD);
    }

    let pal = theme::pal();

    Style::default()
        .bg(theme::tint(color, pal.base, 0.22))
        .fg(theme::tint(color, pal.text, 0.6))
}

/// What the merge button of a PR does.
#[derive(Clone, Debug, PartialEq)]
enum MergeBtn {
    /// Merge now (a merge queue: into it).
    Now,
    /// Auto-merge on: merges (joins the merge queue) when ready.
    WhenReady,
    /// Auto-merge on: off.
    CancelAuto,
    /// In the merge queue: out of it.
    Dequeue,
    /// 0 action: why.
    Off(&'static str),
}

impl MergeBtn {
    fn of(pr: &Pr) -> MergeBtn {
        if pr.queued.is_some() {
            return MergeBtn::Dequeue;
        }

        if pr.auto_merge.is_some() {
            return MergeBtn::CancelAuto;
        }

        match pr.state {
            State::Open => {}
            State::Draft => return MergeBtn::Off("a draft: mark it ready for review first"),
            State::Merged => return MergeBtn::Off("merged: 0 merge"),
            State::Closed => return MergeBtn::Off("closed: 0 merge"),
        }

        if pr.ready() == Ready::Mergeable {
            MergeBtn::Now
        } else if !pr.auto_allowed {
            MergeBtn::Off("auto-merge is off in the repo settings: merge once ready")
        } else {
            MergeBtn::WhenReady
        }
    }

    fn op(&self, pr: &Pr) -> Option<Op> {
        match self {
            MergeBtn::Now => Some(Op::Merge(pr.method.clone(), false)),
            MergeBtn::WhenReady => Some(Op::Merge(pr.method.clone(), true)),
            MergeBtn::CancelAuto => Some(Op::DisableAuto),
            MergeBtn::Dequeue => Some(Op::Dequeue(pr.id.clone())),
            MergeBtn::Off(_) => None,
        }
    }

    /// What it does (a toast), or why not.
    fn verb(&self, pr: &Pr) -> String {
        // `MERGE`: 0 method (0 "merge (merge)")
        let method = if pr.method == "MERGE" {
            String::new()
        } else {
            format!(" ({})", pr.method.to_lowercase())
        };

        match self {
            MergeBtn::Now if pr.queue => "add to the merge queue".into(),
            MergeBtn::Now => format!("merge{method}"),
            MergeBtn::WhenReady if pr.queue => "auto-merge: join the merge queue when ready".into(),
            MergeBtn::WhenReady => format!("merge when ready{method}"),
            MergeBtn::CancelAuto => "auto-merge off".into(),
            MergeBtn::Dequeue => "out of the merge queue".into(),
            MergeBtn::Off(why) => why.to_string(),
        }
    }

    /// The button's text, its color.
    fn label(&self, pr: &Pr) -> (String, Color) {
        let pal = theme::pal();

        let method = if pr.method == "MERGE" {
            String::new()
        } else {
            format!("· {} ", pr.method.to_lowercase())
        };

        match self {
            MergeBtn::Now if pr.queue => (" add to merge queue ".into(), pal.success()),
            MergeBtn::Now => (format!(" merge {method}"), pal.success()),
            MergeBtn::WhenReady if pr.queue => (" queue when ready ".into(), pal.success()),
            MergeBtn::WhenReady => (format!(" merge when ready {method}"), pal.success()),
            MergeBtn::CancelAuto => (" cancel auto-merge ".into(), pal.attention()),
            MergeBtn::Dequeue => (" leave merge queue ".into(), pal.attention()),
            MergeBtn::Off(_) if pr.state == State::Draft => (" merge when ready ".into(), pal.overlay0),
            MergeBtn::Off(_) => (" merge ".into(), pal.overlay0),
        }
    }
}

/// A merge-state op on its way: the button's text.
fn busy_label(op: &Op) -> Option<&'static str> {
    match op {
        Op::Merge(_, false) => Some(" merging… "),
        Op::Merge(_, true) => Some(" auto-merge on… "),
        Op::DisableAuto => Some(" auto-merge off… "),
        Op::Dequeue(_) => Some(" leaving queue… "),
        _ => None,
    }
}

fn secs(total: u64) -> String {
    match total {
        0 => String::new(),
        _ if total < 60 => format!("{total}s"),
        _ if total < 3600 => format!("{}m {}s", total / 60, total % 60),
        _ => format!("{}h {}m", total / 3600, (total % 3600) / 60),
    }
}

fn shift(marks: Vec<usize>, by: usize) -> Vec<usize> {
    marks.into_iter().map(|x| x + by).collect()
}

/// `CI / lint` of check C.
fn check_name(check: &gh::Check) -> String {
    if check.group.is_empty() || check.group == "GitHub Actions" {
        check.name.clone()
    } else {
        format!("{} / {}", check.group, check.name)
    }
}

/// The PRs of sources S: each checkout's branch (as head), then the log's links; 0 dupes.
fn associate(sources: &Sources) -> Result<Vec<PrRef>, String> {
    let mut out: Vec<PrRef> = Vec::new();
    let mut err = None;

    for (dir, stored) in &sources.checkouts {
        associate_repo(&mut out, &mut err, dir, stored.as_deref());
    }

    for link in &sources.links {
        if !out.contains(link) {
            out.push(link.clone());
        }
    }

    match err {
        Some(error) if out.is_empty() => Err(error),
        _ => Ok(out),
    }
}

fn associate_repo(out: &mut Vec<PrRef>, err: &mut Option<String>, dir: &PathBuf, stored: Option<&str>) {
    let Some(repo) = gh::repo_of_dir(dir) else {
        return;
    };

    let git = |args: &[&str]| {
        std::process::Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .stdin(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .output()
            .ok()
            .filter(|output| output.status.success())
            .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
    };

    let branch = git(&["rev-parse", "--abbrev-ref", "HEAD"])
        .filter(|head| head != "HEAD")
        .or_else(|| stored.map(str::to_string));

    let Some(branch) = branch else {
        return;
    };

    let default = git(&["symbolic-ref", "--short", "refs/remotes/origin/HEAD"])
        .map(|origin_head| origin_head.trim_start_matches("origin/").to_string());

    // the default branch: its PRs are other people's
    if default.as_deref() == Some(branch.as_str()) || matches!(branch.as_str(), "main" | "master") {
        return;
    }

    match gh::for_branch(&repo, &branch) {
        Ok(numbers) => {
            push_new_prs(out, &repo, numbers);
        }
        Err(error) => *err = Some(error),
    }
}

fn push_new_prs(out: &mut Vec<PrRef>, repo: &str, numbers: Vec<u64>) {
    for number in numbers {
        let pr = PrRef::new(repo, number);

        if !out.contains(&pr) {
            out.push(pr);
        }
    }
}

#[cfg(test)]
#[path = "merge_tests.rs"]
pub(crate) mod tests;

pub(crate) use crate::infra::browser::open_url;

#[path = "merge_draw.rs"]
mod draw;
#[path = "merge_draw_helpers.rs"]
mod draw_helpers;
#[path = "merge_key.rs"]
mod key;
#[path = "merge_key_helpers.rs"]
mod key_helpers;
#[path = "merge_mouse.rs"]
mod mouse;
#[path = "merge_poll.rs"]
mod poll;
#[path = "merge_updates.rs"]
mod updates;

#[path = "merge_setup.rs"]
mod setup;

#[path = "merge_requests.rs"]
mod requests;

#[path = "merge_context.rs"]
mod context;

#[path = "merge_navigation.rs"]
mod navigation;

#[path = "merge_overview.rs"]
mod overview;

impl Row {
    pub(super) fn new(line: Line<'static>) -> Row {
        Row {
            line,
            stops: Vec::new(),
            input: 0,
        }
    }
}

struct MergeLayout {
    /// Last draw: click targets, the view's area, the doc rows' screen rows.
    hits: Vec<(Rect, Stop)>,
    tab_hits: Vec<(Rect, usize)>,
    sub_hits: Vec<(Rect, Sub)>,
    menu_hits: Vec<(Rect, usize)>,
    area: Rect,
    body_area: Rect,
    edit_rect: Rect,
}

#[derive(Default)]
struct EditState {
    edit: Option<(Edit, TextArea)>,
    menu: Option<Menu>,
    /// A 1st enter on a stop that changes the PR at once (remove a label, update prompt, merge,
    /// auto-merge off, close): a 2nd enter on it within ARM acts.
    armed: Option<(Stop, Instant)>,
    /// The last esc in an edit: a 2nd within 2 s drops edited text.
    esc_at: Option<Instant>,
    /// An edit on its way.
    busy: Option<Op>,
}

#[derive(Default)]
struct ReadState {
    read_at: Option<Instant>,
    reading: bool,
    fast_until: Option<Instant>,
    /// The disk cache of the PR was read (once per tab).
    cached: bool,
}

#[cfg(test)]
#[path = "merge_actions_tests.rs"]
mod action_tests;
#[cfg(test)]
#[path = "merge_test_support.rs"]
mod test_support;

use navigation::{PendingAct, resolve_pending};
