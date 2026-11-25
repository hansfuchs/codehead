//! The thread as the view shows it: the event log folded into items (messages, tool runs, asks),
//! then drawn as rows of a width. Tool calls of an agent between 2 of its messages are 1 run:
//! 1 row, opened by a click or ctrl+o.

pub(crate) mod chat_search;

pub(crate) mod ask;

use std::collections::{HashMap, HashSet};
use std::path::Path;

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use serde_json::Value;

use crate::engine::store::{Event, Kind};
use crate::features::chat::ask::Picker;
use crate::ui::markdown::{self, Row};
use crate::ui::theme;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

#[derive(Clone, Debug)]
pub(crate) struct Call {
    pub id: String,
    pub name: String,
    pub input: Value,
    /// (ok, output) once the result came.
    pub output: Option<(bool, String)>,
}

#[derive(Clone, Debug)]
pub(crate) enum Item {
    /// FROM `owner` or an agent, to agents.
    Prompt {
        from: String,
        to: Vec<String>,
        text: String,
        ts: u64,
    },
    /// Agent text; THOUGHTS: the agent's thoughts right before it (KEY: their fold, 0: none).
    Text {
        agent: String,
        text: String,
        ts: u64,
        thoughts: Vec<String>,
        key: usize,
    },
    /// Tool calls of an agent between 2 of its messages; THOUGHTS: (calls before it, text).
    Run {
        agent: String,
        key: usize,
        calls: Vec<Call>,
        start: u64,
        end: u64,
        thoughts: Vec<(usize, String)>,
    },
    Note {
        agent: String,
        text: String,
        color: Color,
    },
    /// AGENT got the edits of others since its last turn: TEXT drawn as 1 row (cut with …).
    Edits {
        agent: String,
        text: String,
    },
    /// The owner's shell command (`!` in the prompt): its output, its exit code.
    Shell {
        command: String,
        output: String,
        code: i32,
        ts: u64,
        dir: Option<String>,
    },
    /// An answered ask: what AGENT asked (QUESTION), the owner's ANSWER (allow false: declined /
    /// denied). ASKED, TS: the times of the ask and of the answer.
    Settled {
        agent: String,
        question: String,
        answer: String,
        allow: bool,
        permission: bool,
        asked: u64,
        ts: u64,
    },
    /// A subagent of AGENT (its master) ended: its report (done / failed) or why it ended (lost,
    /// stopped); a message of the master. SUB: its index in `Thread::subs`; KEY: the fold of a
    /// long report.
    Sub {
        agent: String,
        sub: usize,
        ts: u64,
        key: usize,
    },
    /// A sub-thread (`>sub`) ended: a message of AGENT (its agent) tagged `subthread`. STATE
    /// `done`: TEXT its result (its last message); `failed` / `stopped`: TEXT why. TITLE: its task.
    Kid {
        agent: String,
        title: String,
        state: String,
        text: String,
        ts: u64,
    },
}

#[derive(Default)]
pub(crate) struct Thread {
    pub items: Vec<Item>,
    pub asks: Vec<Ask>,
    /// Agents in a turn now.
    pub busy: HashSet<String>,
    /// Per agent: last status (state, note).
    pub status: HashMap<String, (String, String)>,
    /// Per agent: model of the last session.
    pub models: HashMap<String, String>,
    /// Open run index per agent (in items).
    open_run: HashMap<String, usize>,
    runs: usize,
    /// Per agent: thoughts not placed yet (the next tool call or message takes them).
    pending: HashMap<String, Vec<String>>,
    /// Fold keys of the messages with thoughts.
    thought_keys: usize,
    /// The subagents of the agents, in start order.
    pub subs: Vec<Sub>,
    /// Per agent stopped on a usage limit: when the host resumes it (unix ms).
    pub resumes: HashMap<String, u64>,
}

/// What a row of the stream does on a click.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Hit {
    Run(usize),
    /// Pick up of lost subagent I (index in `Thread::subs`).
    PickUp(usize),
    /// The thoughts above message KEY.
    Thought(usize),
    /// Choice I of the open ask.
    Choice(usize),
}

/// What is open in the stream: runs (`ALL_RUNS`: every run), thoughts (`ALL_THOUGHTS`: every one).
#[derive(Clone, Copy)]
pub(crate) struct Fold<'a> {
    pub runs: &'a HashSet<usize>,
    pub all_runs: bool,
    pub thoughts: Thoughts,
    pub open_thoughts: &'a HashSet<usize>,
    pub all_thoughts: bool,
}

impl Fold<'_> {
    fn run(&self, key: usize) -> bool {
        self.all_runs || self.runs.contains(&key)
    }

    /// The thoughts above message KEY: open.
    fn thought(&self, key: usize) -> bool {
        self.thoughts == Thoughts::Open || self.all_thoughts || self.open_thoughts.contains(&key)
    }
}

/// The drawn stream: rows, the clickable ones.
#[derive(Default)]
pub(crate) struct Drawn {
    pub rows: Vec<Row>,
    pub hits: HashMap<usize, Hit>,
    /// Where the view draws the working wave (3 cols): row, col, color (bubbles: the box of an
    /// agent at work).
    pub waves: Vec<(usize, u16, Color)>,
    /// Per item: its 1st row (the scroll anchor of a view scrolled back).
    pub starts: Vec<usize>,
}

/// Colors of the agents (name → color).
pub(crate) type Colors = HashMap<String, Color>;

/// Draw T at WIDTH in LOOK. FOLD: what is open (runs, thoughts). `ASK_AT`: the choice under the
/// cursor of the 1st open ask; PICKS: its picked choices (multi).
pub(crate) fn draw(
    thread: &Thread,
    look: Look,
    width: u16,
    colors: &Colors,
    roster: &[(String, String)],
    fold: &Fold,
    ask: &Picker,
) -> Drawn {
    // `@name` of an agent: in its color, in every message and note of this draw
    let mut marks: Vec<(String, Color)> = colors
        .iter()
        .map(|(count, character)| (count.clone(), *character))
        .collect();

    marks.sort_by(|agent, bytes| agent.0.cmp(&bytes.0));
    markdown::set_marks(marks);

    let directory = match look {
        Look::Lines => draw_lines(thread, width, colors, roster, fold, ask),
        Look::Bubbles => draw_bubbles(thread, width, colors, roster, fold, ask),
    };

    memo::sweep();

    directory
}

mod memo;

#[cfg(test)]
#[path = "chat_tests.rs"]
mod tests;

/// Draw time of a thread on disk: `CODEHEAD_BENCH=<thread id> cargo test --release bench_draw -- --ignored --nocapture`.
#[cfg(test)]
#[path = "chat_bench_tests.rs"]
mod bench;

#[cfg(test)]
#[path = "chat_resume_tests.rs"]
mod resume_tests;

pub(crate) use crate::domain::chat::*;

mod events;

mod rows;
use rows::ask_rows;
use rows::run_thought_rows;
use rows::sub_rows;
use rows::thought_rows;

mod metadata;
use metadata::agents_on;
use metadata::badge;
use metadata::color_of;
use metadata::edits_row;
use metadata::error_hint;
use metadata::resume_of;
use metadata::thought_count;

mod calls;
use calls::call_rows;
use calls::run_lines;
use calls::run_summary;
use calls::tool_label;

mod draw_lines;
use draw_lines::draw_lines;

mod bubbles;
use bubbles::KID;
use bubbles::bottom_border;
use bubbles::kid_rows;
use bubbles::kid_tag;
use bubbles::minute_divider;
use bubbles::owner_bubble;
use bubbles::top_border;

mod draw_bubbles;
use draw_bubbles::draw_bubbles;

mod attachments;
use attachments::clip_row;
use attachments::link_markers;
#[cfg(test)]
use attachments::widest;

mod shell;
use shell::shell_rows;

/// A box under construction: its 1st row, the style of its border.
pub(super) struct Open {
    start: usize,
    border: Style,
    agent: Option<String>,
    /// The minute (local clock `HH:MM`) of the box's last item: an item of a later minute gets a
    /// divider with its time.
    minute: String,
    /// The label of its bottom border (the agent's name): 0 spans, a plain bottom border.
    name: Vec<Span<'static>>,
    /// The agent at work: the wave next to the name, in this color.
    wave: Option<Color>,
}
