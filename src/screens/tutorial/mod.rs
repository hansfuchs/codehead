//! The setup tutorial: real threads in a sandbox, the real thread view (and the real new-thread
//! form) in a frame, a quest panel right of it: per quest a checklist, ✓ per task done, the keys
//! of the next task. The agents are scripted (0 tokens, the same every time) but run the real
//! way: ACP processes under a real host, real edits in a real git repo, `send` between them,
//! flow steps reported with `flow_step`, the real changes tab.
//!
//! Quests, in the order of 1 real task: basics: 1 start a task (send it with a style guide
//! attached, the attachment in the files view) · 2 work with the crew (@mention + #shortcut,
//! watch the review) · 3 check and commit (changes view, a line in the IDE (mocked), stage
//! hello.md only, commit). Then the owner may stop (esc esc: done). Advanced: 4 side questions
//! (`>sub`) · 5 ship it (the lead posts a mock PR, merge view, merge) · 6 many threads (ctrl+p,
//! the seeded thread) · 7 new thread with a flow (home repo, notes, the flow bar) · 8 finish.
//! The quest panel shows the current quest only: its concept (what, when), tasks, docs link.
//!
//! Mocks ($`CODEHEAD_TUTORIAL`, see [`active`]): 0 IDE opens (a toast), 0 browser opens, 0 GitHub
//! call (gh fetch / diff / run / labels answer with the mock PR of the sandbox repo).
//!
//! - `codehead tutorial <spec json>`: the wizard runs it as a child process. The sandbox (temp
//!   dir: data, config, git repo) lives in its env only: the owner's threads, agents and settings
//!   stay untouched. Exit: 0 done (next screen), 10 back, 130 quit. The sandbox goes at the end.
//! - `codehead tutorial --check <spec json>`: quest 1 without a screen (events printed).
//! - `codehead tutor-agent role=<role> builder=<name> reviewer=<name>`: 1 scripted agent, ACP
//!   over stdin / stdout.

pub(crate) mod wizard;

use std::collections::HashMap;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::{Terminal, TerminalOptions, Viewport};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::engine::store::{self, Event, Kind};
use crate::ui::theme;

/// An agent of the tutorial: the wizard's crew member.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Member {
    pub name: String,
    pub role: String,
    pub harness: String,
    pub avatar: String,
    pub color: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Spec {
    pub dry: bool,
    pub crew: Vec<Member>,
}

/// Exit codes of `codehead tutorial`.
pub(crate) const DONE: i32 = 0;
pub(crate) const BACK: i32 = 10;
pub(crate) const QUIT: i32 = 130;

/// The widest view: narrower than the split (changes + chat), so the changes tab is a tab.
const VIEW_MAX: u16 = 130;
/// The quest panel: a 3rd of the width, within these; under `PANEL_FROM` columns: 0 panel, the
/// task on the top row.
const PANEL_MIN: u16 = 40;
const PANEL_MAX: u16 = 60;
const PANEL_FROM: u16 = 100;
/// Under this size: the tutorial asks for a bigger window first.
pub(crate) const MIN_W: u16 = 130;
pub(crate) const MIN_H: u16 = 34;
/// The 1st prompt, ready in the input: the style guide attached as a paste.
pub(crate) const TASK: &str = "make hello.md say hi to the crew. style: [Pasted text #1 +3 lines]";
/// The attachment's marker in TASK.
const MARKER: &str = "[Pasted text #1 +3 lines]";
/// The attachment: a style guide (the reviewer's nit follows its last rule).
const STYLE: &str = "- lower case\n- 1 line per idea\n- a newline at the end\n";
/// The mock PR's link, as the lead posts it.
const PR_URL: &str = "https://github.com/crew/hello/pull/7";
/// The seeded 2nd thread.
const SEEDED: &str = "fix the readme";
/// The new thread's prompt, ready in the form.
const FLOW_TASK: &str = "add a line on flows to hello.md";

/// 1 task of a quest: done when the owner did it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Task {
    /// The 1st owner message.
    Send,
    /// The files view open (a click on the message's attachment).
    Attach,
    /// An owner message to the reviewer (solo: a 2nd owner message), a #shortcut in it.
    Mention,
    /// The reviewer approves after the builder's fix.
    Watch,
    /// The changes tab open.
    Changes,
    /// An IDE open asked from the changes view (mocked: a toast).
    Ide,
    /// hello.md staged.
    Stage,
    /// A commit of the owner.
    Commit,
    /// A sub-thread started (`>sub`).
    Sub,
    /// The sub-thread's result back.
    SubDone,
    /// An agent posted the PR link.
    AskPr,
    /// The merge view open.
    MergeView,
    /// The mock PR merged.
    Merged,
    /// The menu open (ctrl+p).
    Menu,
    /// The seeded thread open.
    Switch,
    /// The menu open again.
    Menu2,
    /// The new-thread form open (n in the menu).
    New,
    /// A flow picked in the form.
    Flow,
    /// The new thread started.
    Start,
    /// The flow finished.
    FlowRun,
    /// esc esc.
    Leave,
}

pub(crate) struct Quest {
    pub title: &'static str,
    pub tasks: Vec<Task>,
    /// The concept: what it is, when to use it (short lines of the docs).
    pub about: &'static [&'static str],
    /// Its docs page (under DOCS).
    pub doc: &'static str,
    /// After the basics: the owner can stop before it.
    pub advanced: bool,
}

/// The docs root of the quests' links.
const DOCS: &str = "codehead.sh/docs/";

pub(crate) struct Coach {
    pub quests: Vec<Quest>,
    done: Vec<Task>,
    lead: String,
    builder: String,
    reviewer: Option<String>,
    owner_msgs: usize,
    fixed: bool,
    /// The last read of the repo (staged, commits, the merge marker): max. 1 per `REPO_EVERY`.
    repo_at: Option<std::time::Instant>,
}

/// How often the coach reads the repo's git state.
const REPO_EVERY: Duration = Duration::from_millis(700);

/// How the new-thread form ended.
enum Form {
    /// The new thread (its id).
    Started(String),
    /// esc esc, or the form closed.
    Back,
    /// ctrl+c ctrl+c.
    Quit,
}

/// The script's pace: slow enough to follow each step on screen.
const WORD: Duration = Duration::from_millis(70);
const TOOL: Duration = Duration::from_millis(1200);
const THINK: Duration = Duration::from_millis(1200);
/// Between 2 flow steps: the flow bar's move stays visible.
const STEP: Duration = Duration::from_millis(1500);

struct Agent {
    out: io::Stdout,
    session: String,
    cwd: PathBuf,
    me: String,
    thread: String,
    next: u64,
}

#[cfg(test)]
#[path = "tutorial_tests.rs"]
mod tests;

use crate::infra::tutorial::{git_out, mock_merged};

#[cfg(test)]
use crate::infra::tutorial::{mock_diff, mock_pr, mock_run};

#[path = "tutorial_tick.rs"]
mod tick;
#[path = "tutorial_updates.rs"]
mod updates;

#[path = "tutorial_coach.rs"]
mod coach;

#[path = "tutorial_sandbox.rs"]
mod sandbox;

use sandbox::{run_dir, sandbox, sandbox_root, style_path, sweep};

#[path = "tutorial_layout.rs"]
mod layout;

use layout::{layout, size, size_gate, wipe, wrap};

#[path = "tutorial_panel.rs"]
mod panel;

use panel::chrome;

#[path = "tutorial_view.rs"]
mod view;

pub(crate) use view::run;

#[path = "tutorial_form.rs"]
mod form;

use form::{new_thread, start_thread};

#[path = "tutorial_scenario.rs"]
mod scenario;

use scenario::headless;

#[path = "tutorial_agents.rs"]
mod agents;

pub(crate) use agents::agent;

#[path = "tutorial_quests.rs"]
mod quests;
use quests::make_quests;

#[cfg(test)]
use agents::{asks_pr, step_of};
